//! The programs the kernel runs at every process start. They only read: which process started
//! which, the program the kernel has just started and the file it came from, its command line
//! and who it runs as. Each sends what it read to Borg through one ring buffer and leaves the
//! kernel as it was.

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
            bpf_get_current_task, bpf_get_ns_current_pid_tgid, bpf_ktime_get_boot_ns,
            bpf_probe_read_kernel_str as read_kernel_str, bpf_probe_read_user as read_user,
        },
    },
    macros::{map, tracepoint},
    maps::{LruHashMap, PerCpuArray, RingBuf},
    programs::TracePointContext,
};
use borg_watch_ebpf::{
    ARGS_BYTES, COMM_BYTES, EVENTS_BYTES, EXE_WINDOW, EXEC, Exec, NAME_BYTES, PARENTS_KEPT, PATH_STEPS, Parent,
};

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
// Where the kernel keeps the file a program came from, and how to climb from that file to the
// root: each folder's parent and name, and each mount's root, parent and mount point. From the
// same BTF, and zero when the kernel has none; the program is then the path it was started by.
#[unsafe(no_mangle)]
static MM_EXE_FILE: u32 = 0;
#[unsafe(no_mangle)]
static FILE_MNT: u32 = 0;
#[unsafe(no_mangle)]
static FILE_DENTRY: u32 = 0;
#[unsafe(no_mangle)]
static DENTRY_PARENT: u32 = 0;
#[unsafe(no_mangle)]
static DENTRY_NAME: u32 = 0;
#[unsafe(no_mangle)]
static VFSMOUNT_ROOT: u32 = 0;
#[unsafe(no_mangle)]
static MOUNT_MNT: u32 = 0;
#[unsafe(no_mangle)]
static MOUNT_PARENT: u32 = 0;
#[unsafe(no_mangle)]
static MOUNT_MOUNTPOINT: u32 = 0;
// Where a file keeps its inode, and the inode its count of names on disk.
#[unsafe(no_mangle)]
static FILE_INODE: u32 = 0;
#[unsafe(no_mangle)]
static INODE_NLINK: u32 = 0;
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

/// A pointer-sized value of kernel memory, `offset` bytes past `base`.
fn kernel_word(base: u64, offset: u32) -> Option<u64> {
    let at = base.wrapping_add(u64::from(offset)) as *const u64;
    // SAFETY: a read of kernel memory at an offset from the kernel's own BTF, which fails
    // cleanly where nothing is.
    unsafe { bpf_probe_read_kernel(at) }.ok()
}

/// The memory of the current task, which is the new program's once the kernel has started it.
fn current_mm() -> Option<u64> {
    let task_mm = global(&TASK_MM);
    if task_mm == 0 {
        return None;
    }
    // SAFETY: the helper takes nothing and returns the current task's address.
    let task = unsafe { bpf_get_current_task() };
    kernel_word(task, task_mm).filter(|mm| *mm != 0)
}

/// Copies the command line of the program the kernel has just started into `args`, from where
/// the kernel put it in the program's own memory a moment before. Says how many bytes it copied
/// and whether there were more, or nothing when it could not read them.
///
/// # Safety
///
/// `args` must point at `ARGS_BYTES` writable bytes.
unsafe fn command_line(mm: u64, args: *mut u8) -> Option<(u32, bool)> {
    let start = kernel_word(mm, global(&MM_ARG_START))?;
    let end = kernel_word(mm, global(&MM_ARG_END))?;
    let whole = end.checked_sub(start)?;
    let length = if whole > ARGS_BYTES as u64 { ARGS_BYTES as u32 } else { whole as u32 };
    // SAFETY: at most `ARGS_BYTES` bytes into `args`, which the caller says is that big, from the
    // new program's own memory. The read fails cleanly where nothing is.
    let read = unsafe { read_user(args.cast::<c_void>(), length, start as *const c_void) };
    (read == 0).then_some((length, whole > ARGS_BYTES as u64))
}

/// Writes the name of a folder entry into `exe`, `at` bytes in, and says how many bytes it took
/// with its closing zero.
///
/// # Safety
///
/// `exe` must point at `EXE_BYTES` writable bytes.
unsafe fn name(dentry: u64, exe: *mut u8, at: u32) -> Option<u32> {
    let name = kernel_word(dentry, global(&DENTRY_NAME))?;
    // The compiler would drop the mask where it knows `at` is inside the window already, and the
    // verifier needs to see it.
    let into = exe.wrapping_add(core::hint::black_box(at) as usize & (EXE_WINDOW - 1));
    // SAFETY: at most `NAME_BYTES` bytes, `at` masked to the window, so inside the `EXE_BYTES`
    // the caller gave. The name is the kernel's own, and the read fails cleanly where nothing is.
    let read = unsafe { read_kernel_str(into.cast::<c_void>(), NAME_BYTES as u32, name as *const c_void) };
    (read > 0).then_some(read as u32)
}

