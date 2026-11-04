//! Loads the watcher's programs into the kernel and reads what they send. Linux only, and only
//! in a Borg built with the programs.

use anyhow::{Context, Result};
use aya::{
    Ebpf, EbpfLoader,
    maps::{MapData, PerCpuArray, RingBuf},
    programs::TracePoint,
};
use rustix::{
    event::{PollFd, PollFlags, Timespec},
    io::Errno,
    process::{Resource, Rlimit, getrlimit, setrlimit},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{Receiver, Sender, channel};

use super::{KERNEL_BTF, Namespace, Offsets, btf::Layout};
use crate::watch::{EVENTS_QUEUED, Executed, facts};

/// How long the reader waits for records before it looks at what the kernel had no room for.
const WAKE_EVERY: Timespec = Timespec { tv_sec: 1, tv_nsec: 0 };
/// How often the reader says that the kernel had no room for program starts.
const DROPS_SAID_EVERY: Duration = Duration::from_secs(60);

/// Loads the programs, and sends every program start they report, with what they read.
pub fn listen() -> Result<Receiver<Executed>> {
    let (ebpf, ring, dropped) = load().context("loading the watcher's eBPF programs")?;
    let (sender, receiver) = channel(EVENTS_QUEUED);
    std::thread::spawn(move || read(ebpf, ring, &dropped, &sender));
    Ok(receiver)
}

type Loaded = (Ebpf, RingBuf<MapData>, PerCpuArray<MapData, u64>);

fn load() -> Result<Loaded> {
    let tracefs = super::tracefs().context("found no tracefs that Borg may read")?;
    let offsets = Offsets::read(&tracefs)?;
    let namespace = Namespace::own(Path::new(facts::PROC))?;
    let layout = match std::fs::read(KERNEL_BTF) {
        Ok(btf) => Layout::read(&btf),
        Err(error) => {
            eprintln!("borg watch: reading {KERNEL_BTF}: {error}");
            None
        }
    };
    if layout.is_none() {
        eprintln!("borg watch: the kernel does not say where it keeps command lines; they come from /proc");
    }
    let layout = layout.unwrap_or(Layout { task_mm: 0, mm_arg_start: 0, mm_arg_end: 0 });
    raise_memlock();
    let mut ebpf = EbpfLoader::new()
        .override_global("EXEC_FILENAME", &offsets.exec_filename, true)
        .override_global("FORK_CHILD_PID", &offsets.fork_child_pid, true)
        .override_global("TASK_MM", &layout.task_mm, true)
        .override_global("MM_ARG_START", &layout.mm_arg_start, true)
        .override_global("MM_ARG_END", &layout.mm_arg_end, true)
        .override_global("PIDNS_DEV", &namespace.dev, true)
        .override_global("PIDNS_INO", &namespace.ino, true)
        .load(aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/borg-watch-ebpf")))?;
    // Parents first, so a process that starts a program while the rest attach has its parent.
    attach(&mut ebpf, "borg_fork", "sched", "sched_process_fork")?;
    attach(&mut ebpf, "borg_exit", "sched", "sched_process_exit")?;
    attach(&mut ebpf, "borg_exec", "sched", "sched_process_exec")?;
    let ring = RingBuf::try_from(ebpf.take_map("EVENTS").context("the programs have no EVENTS map")?)?;
    let dropped = PerCpuArray::try_from(ebpf.take_map("DROPPED").context("the programs have no DROPPED map")?)?;
    Ok((ebpf, ring, dropped))
}

fn attach(ebpf: &mut Ebpf, name: &str, category: &str, event: &str) -> Result<()> {
    let program: &mut TracePoint =
        ebpf.program_mut(name).with_context(|| format!("the programs have no {name}"))?.try_into()?;
    program.load().with_context(|| format!("loading {name}"))?;
    program.attach(category, event).with_context(|| format!("attaching {name} to {category}/{event}"))?;
    Ok(())
}

/// Kernels before 5.11 count the programs' memory as locked memory, which is limited to a few
/// pages for most processes. The service's unit raises the limit; this takes what it allows.
fn raise_memlock() {
    let limit = getrlimit(Resource::Memlock);
    let _ = setrlimit(Resource::Memlock, Rlimit { current: limit.maximum, maximum: limit.maximum });
}

/// Reads the ring buffer until the watcher stops listening. The programs stay in the kernel
/// while `ebpf` lives, and leave it with this thread.
fn read(ebpf: Ebpf, mut ring: RingBuf<MapData>, dropped: &PerCpuArray<MapData, u64>, sender: &Sender<Executed>) {
    let _attached = ebpf;
    let mut said = 0_u64;
    let mut said_at: Option<Instant> = None;
    loop {
        let waited = {
            let mut ready = [PollFd::new(&ring, PollFlags::IN)];
            rustix::event::poll(&mut ready, Some(&WAKE_EVERY))
        };
        match waited {
            Ok(_) | Err(Errno::INTR) => {}
            Err(error) => {
                eprintln!("borg watch: the watcher's programs stopped: {error}");
                return;
            }
        }
        while let Some(record) = ring.next() {
            let started = super::parse(&record);
            // The kernel may write over a record only once Borg lets it go, so it goes before
            // the wait for the watcher.
            drop(record);
            if let Some((pid, seen)) = started
                && sender.blocking_send(Executed { pid, seen: Some(Box::new(seen)) }).is_err()
            {
                return;
            }
        }
        let lost: u64 = dropped.get(&0, 0).map(|counts| counts.iter().sum()).unwrap_or(0);
        if lost > said && said_at.is_none_or(|at| at.elapsed() >= DROPS_SAID_EVERY) {
            eprintln!("borg watch: the kernel had no room for {} program starts", lost - said);
            said = lost;
            said_at = Some(Instant::now());
        }
    }
}
