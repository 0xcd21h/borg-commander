//! The watcher's own programs inside the Linux kernel, and what Borg makes of what they send.
//!
//! The programs in `borg-watch-ebpf` run at every program start, inside the kernel, before the
//! new program runs a single instruction. They send the path of the program, the process that
//! started it, the user, and the command line the kernel has just copied into the program's
//! memory. Borg then reads `/proc` for the rest, as it does for the kernel's plain process
//! events, and trusts what the programs read wherever `/proc` has moved on: a command that has
//! already ended, a process that has started another program since, or one that rewrote its
//! command line.
//!
//! Loading them needs Linux 5.8 or later and the `CAP_BPF` and `CAP_PERFMON` capabilities. Where
//! that fails, the watcher falls back to the kernel's process events. The command line needs the
//! kernel's BTF as well, which tells where the kernel keeps it; without BTF it comes from `/proc`.

// Only the Linux loader reads records and tracefs. The tests read them everywhere.
#![cfg_attr(not(borg_ebpf), allow(dead_code))]

use anyhow::{Context, Result, bail};
use borg_core::{ProcessKey, StartedProcess};
use borg_watch_ebpf::{ARGS_BYTES, COMM_BYTES, EXEC, Exec, FILENAME_BYTES};
use std::{
    collections::HashMap,
    fs,
    mem::{offset_of, size_of},
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::facts::Processes;

/// Where the kernel describes its tracepoints, on systems that mount tracefs on its own and on
/// older ones that keep it under debugfs.
const TRACEFS: &[&str] = &["/sys/kernel/tracing", "/sys/kernel/debug/tracing"];
/// The inode of the machine's own pid namespace, which the kernel gives every machine.
const INIT_PID_NAMESPACE: u64 = 0xEFFF_FFFC;
/// Linux gives process start times in `/proc` in hundredths of a second on every architecture
/// Borg ships for.
const NANOS_PER_TICK: u64 = 10_000_000;
/// Where the kernel publishes the layout of its own types.
pub const KERNEL_BTF: &str = "/sys/kernel/btf/vmlinux";

/// What the programs inside the kernel read as a process started a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The process that started it; zero when the programs did not see it start.
    pub parent: u32,
    pub uid: u32,
    /// When the process started, in nanoseconds since the machine booted; zero when unknown.
    pub forked_ns: u64,
    /// When it started the program.
    pub exec_ns: u64,
    /// The name the process took from the program.
    pub name: String,
    /// The path the program was started by, as it was passed: absolute, or relative to the
    /// process's working folder.
    pub program: PathBuf,
    /// The command line as the kernel copied it into the new program, when the programs read it.
    pub command: Option<Vec<String>>,
    /// The command line had more than the programs keep.
    pub truncated: bool,
}

impl Seen {
    /// The process as Borg reports it: what `/proc` says, corrected by what the programs read
    /// at the moment it started, or what the programs read alone once `/proc` has nothing left
    /// of it.
    pub fn started(&self, processes: &Processes, pid: u32, at: SystemTime) -> StartedProcess {
        let read = processes.started(pid, at).filter(|process| self.same_process(process.key));
        match read {
            Some(mut process) => {
                self.correct(&mut process, processes);
                process
            }
            None => self.alone(processes, pid, at),
        }
    }

    /// Whether `/proc` still shows the process the programs saw, and not a later one that was
    /// given its number: that one started after this one started its program.
    fn same_process(&self, key: ProcessKey) -> bool {
        key.start <= ticks(self.exec_ns) + 1
    }

    /// Whether `/proc` shows a later program of the process: it took another name, and the file
    /// it runs is not where this path leads. A process may rename itself without starting
    /// anything, and a script keeps its own name while it runs its interpreter. A process that
    /// runs a deleted file stays as `/proc` shows it, so renaming itself cannot hide that the
    /// file is gone.
    fn moved_on(&self, process: &StartedProcess, processes: &Processes) -> bool {
        process.name != self.name
            && !process.program_deleted
            && processes.runs(process.key.pid, &self.program) != Some(true)
    }

    fn correct(&self, process: &mut StartedProcess, processes: &Processes) {
        if self.moved_on(process, processes) {
            process.name.clone_from(&self.name);
            process.program = self.program();
            process.command = self.command.clone().unwrap_or_default();
            process.link = process.program.as_deref().and_then(|program| {
                processes.started_by_link(process.key.pid, &process.name, &process.command, program)
            });
        } else if let Some(command) = &self.command
            && (!self.truncated || process.command.is_empty())
        {
            process.command.clone_from(command);
        }
    }

    /// A process that ended before Borg could read it: only what the programs read, and the
    /// processes above it as they are now.
    fn alone(&self, processes: &Processes, pid: u32, at: SystemTime) -> StartedProcess {
        let started = if self.forked_ns > 0 { self.forked_ns } else { self.exec_ns };
        StartedProcess {
            key: ProcessKey { pid, start: ticks(started) },
            name: self.name.clone(),
            program: self.program(),
            command: self.command.clone().unwrap_or_default(),
            user: super::user_name(self.uid),
            uid: self.uid,
            ancestors: processes.ancestors(self.parent),
            at,
            ..StartedProcess::default()
        }
    }

    fn program(&self) -> Option<PathBuf> {
        (!self.program.as_os_str().is_empty()).then(|| self.program.clone())
    }
}