/// Whether the file the new program came from has no name left on disk: a program run from
/// memory, or from a file deleted before it started.
fn unlinked(mm: u64) -> Option<bool> {
    let file_inode = global(&FILE_INODE);
    if file_inode == 0 {
        return None;
    }
    let file = kernel_word(mm, global(&MM_EXE_FILE)).filter(|file| *file != 0)?;
    let inode = kernel_word(file, file_inode).filter(|inode| *inode != 0)?;
    let at = inode.wrapping_add(u64::from(global(&INODE_NLINK))) as *const u32;
    // SAFETY: as in `kernel_word`.
    let names: u32 = unsafe { bpf_probe_read_kernel(at) }.ok()?;
    Some(names == 0)
}

/// Writes the path of the file the new program came from into `exe`, as the kernel itself would
/// name it: each name from the file up to the root of its mount namespace, climbing from a
/// mount's root to where it is mounted. Keeps the bytes written in `length`, and leaves nothing
/// in it when it could not read the whole path.
///
/// The count lives in the record and not in a register: the verifier knows nothing of what a
/// record holds, so every way through a step looks the same to it and it checks each step once.
///
/// # Safety
///
/// `exe` must point at `EXE_BYTES` writable bytes, and `length` at a writable `u32`.
unsafe fn executable(mm: u64, exe: *mut u8, length: *mut u32) -> Option<()> {
    let exe_file = global(&MM_EXE_FILE);
    if exe_file == 0 {
        return None;
    }
    let file = kernel_word(mm, exe_file).filter(|file| *file != 0)?;
    let mut dentry = kernel_word(file, global(&FILE_DENTRY))?;
    let mut mount = kernel_word(file, global(&FILE_MNT))?.wrapping_sub(u64::from(global(&MOUNT_MNT)));
    // SAFETY: as the caller says.
    unsafe { length.write_volatile(0) };
    for _ in 0..PATH_STEPS {
        let root = kernel_word(mount.wrapping_add(u64::from(global(&MOUNT_MNT))), global(&VFSMOUNT_ROOT))?;
        let parent = kernel_word(dentry, global(&DENTRY_PARENT))?;
        if dentry == root || dentry == parent {
            let above = kernel_word(mount, global(&MOUNT_PARENT))?;
            if above == mount {
                // The root of the namespace. A file that has no folder at all, as a program run
                // from memory, is named by its own entry.
                // SAFETY: as the caller says.
                if unsafe { length.read_volatile() } == 0 {
                    let read = unsafe { name(dentry, exe, 0) }?;
                    unsafe { length.write_volatile(read) };
                }
                return Some(());
            }
            dentry = kernel_word(mount, global(&MOUNT_MOUNTPOINT))?;
            mount = above;
            continue;
        }
        // SAFETY: as the caller says.
        let written = unsafe { length.read_volatile() };
        if written as usize >= EXE_WINDOW {
            return None;
        }
        // SAFETY: as the caller says.
        let read = unsafe { name(dentry, exe, written) }?;
        unsafe { length.write_volatile(written + read) };
        dentry = parent;
    }
    None
}

/// The kernel started a program: send the process, its parent, its user, the path it was
/// started by, the file it came from and its command line.
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
        addr_of_mut!((*record).forked_ns).write(parent.map_or(0, |parent| parent.forked_ns));
        addr_of_mut!((*record).exec_ns).write(boot_ns());
        addr_of_mut!((*record).comm).write(comm);
        let path = &mut *addr_of_mut!((*record).filename);
        path[0] = 0;
        let at = ctx.as_ptr().cast::<u8>().wrapping_add((location & 0xffff) as usize);
        let _ = bpf_probe_read_kernel_str_bytes(at, path);
        let mm = current_mm();
        let args = mm.and_then(|mm| command_line(mm, addr_of_mut!((*record).args).cast()));
        let (length, read, truncated) = match args {
            Some((length, truncated)) => (length, 1, u32::from(truncated)),
            None => (0, 0, 0),
        };
        addr_of_mut!((*record).args_length).write(length);
        addr_of_mut!((*record).args_read).write(read);
        addr_of_mut!((*record).args_truncated).write(truncated);
        let length = addr_of_mut!((*record).exe_length);
        if mm.and_then(|mm| executable(mm, addr_of_mut!((*record).exe).cast(), length)).is_none() {
            length.write(0);
        }
        let gone = mm.and_then(unlinked).unwrap_or(false);
        addr_of_mut!((*record).exe_unlinked).write(u32::from(gone));
        addr_of_mut!((*record).reserved).write(0);
    }
    entry.submit(0);
    Some(())
}
