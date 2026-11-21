//! What the watcher's eBPF programs report, laid out the same way in the kernel and in Borg.
//!
//! The programs send one [`Exec`] record through a ring buffer each time the kernel has started
//! a program: the process, its parent, its user, the path it was started by, the file the kernel
//! ran, and its command line as the kernel has just copied it into the new program's memory.
//!
//! Every record starts with its kind as a `u32`, so records of other kinds can join later. The
//! offsets of the fields Borg reads come from `offset_of!` on these structs, so a reader cannot
//! drift from what the kernel writes.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// The kind of an [`Exec`] record.
pub const EXEC: u32 = 1;
/// Bytes of the name a process calls itself, as the kernel keeps it.
pub const COMM_BYTES: usize = 16;
/// Bytes of the path a program was started by, with its closing zero.
pub const FILENAME_BYTES: usize = 256;
/// Bytes of command line a record carries before it says the rest was cut.
pub const ARGS_BYTES: usize = 4096;
/// Bytes of one name in a path, with its closing zero, as the kernel limits it.
pub const NAME_BYTES: usize = 256;
/// Bytes of the program's path the kernel side writes before it gives up on a deeper one. A
/// power of two, so the kernel's verifier can see every write stays inside the record.
pub const EXE_WINDOW: usize = 1024;
/// Bytes of the field the program's path goes in: the window, and room for the last name.
pub const EXE_BYTES: usize = EXE_WINDOW + NAME_BYTES;
/// Folders and mount points the kernel side climbs from the program's file to the root.
pub const PATH_STEPS: usize = 48;
/// Bytes of the ring buffer between the kernel and Borg.
pub const EVENTS_BYTES: u32 = 4 * 1024 * 1024;
/// Processes whose parents the kernel side remembers until they start a program.
pub const PARENTS_KEPT: u32 = 32 * 1024;

/// A program was started: sent from the kernel's `sched_process_exec` tracepoint.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Exec {
    pub kind: u32,
    /// The process, as Borg sees process numbers.
    pub pid: u32,
    /// The process that started it, as Borg sees process numbers; zero when the kernel side did
    /// not see it start.
    pub parent: u32,
    pub uid: u32,
    /// Bytes of `args` that hold the command line.
    pub args_length: u32,
    /// One when the kernel side read the command line.
    pub args_read: u32,
    /// One when the command line had more than `args` holds.
    pub args_truncated: u32,
    /// Bytes of `exe` that hold the program's path; zero when the kernel side could not read it.
    pub exe_length: u32,
    /// One when the program's file had no name left on disk as it started, as a program run from
    /// memory or from a file deleted first.
    pub exe_unlinked: u32,
    pub reserved: u32,
    /// When the process started, in nanoseconds since the machine booted; zero when unknown.
    pub forked_ns: u64,
    /// When it started the program, in nanoseconds since the machine booted.
    pub exec_ns: u64,
    pub comm: [u8; COMM_BYTES],
    /// The path the program was started by, closed with a zero.
    pub filename: [u8; FILENAME_BYTES],
    /// The arguments, each closed with a zero, as `/proc/<pid>/cmdline` gives them.
    pub args: [u8; ARGS_BYTES],
    /// The file the kernel ran, as the names from the file up to the root of its mount
    /// namespace, each closed with a zero: `true`, `bin`, `usr` for `/usr/bin/true`.
    pub exe: [u8; EXE_BYTES],
}

/// What the kernel side remembers about a process until it starts a program.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Parent {
    /// The process that started it, as Borg sees process numbers.
    pub parent: u32,
    pub reserved: u32,
    pub forked_ns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn records_have_the_same_shape_on_every_machine_and_in_the_kernel() {
        assert_eq!(offset_of!(Exec, forked_ns), 40);
        assert_eq!(offset_of!(Exec, comm), 56);
        assert_eq!(offset_of!(Exec, args), 56 + COMM_BYTES + FILENAME_BYTES);
        assert_eq!(offset_of!(Exec, exe), 56 + COMM_BYTES + FILENAME_BYTES + ARGS_BYTES);
        assert_eq!(size_of::<Exec>(), 56 + COMM_BYTES + FILENAME_BYTES + ARGS_BYTES + EXE_BYTES);
        assert!(EXE_WINDOW.is_power_of_two());
        assert_eq!(size_of::<Exec>() % 8, 0, "the ring buffer keeps records 8-byte aligned");
        assert_eq!(size_of::<Parent>(), 16);
        assert!(EVENTS_BYTES.is_power_of_two(), "the kernel takes ring buffers of a power of two");
    }
}
