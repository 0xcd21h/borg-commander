//! What the system says about a process that has just started. On Linux everything comes from
//! `/proc`, which the reader takes as a folder, so its tests run against a folder of fixtures
//! on any system.
//!
//! A process is identified by its number together with the moment it started, in clock ticks
//! since the machine booted, exactly as `/proc` reports it. Both are read again before Borg
//! acts on a process, so a number given to another process in the meantime is left alone.

use borg_core::{Ancestor, PRELOAD_VARIABLES, ProcessKey, StartedProcess, StreamKind};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Where Linux keeps what it knows about each process.
pub const PROC: &str = "/proc";
/// Bytes of a command line or an environment Borg reads.
const BLOCK: u64 = 64 * 1024;
/// Generations of parents an alert keeps.
const ANCESTORS: usize = 8;
/// What a program's link says when its file has been deleted.
const DELETED: &str = " (deleted)";
/// What the links of standard input and output say when they lead to a socket or a pipe.
const SOCKET: &str = "socket:";
const PIPE: &str = "pipe:";
/// Where terminals are.
const TERMINALS: &[&str] = &["/dev/pts/", "/dev/tty", "/dev/console"];
/// The fields of `stat` after the program's name: the state comes first.
const STATE: usize = 0;
const PARENT: usize = 1;
const TERMINAL: usize = 4;
const STARTED_AT: usize = 19;

/// What `/proc/<pid>/stat` says, as far as the watcher needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// The name the process calls itself.
    pub name: String,
    pub parent: u32,
    /// The terminal it belongs to, as a device number; zero when it has none.
    pub terminal: u32,
    /// Clock ticks since the machine booted.
    pub started_at: u64,
}

/// `stat` holds the program's name in brackets, and the name may hold brackets and spaces of
/// its own, so the fields after it start at the last bracket.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_owned();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    if fields.len() <= STARTED_AT || fields.get(STATE).is_none_or(|state| state.is_empty()) {
        return None;
    }
    Some(Stat {
        name,
        parent: fields[PARENT].parse().ok()?,
        terminal: fields[TERMINAL].parse().ok()?,
        started_at: fields[STARTED_AT].parse().ok()?,
    })
}

/// The user a process runs as, from the first number on the `Uid` line of `status`.
pub fn parse_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|values| values.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// The preload variables in an environment, which `/proc` gives as `name=value` with a zero
/// byte after each.
pub fn preloads(environ: &[u8]) -> Vec<(String, String)> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, value)| PRELOAD_VARIABLES.contains(name) && !value.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// What a standard stream's link leads to.
pub fn stream_kind(target: &str) -> StreamKind {
    if target.starts_with(SOCKET) {
        StreamKind::Socket
    } else if target.starts_with(PIPE) {
        StreamKind::Pipe
    } else if TERMINALS.iter().any(|terminal| target.starts_with(terminal)) {
        StreamKind::Terminal
    } else if target.starts_with('/') {
        StreamKind::File
    } else {
        StreamKind::Other
    }
}

/// The program a process runs, and whether its file has been deleted since it started.
pub fn program_of(link: &str) -> (PathBuf, bool) {
    match link.strip_suffix(DELETED) {
        Some(path) => (PathBuf::from(path), true),
        None => (PathBuf::from(link), false),
    }
}

/// A command line, which `/proc` gives as arguments with a zero byte after each.
pub fn command_of(cmdline: &[u8]) -> Vec<String> {
    cmdline
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect()
}

/// Reads what the system knows about processes. Everything it reads may vanish between one
/// read and the next, so every answer is optional rather than an error.
#[derive(Debug, Clone)]
pub struct Processes {
    root: PathBuf,
}

impl Default for Processes {
    fn default() -> Self {
        Self::at(PROC)
    }
}

impl Processes {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn folder(&self, pid: u32) -> PathBuf {
        self.root.join(pid.to_string())
    }

    fn text(&self, pid: u32, name: &str) -> Option<String> {
        fs::read_to_string(self.folder(pid).join(name)).ok()
    }

    /// At most [`BLOCK`] bytes of one of a process's files, which may be closed to Borg.
    fn block(&self, pid: u32, name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let read =
            fs::File::open(self.folder(pid).join(name)).and_then(|file| file.take(BLOCK).read_to_end(&mut bytes));
        if read.is_err() { Vec::new() } else { bytes }
    }

    fn link(&self, pid: u32, name: &str) -> Option<String> {
        fs::read_link(self.folder(pid).join(name)).ok().map(|path| path.to_string_lossy().into_owned())
    }

    pub fn stat(&self, pid: u32) -> Option<Stat> {
        self.text(pid, "stat").as_deref().and_then(parse_stat)
    }

    /// The process with this number, as it is now.
    pub fn key(&self, pid: u32) -> Option<ProcessKey> {
        Some(ProcessKey { pid, start: self.stat(pid)?.started_at })
    }

