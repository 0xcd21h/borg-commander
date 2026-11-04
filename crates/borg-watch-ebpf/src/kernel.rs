//! The programs the kernel runs at every process start. They only read: which process started
//! which, the program the kernel has just started, its command line and who it runs as. Each
//! sends what it read to Borg through one ring buffer and leaves the kernel as it was.

#![allow(unsafe_code)]

use core::{
    ffi::c_void,
    mem::size_of,
    ptr::{addr_of_mut, read_volatile},
};

use aya_ebpf::{
    EbpfContext,
    bindings::bpf_pidns_info,
    helpers::{
        bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_get_current_uid_gid, bpf_probe_read_kernel,
        bpf_probe_read_kernel_str_bytes,
        generated::{
            bpf_get_current_task, bpf_get_ns_current_pid_tgid, bpf_ktime_get_boot_ns, bpf_probe_read_user as read_user,
        },
    },
    macros::{map, tracepoint},
    maps::{LruHashMap, PerCpuArray, RingBuf},
    programs::TracePointContext,
};
use borg_watch_ebpf::{ARGS_BYTES, COMM_BYTES, EVENTS_BYTES, EXEC, Exec, PARENTS_KEPT, Parent};

// Where each field these programs read sits in its tracepoint's record. The kernel describes its
// records in tracefs, and Borg reads the offsets there before it loads the programs, so they
// hold on every kernel version.
#[unsafe(no_mangle)]
static EXEC_FILENAME: u32 = 0;
#[unsafe(no_mangle)]
static FORK_CHILD_PID: u32 = 0;
// Where a task keeps its memory, and where that memory says its command line is. They change
// with every kernel build, so Borg reads them from the kernel's own BTF. Zero when the kernel
// has none, and the command line then comes from /proc.
#[unsafe(no_mangle)]
static TASK_MM: u32 = 0;
#[unsafe(no_mangle)]
static MM_ARG_START: u32 = 0;
#[unsafe(no_mangle)]
static MM_ARG_END: u32 = 0;
// The pid namespace Borg runs in, by its device and inode. Zero when Borg runs in the
// machine's own namespace, where it sees every process by the kernel's own numbers.
#[unsafe(no_mangle)]
static PIDNS_DEV: u64 = 0;
#[unsafe(no_mangle)]
static PIDNS_INO: u64 = 0;

/// Borg is MIT or Apache-2.0. The kernel lends its tracing helpers only to programs under a
/// license it counts as compatible with the GPL, and "Dual MIT/GPL" is the one of those that
/// offers MIT.
#[unsafe(no_mangle)]
#[unsafe(link_section = "license")]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";

#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(EVENTS_BYTES, 0);
/// Which process started which, by the kernel's thread numbers, until the child starts a program.
#[map]
static PARENTS: LruHashMap<u32, Parent> = LruHashMap::with_max_entries(PARENTS_KEPT, 0);
/// Records the ring buffer had no room for, counted on each processor.
#[map]
static DROPPED: PerCpuArray<u64> = PerCpuArray::with_max_entries(1, 0);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

fn global<T: Copy>(value: &T) -> T {
    // SAFETY: a plain value the loader may have rewritten before loading. A volatile read keeps
    // the compiler from using the value it was built with.
    unsafe { read_volatile(value) }
}

/// The calling process by Borg's numbers, or nothing when it lives outside the pid namespace
/// Borg watches.
fn current_pid() -> Option<u32> {
    let ino = global(&PIDNS_INO);
    if ino == 0 {
        return Some((bpf_get_current_pid_tgid() >> 32) as u32);
    }
    let mut info = bpf_pidns_info { pid: 0, tgid: 0 };
    // SAFETY: the helper writes at most `size` bytes into `info`, which is that big.
    let found =
        unsafe { bpf_get_ns_current_pid_tgid(global(&PIDNS_DEV), ino, &mut info, size_of::<bpf_pidns_info>() as u32) };
    (found == 0).then_some(info.tgid)
}

/// The calling thread by the kernel's own numbers.
fn current_thread() -> u32 {
    bpf_get_current_pid_tgid() as u32
}

fn boot_ns() -> u64 {
    // SAFETY: the helper takes nothing and returns a number.
    unsafe { bpf_ktime_get_boot_ns() }
}

fn count_dropped() {
    if let Some(count) = DROPPED.get_ptr_mut(0) {
        // SAFETY: this processor's own counter, which nothing else writes while this runs.
        unsafe { *count += 1 };
    }
}

/// A task was created: remember its parent and when, until it starts a program.
#[tracepoint]
pub fn borg_fork(ctx: TracePointContext) -> Option<()> {
    let parent = current_pid()?;
    // SAFETY: the offset is where the tracepoint's own description puts the field.
    let child: u32 = unsafe { ctx.read_at(global(&FORK_CHILD_PID) as usize) }.ok()?;
    let entry = Parent { parent, reserved: 0, forked_ns: boot_ns() };
    PARENTS.insert(&child, &entry, 0).ok()
}