/// Nanoseconds since boot in the clock ticks `/proc` counts process starts in.
pub fn ticks(nanos: u64) -> u64 {
    nanos / NANOS_PER_TICK
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// Text the kernel keeps in a fixed field, up to its first zero byte.
fn text_at(bytes: &[u8], at: usize, length: usize) -> Option<String> {
    let field = bytes.get(at..at + length)?;
    let end = memchr::memchr(0, field).unwrap_or(field.len());
    Some(String::from_utf8_lossy(&field[..end]).into_owned())
}

/// Reads a program start as the programs lay it out: the process and what they read. A record
/// that is short or of a kind Borg does not know is nothing.
pub fn parse(bytes: &[u8]) -> Option<(u32, Seen)> {
    if u32_at(bytes, 0)? != EXEC || bytes.len() < size_of::<Exec>() {
        return None;
    }
    let command = if u32_at(bytes, offset_of!(Exec, args_read))? != 0 {
        let length = usize::try_from(u32_at(bytes, offset_of!(Exec, args_length))?).ok()?.min(ARGS_BYTES);
        let start = offset_of!(Exec, args);
        Some(super::facts::command_of(bytes.get(start..start + length)?))
    } else {
        None
    };
    let seen = Seen {
        parent: u32_at(bytes, offset_of!(Exec, parent))?,
        uid: u32_at(bytes, offset_of!(Exec, uid))?,
        forked_ns: u64_at(bytes, offset_of!(Exec, forked_ns))?,
        exec_ns: u64_at(bytes, offset_of!(Exec, exec_ns))?,
        name: text_at(bytes, offset_of!(Exec, comm), COMM_BYTES)?,
        program: PathBuf::from(text_at(bytes, offset_of!(Exec, filename), FILENAME_BYTES)?),
        command,
        truncated: u32_at(bytes, offset_of!(Exec, args_truncated))? != 0,
    };
    Some((u32_at(bytes, offset_of!(Exec, pid))?, seen))
}

/// Where each field the programs read sits in its tracepoint's record, from the kernel's own
/// description of the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offsets {
    pub exec_filename: u32,
    pub fork_child_pid: u32,
}

/// The fields a tracepoint's `format` file describes, by name, each with its offset and size.
fn fields(format: &str) -> HashMap<String, (u32, u32)> {
    format
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().split(';').map(str::trim);
            let declaration = parts.next()?.strip_prefix("field:")?;
            let name = declaration.split_whitespace().last()?.trim_start_matches('*');
            let name = name.split('[').next()?;
            let mut offset = None;
            let mut size = None;
            for part in parts {
                if let Some(value) = part.strip_prefix("offset:") {
                    offset = value.parse().ok();
                } else if let Some(value) = part.strip_prefix("size:") {
                    size = value.parse().ok();
                }
            }
            Some((name.to_owned(), (offset?, size?)))
        })
        .collect()
}

/// The offset of one field, which must be as big as the programs read it.
fn field(tracefs: &Path, event: &str, name: &str, size: u32) -> Result<u32> {
    let path = tracefs.join("events").join(event).join("format");
    let format = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    match fields(&format).get(name) {
        Some(&(offset, found)) if found == size => Ok(offset),
        Some(&(_, found)) => bail!("{event} has {name} of {found} bytes, where Borg reads {size}"),
        None => bail!("{event} has no field {name}"),
    }
}

impl Offsets {
    pub fn read(tracefs: &Path) -> Result<Self> {
        Ok(Self {
            exec_filename: field(tracefs, "sched/sched_process_exec", "filename", 4)?,
            fork_child_pid: field(tracefs, "sched/sched_process_fork", "child_pid", 4)?,
        })
    }
}

/// The first tracefs that describes the scheduler's tracepoints.
pub fn tracefs() -> Option<PathBuf> {
    TRACEFS.iter().map(PathBuf::from).find(|path| path.join("events/sched/sched_process_exec").is_dir())
}

/// The pid namespace Borg runs in, as the programs compare it: zero for the machine's own,
/// where they report every process by the kernel's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Namespace {
    pub dev: u64,
    pub ino: u64,
}

impl Namespace {
    /// From the device and inode of `/proc/self/ns/pid`. The kernel numbers devices its own way
    /// inside, with the major number above the low 20 bits.
    pub fn from_stat(major: u32, minor: u32, ino: u64) -> Self {
        if ino == INIT_PID_NAMESPACE {
            return Self::default();
        }
        Self { dev: (u64::from(major) << 20) | u64::from(minor), ino }
    }

    pub fn own(proc: &Path) -> Result<Self> {
        let path = proc.join("self/ns/pid");
        let stat = rustix::fs::stat(&path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self::from_stat(rustix::fs::major(stat.st_dev), rustix::fs::minor(stat.st_dev), stat.st_ino))
    }
}

mod btf;

#[cfg(borg_ebpf)]
mod load;
#[cfg(borg_ebpf)]
pub use load::listen;

/// This Borg was built without the programs, on a system other than Linux or on purpose.
#[cfg(not(borg_ebpf))]
pub fn listen() -> Result<tokio::sync::mpsc::Receiver<super::Executed>> {
    bail!("this Borg was built without its eBPF programs")
}

#[cfg(test)]
mod tests;