    /// Whether this is still the same process, and not another one that took its number.
    pub fn running(&self, process: ProcessKey) -> bool {
        self.key(process.pid) == Some(process)
    }

    /// The user a process runs as, from its own report of it.
    pub fn user(&self, pid: u32) -> Option<u32> {
        self.text(pid, "status").as_deref().and_then(parse_uid).or_else(|| self.owner(pid))
    }

    /// Who owns a process's folder, which is one look at the file system rather than a read.
    fn owner(&self, pid: u32) -> Option<u32> {
        fs::metadata(self.folder(pid)).ok().map(|metadata| metadata.uid())
    }

    /// The name of the terminal a process belongs to, when it has one: the first of its
    /// standard streams that leads to a terminal.
    fn terminal(&self, pid: u32, stat: &Stat) -> Option<String> {
        if stat.terminal == 0 {
            return None;
        }
        let named = ["fd/0", "fd/1", "fd/2"]
            .into_iter()
            .filter_map(|stream| self.link(pid, stream))
            .find(|target| stream_kind(target) == StreamKind::Terminal);
        Some(named.unwrap_or_else(|| stat.terminal.to_string()))
    }

    fn stream(&self, pid: u32, stream: &str) -> StreamKind {
        self.link(pid, stream).as_deref().map(stream_kind).unwrap_or_default()
    }

    /// One process in the chain that started another.
    fn ancestor(&self, pid: u32) -> Option<(Ancestor, u32)> {
        let stat = self.stat(pid)?;
        let program = self.link(pid, "exe").map(|link| program_of(&link).0);
        let ancestor = Ancestor {
            pid,
            name: stat.name.clone(),
            program,
            terminal: stat.terminal != 0,
            command: command_of(&self.block(pid, "cmdline")).join(" "),
        };
        Some((ancestor, stat.parent))
    }

    /// Everything the watcher's rules need about a process that has just started, and about
    /// the processes that led to it. `at` is when Borg heard of it.
    pub fn started(&self, pid: u32, at: SystemTime) -> Option<StartedProcess> {
        let stat = self.stat(pid)?;
        let (program, program_deleted) = match self.link(pid, "exe") {
            Some(link) => {
                let (path, deleted) = program_of(&link);
                (Some(path), deleted)
            }
            None => (None, false),
        };
        let uid = self.user(pid).unwrap_or_default();
        let mut ancestors = Vec::new();
        let mut parent = stat.parent;
        while ancestors.len() < ANCESTORS && parent > 0 {
            let Some((ancestor, grandparent)) = self.ancestor(parent) else { break };
            ancestors.push(ancestor);
            if grandparent == parent {
                break;
            }
            parent = grandparent;
        }
        Some(StartedProcess {
            key: ProcessKey { pid, start: stat.started_at },
            name: stat.name.clone(),
            program,
            program_deleted,
            command: command_of(&self.block(pid, "cmdline")),
            user: crate::watch::user_name(uid),
            uid,
            terminal: self.terminal(pid, &stat),
            input: self.stream(pid, "fd/0"),
            output: self.stream(pid, "fd/1"),
            preloads: preloads(&self.block(pid, "environ")),
            ancestors,
            at,
        })
    }

    /// Every process the folder lists, by number.
    pub fn all(&self) -> Vec<u32> {
        let Ok(entries) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut pids: Vec<u32> =
            entries.filter_map(Result::ok).filter_map(|entry| entry.file_name().to_str()?.parse().ok()).collect();
        pids.sort_unstable();
        pids
    }

    /// The processes of one user, by number. Ownership of the process's folder is what the
    /// system says, and one look at it is cheap enough to do several times a second.
    pub fn of_user(&self, uid: u32) -> Vec<u32> {
        self.all().into_iter().filter(|pid| self.owner(*pid) == Some(uid)).collect()
    }

    /// Which process started which, for everything running now.
    pub fn parents(&self) -> BTreeMap<u32, u32> {
        self.all().into_iter().filter_map(|pid| self.stat(pid).map(|stat| (pid, stat.parent))).collect()
    }

    /// A process and everything below it, parents before their children, so stopping them in
    /// this order leaves nothing running that could start more.
    pub fn tree(&self, pid: u32) -> Vec<u32> {
        descendants(&self.parents(), pid)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// `pid` and everything descended from it, each generation after the one above it.
pub fn descendants(parents: &BTreeMap<u32, u32>, pid: u32) -> Vec<u32> {
    let mut tree = vec![pid];
    let mut index = 0;
    while let Some(&parent) = tree.get(index) {
        for (&child, &of) in parents {
            if of == parent && !tree.contains(&child) {
                tree.push(child);
            }
        }
        index += 1;
    }
    tree
}

#[cfg(test)]
mod tests;