/// A task ended: its parent is not needed any more.
#[tracepoint]
pub fn borg_exit(_ctx: TracePointContext) -> Option<()> {
    PARENTS.remove(&current_thread()).ok()
}

/// Copies the command line of the program the kernel has just started into `args`, from where
/// the kernel put it in the program's own memory a moment before. Says how many bytes it copied
/// and whether there were more, or nothing when it could not read them.
///
/// # Safety
///
/// `args` must point at `ARGS_BYTES` writable bytes.
unsafe fn command_line(args: *mut u8) -> Option<(u32, bool)> {
    let task_mm = global(&TASK_MM);
    if task_mm == 0 {
        return None;
    }
    // SAFETY: the helper takes nothing and returns the current task's address.
    let task = unsafe { bpf_get_current_task() } as *const u8;
    // SAFETY: reads of kernel memory at offsets from the kernel's own BTF, which fail cleanly
    // where nothing is.
    let mm: *const u8 = unsafe { bpf_probe_read_kernel(task.wrapping_add(task_mm as usize).cast()) }.ok()?;
    if mm.is_null() {
        return None;
    }
    // SAFETY: as above.
    let start: u64 = unsafe { bpf_probe_read_kernel(mm.wrapping_add(global(&MM_ARG_START) as usize).cast()) }.ok()?;
    // SAFETY: as above.
    let end: u64 = unsafe { bpf_probe_read_kernel(mm.wrapping_add(global(&MM_ARG_END) as usize).cast()) }.ok()?;
    let whole = end.checked_sub(start)?;
    let length = if whole > ARGS_BYTES as u64 { ARGS_BYTES as u32 } else { whole as u32 };
    // SAFETY: at most `ARGS_BYTES` bytes into `args`, which the caller says is that big, from the
    // new program's own memory. The read fails cleanly where nothing is.
    let read = unsafe { read_user(args.cast::<c_void>(), length, start as *const c_void) };
    (read == 0).then_some((length, whole > ARGS_BYTES as u64))
}

/// The kernel started a program: send the process, its parent, its user, the path it was
/// started by and its command line.
#[tracepoint]
pub fn borg_exec(ctx: TracePointContext) -> Option<()> {
    let pid = current_pid()?;
    // SAFETY: as in `borg_fork`. The field says where in the record the path is: its offset in
    // the low 16 bits.
    let location: u32 = unsafe { ctx.read_at(global(&EXEC_FILENAME) as usize) }.ok()?;
    let kernel_pid = (bpf_get_current_pid_tgid() >> 32) as u32;
    // SAFETY: the value is copied at once, before the map can change under it.
    let parent = unsafe { PARENTS.get(&kernel_pid) }.copied();
    let comm = bpf_get_current_comm().unwrap_or([0; COMM_BYTES]);
    let Some(mut entry) = EVENTS.reserve::<Exec>(0) else {
        count_dropped();
        return None;
    };
    let record: *mut Exec = entry.as_mut_ptr();
    // SAFETY: `record` is a record of the ring buffer reserved for this program until it is
    // submitted, and Borg reads only the bytes written here. The path's location comes from the
    // record the kernel handed this program, and the read fails cleanly outside it.
    unsafe {
        addr_of_mut!((*record).kind).write(EXEC);
        addr_of_mut!((*record).pid).write(pid);
        addr_of_mut!((*record).parent).write(parent.map_or(0, |parent| parent.parent));
        addr_of_mut!((*record).uid).write(bpf_get_current_uid_gid() as u32);
        addr_of_mut!((*record).reserved).write(0);
        addr_of_mut!((*record).forked_ns).write(parent.map_or(0, |parent| parent.forked_ns));
        addr_of_mut!((*record).exec_ns).write(boot_ns());
        addr_of_mut!((*record).comm).write(comm);
        let path = &mut *addr_of_mut!((*record).filename);
        path[0] = 0;
        let at = ctx.as_ptr().cast::<u8>().wrapping_add((location & 0xffff) as usize);
        let _ = bpf_probe_read_kernel_str_bytes(at, path);
        let (length, read, truncated) = match command_line(addr_of_mut!((*record).args).cast()) {
            Some((length, truncated)) => (length, 1, u32::from(truncated)),
            None => (0, 0, 0),
        };
        addr_of_mut!((*record).args_length).write(length);
        addr_of_mut!((*record).args_read).write(read);
        addr_of_mut!((*record).args_truncated).write(truncated);
    }
    entry.submit(0);
    Some(())
}
