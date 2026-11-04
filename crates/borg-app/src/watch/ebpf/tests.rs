use super::*;
use crate::watch::Executed;
use std::os::unix::fs::symlink;

/// A hundred seconds after boot, in the kernel's nanoseconds and in `/proc`'s ticks.
const EXEC_NS: u64 = 100_000_000_000;
const EXEC_TICK: u64 = 10_000;

fn put_u32(record: &mut [u8], at: usize, value: u32) {
    record[at..at + 4].copy_from_slice(&value.to_ne_bytes());
}

fn put_u64(record: &mut [u8], at: usize, value: u64) {
    record[at..at + 8].copy_from_slice(&value.to_ne_bytes());
}

fn put_text(record: &mut [u8], at: usize, text: &str) {
    record[at..at + text.len()].copy_from_slice(text.as_bytes());
}

/// A program start as the programs inside the kernel send it, with the command line when they
/// read one.
fn exec_record(pid: u32, parent: u32, name: &str, program: &str, command: Option<&[&str]>) -> Vec<u8> {
    let mut record = vec![0xAA; size_of::<Exec>()];
    put_u32(&mut record, offset_of!(Exec, kind), EXEC);
    put_u32(&mut record, offset_of!(Exec, pid), pid);
    put_u32(&mut record, offset_of!(Exec, parent), parent);
    put_u32(&mut record, offset_of!(Exec, uid), 1000);
    put_u32(&mut record, offset_of!(Exec, args_length), 0);
    put_u32(&mut record, offset_of!(Exec, args_read), 0);
    put_u32(&mut record, offset_of!(Exec, args_truncated), 0);
    put_u32(&mut record, offset_of!(Exec, exe_length), 0);
    put_u32(&mut record, offset_of!(Exec, exe_unlinked), 0);
    put_u64(&mut record, offset_of!(Exec, forked_ns), EXEC_NS - 2_000_000);
    put_u64(&mut record, offset_of!(Exec, exec_ns), EXEC_NS);
    for field in
        [offset_of!(Exec, comm)..offset_of!(Exec, filename), offset_of!(Exec, filename)..offset_of!(Exec, args)]
    {
        record[field].fill(0);
    }
    put_text(&mut record, offset_of!(Exec, comm), name);
    put_text(&mut record, offset_of!(Exec, filename), program);
    if let Some(command) = command {
        let bytes: Vec<u8> = command.iter().flat_map(|argument| argument.bytes().chain([0])).collect();
        put_u32(&mut record, offset_of!(Exec, args_length), bytes.len() as u32);
        put_u32(&mut record, offset_of!(Exec, args_read), 1);
        record[offset_of!(Exec, args)..offset_of!(Exec, args) + bytes.len()].copy_from_slice(&bytes);
    }
    record
}

/// Gives a record the file the kernel ran, as the programs write it: each name from the file up.
fn with_exe(mut record: Vec<u8>, names: &[&str]) -> Vec<u8> {
    let bytes: Vec<u8> = names.iter().flat_map(|name| name.bytes().chain([0])).collect();
    put_u32(&mut record, offset_of!(Exec, exe_length), bytes.len() as u32);
    record[offset_of!(Exec, exe)..offset_of!(Exec, exe) + bytes.len()].copy_from_slice(&bytes);
    record
}

fn seen(name: &str, program: &str, command: Option<&[&str]>) -> Seen {
    Seen {
        parent: 0,
        uid: 1000,
        forked_ns: EXEC_NS - 2_000_000,
        exec_ns: EXEC_NS,
        name: name.into(),
        program: program.into(),
        executable: None,
        unlinked: false,
        command: command.map(|command| command.iter().map(|argument| (*argument).to_owned()).collect()),
        truncated: false,
    }
}

#[test]
fn records_read_as_the_kernel_side_lays_them_out() {
    let command = ["curl", "-s", "", "https://example.com"];
    let (pid, seen) = parse(&exec_record(300, 200, "curl", "/usr/bin/curl", Some(&command))).expect("a start");
    assert_eq!((pid, seen.parent, seen.uid), (300, 200, 1000));
    assert_eq!((seen.forked_ns, seen.exec_ns), (EXEC_NS - 2_000_000, EXEC_NS));
    assert_eq!(seen.name, "curl");
    assert_eq!(seen.program, Path::new("/usr/bin/curl"));
    assert_eq!(
        seen.command,
        Some(vec!["curl".into(), "-s".into(), "https://example.com".into()]),
        "the bytes after the command line are left alone, and empty arguments read as /proc gives them"
    );
    assert!(!seen.truncated);

    assert_eq!(seen.executable, None, "the programs did not name the file");

    let (_, unread) = parse(&exec_record(300, 200, "curl", "/usr/bin/curl", None)).expect("a start");
    assert_eq!(unread.command, None, "a command line the programs could not read comes from /proc");

    let named = with_exe(exec_record(300, 200, "quick", "./quick", None), &["quick", "tmp"]);
    let (_, named) = parse(&named).expect("a start");
    assert_eq!(named.program, Path::new("./quick"), "the path it was started by");
    assert_eq!(named.executable.as_deref(), Some(Path::new("/tmp/quick")), "and the file the kernel ran");
    let mut memory = with_exe(exec_record(300, 200, "3", "/dev/fd/3", None), &["memfd:payload"]);
    put_u32(&mut memory, offset_of!(Exec, exe_unlinked), 1);
    let (_, memory) = parse(&memory).expect("a start");
    assert_eq!(memory.executable.as_deref(), Some(Path::new("/memfd:payload")), "a file with no folder");
    assert!(memory.unlinked, "and no name on disk");
    assert!(!named.unlinked);

    let mut cut = exec_record(300, 200, "curl", "/usr/bin/curl", Some(&["curl", "-s"]));
    put_u32(&mut cut, offset_of!(Exec, args_truncated), 1);
    let (_, cut) = parse(&cut).expect("a start");
    assert!(cut.truncated);
}

#[test]
fn a_short_record_or_one_of_an_unknown_kind_is_nothing() {
    let whole = exec_record(300, 1, "sh", "/bin/sh", Some(&["sh"]));
    assert_eq!(parse(&whole[..whole.len() - 1]), None);
    assert_eq!(parse(&whole[..2]), None);
    assert_eq!(parse(&[]), None);
    let mut unknown = whole.clone();
    put_u32(&mut unknown, 0, 99);
    assert_eq!(parse(&unknown), None);

    let mut lying = with_exe(whole, &["sh", "bin"]);
    put_u32(&mut lying, offset_of!(Exec, args_length), u32::MAX);
    put_u32(&mut lying, offset_of!(Exec, exe_length), u32::MAX);
    let (_, seen) = parse(&lying).expect("a length past the record is cut to it");
    assert_eq!(seen.command.as_deref().and_then(<[String]>::first).map(String::as_str), Some("sh"));
    let executable = seen.executable.expect("a path");
    assert!(executable.ends_with("bin/sh"), "the bytes past what was written are cut to the field: {executable:?}");
}

const SCHED_PROCESS_EXEC: &str = "name: sched_process_exec\nID: 312\nformat:\n\
\tfield:unsigned short common_type;\toffset:0;\tsize:2;\tsigned:0;\n\
\tfield:unsigned char common_flags;\toffset:2;\tsize:1;\tsigned:0;\n\
\tfield:unsigned char common_preempt_count;\toffset:3;\tsize:1;\tsigned:0;\n\
\tfield:int common_pid;\toffset:4;\tsize:4;\tsigned:1;\n\n\
\tfield:__data_loc char[] filename;\toffset:8;\tsize:4;\tsigned:0;\n\
\tfield:pid_t pid;\toffset:12;\tsize:4;\tsigned:1;\n\
\tfield:pid_t old_pid;\toffset:16;\tsize:4;\tsigned:1;\n\n\
print fmt: \"filename=%s pid=%d old_pid=%d\", __get_str(filename), REC->pid, REC->old_pid\n";

/// `sched_process_fork` as kernels before 6.11 describe it, with the names in fixed fields.
const SCHED_PROCESS_FORK: &str = "name: sched_process_fork\nformat:\n\
\tfield:int common_pid;\toffset:4;\tsize:4;\tsigned:1;\n\n\
\tfield:char parent_comm[16];\toffset:8;\tsize:16;\tsigned:0;\n\
\tfield:pid_t parent_pid;\toffset:24;\tsize:4;\tsigned:1;\n\
\tfield:char child_comm[16];\toffset:28;\tsize:16;\tsigned:0;\n\
\tfield:pid_t child_pid;\toffset:44;\tsize:4;\tsigned:1;\n";

/// `sched_process_fork` as later kernels describe it, with the names stored after the record.
const SCHED_PROCESS_FORK_LATER: &str = "name: sched_process_fork\nformat:\n\
\tfield:__data_loc char[] parent_comm;\toffset:8;\tsize:4;\tsigned:0;\n\
\tfield:pid_t parent_pid;\toffset:12;\tsize:4;\tsigned:1;\n\
\tfield:__data_loc char[] child_comm;\toffset:16;\tsize:4;\tsigned:0;\n\
\tfield:pid_t child_pid;\toffset:20;\tsize:4;\tsigned:1;\n";

#[test]
fn a_tracepoint_format_gives_each_field_its_offset_and_size() {
    let exec = fields(SCHED_PROCESS_EXEC);
    assert_eq!(exec.get("filename"), Some(&(8, 4)));
    assert_eq!(exec.get("old_pid"), Some(&(16, 4)));
    assert_eq!(exec.get("common_pid"), Some(&(4, 4)));
    assert_eq!(fields(SCHED_PROCESS_FORK).get("child_pid"), Some(&(44, 4)));
    assert_eq!(
        fields(SCHED_PROCESS_FORK).get("child_comm"),
        Some(&(28, 16)),
        "an array's name goes without its length"
    );
    assert_eq!(fields(SCHED_PROCESS_FORK_LATER).get("child_pid"), Some(&(20, 4)));
    let pointer = "\tfield:const char *const * argv;\toffset:24;\tsize:8;\tsigned:0;\n";
    assert_eq!(fields(pointer).get("argv"), Some(&(24, 8)), "a pointer's name goes without its stars");
    assert!(fields("print fmt: \"field:x\"\n").is_empty());
}

fn tracefs_with(events: &[(&str, &str)]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    for (event, format) in events {
        let at = folder.path().join("events").join(event);
        fs::create_dir_all(&at).unwrap();
        fs::write(at.join("format"), format).unwrap();
    }
    folder
}

#[test]
fn the_offsets_come_from_the_kernel() {
    let later = tracefs_with(&[
        ("sched/sched_process_exec", SCHED_PROCESS_EXEC),
        ("sched/sched_process_fork", SCHED_PROCESS_FORK_LATER),
    ]);
    assert_eq!(Offsets::read(later.path()).unwrap(), Offsets { exec_filename: 8, fork_child_pid: 20 });
    let earlier = tracefs_with(&[
        ("sched/sched_process_exec", SCHED_PROCESS_EXEC),
        ("sched/sched_process_fork", SCHED_PROCESS_FORK),
    ]);
    assert_eq!(Offsets::read(earlier.path()).unwrap().fork_child_pid, 44);

    let no_fork = tracefs_with(&[("sched/sched_process_exec", SCHED_PROCESS_EXEC)]);
    assert!(Offsets::read(no_fork.path()).is_err(), "the programs cannot do without a parent");

    let wider = SCHED_PROCESS_EXEC.replace("filename;\toffset:8;\tsize:4", "filename;\toffset:8;\tsize:8");
    let wrong = tracefs_with(&[("sched/sched_process_exec", &wider), ("sched/sched_process_fork", SCHED_PROCESS_FORK)]);
    let error = Offsets::read(wrong.path()).unwrap_err().to_string();
    assert!(error.contains("filename of 8 bytes"), "a field of another size is refused: {error}");
}

#[test]
fn the_machines_own_pid_namespace_reports_every_process_by_the_kernels_numbers() {
    assert_eq!(Namespace::from_stat(0, 4, INIT_PID_NAMESPACE), Namespace::default());
    assert_eq!(Namespace::from_stat(0, 4, 4_026_532_281), Namespace { dev: 4, ino: 4_026_532_281 });
    assert_eq!(Namespace::from_stat(1, 300, 7), Namespace { dev: (1 << 20) | 300, ino: 7 });
}

#[test]
fn process_starts_count_in_the_ticks_proc_counts_them_in() {
    assert_eq!(ticks(EXEC_NS), EXEC_TICK);
    assert_eq!(ticks(9_999_999), 0);
    assert_eq!(ticks(0), 0);
}

/// A folder that looks like `/proc`, with the files the watcher reads.
struct FakeProc {
    folder: tempfile::TempDir,
}

impl FakeProc {
    fn new() -> Self {
        Self { folder: tempfile::tempdir().unwrap() }
    }

    fn processes(&self) -> Processes {
        Processes::at(self.folder.path().join("proc"))
    }

    /// A file outside the process folders, for a process to run.
    fn file(&self, path: &str) -> PathBuf {
        let path = self.folder.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "#!").unwrap();
        path
    }

    fn process(&self, pid: u32, name: &str, parent: u32, started_at: u64, program: &Path, command: &[&str]) {
        let at = self.folder.path().join("proc").join(pid.to_string());
        fs::create_dir_all(at.join("fd")).unwrap();
        let mut fields = vec!["S".to_owned(), parent.to_string(), "100".into(), "100".into(), "0".into()];
        fields.resize(19, "0".into());
        fields.push(started_at.to_string());
        fs::write(at.join("stat"), format!("{pid} ({name}) {}\n", fields.join(" "))).unwrap();
        fs::write(at.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").unwrap();
        fs::write(at.join("cmdline"), command.join("\0")).unwrap();
        fs::write(at.join("environ"), "").unwrap();
        symlink(program, at.join("exe")).unwrap();
    }

    /// Gives a process a root folder of its own.
    fn root(&self, pid: u32, root: &Path) {
        symlink(root, self.folder.path().join("proc").join(pid.to_string()).join("root")).unwrap();
    }
}

#[test]
fn a_command_that_ended_before_borg_could_read_it_is_still_reported() {
    let proc = FakeProc::new();
    proc.process(1, "systemd", 0, 1, &proc.file("sbin/init"), &["/sbin/init"]);
    proc.process(200, "bash", 1, 100, &proc.file("bin/bash"), &["-bash"]);
    let mut seen = seen("curl", "/usr/bin/curl", Some(&["curl", "-s", "https://example.com/x.sh"]));
    seen.parent = 200;
    let at = SystemTime::UNIX_EPOCH;

    let started = Executed { pid: 300, seen: Some(Box::new(seen.clone())) }.started(&proc.processes(), at);
    let started = started.expect("what the programs read is enough");
    assert_eq!(started.key, ProcessKey { pid: 300, start: ticks(seen.forked_ns) });
    assert_eq!(started.name, "curl");
    assert_eq!(started.program.as_deref(), Some(Path::new("/usr/bin/curl")));
    assert_eq!(started.command, ["curl", "-s", "https://example.com/x.sh"]);
    assert_eq!(started.uid, 1000);
    let parents: Vec<&str> = started.ancestors.iter().map(|ancestor| ancestor.name.as_str()).collect();
    assert_eq!(parents, ["bash", "systemd"], "the processes above it are still there");

    assert!(
        Executed::number(300).started(&proc.processes(), at).is_none(),
        "without the programs, a command that has ended is gone"
    );
}

#[test]
fn a_command_that_ended_is_named_by_the_file_the_kernel_ran() {
    let proc = FakeProc::new();
    let mut seen = seen("quick", "./quick", Some(&["./quick", "--now"]));
    seen.executable = Some(PathBuf::from("/tmp/quick"));
    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.program.as_deref(), Some(Path::new("/tmp/quick")), "where the relative path led");

    assert!(!started.program_deleted);

    seen.unlinked = true;
    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert!(started.program_deleted, "a file that was gone as it started is still a deleted program");

    seen.executable = None;
    seen.unlinked = false;
    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.program.as_deref(), Some(Path::new("./quick")), "the path it was started by, without it");
}

#[test]
fn a_process_that_renamed_itself_after_a_relative_start_keeps_its_program() {
    let proc = FakeProc::new();
    let program = proc.file("opt/agent/agent");
    proc.process(300, "agent: worker", 1, EXEC_TICK, &program, &["agent: worker"]);
    let mut seen = seen("agent", "./agent", Some(&["./agent", "--worker"]));
    seen.executable = Some(program.clone());

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.program.as_deref(), Some(program.as_path()), "the same file, so the same program");
    assert_eq!(started.name, "agent: worker");
    assert_eq!(started.command, ["./agent", "--worker"]);
}

#[test]
fn a_process_that_runs_another_file_than_the_kernel_ran_has_moved_on() {
    let proc = FakeProc::new();
    proc.process(300, "env", 1, EXEC_TICK, &proc.file("bin/sleep"), &["sleep", "60"]);
    let mut seen = seen("env", "/usr/bin/env", Some(&["env", "sleep", "60"]));
    seen.executable = Some(PathBuf::from("/usr/bin/env"));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.program.as_deref(), Some(Path::new("/usr/bin/env")), "even under the same name");
    assert_eq!(started.command, ["env", "sleep", "60"]);
}

#[test]
fn the_command_line_as_it_was_passed_wins_over_one_the_process_rewrote() {
    let proc = FakeProc::new();
    let program = proc.file("usr/bin/python3");
    proc.process(300, "python3", 1, EXEC_TICK, &program, &["[kworker/0:1]"]);
    let seen = seen("python3", "/usr/bin/python3", Some(&["python3", "-c", "import pty"]));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.command, ["python3", "-c", "import pty"]);
    assert_eq!(started.program.as_deref(), Some(program.as_path()), "the program /proc resolved stays");
    assert_eq!(started.key, ProcessKey { pid: 300, start: EXEC_TICK }, "the start /proc counted stays");

    let mut cut = seen.clone();
    cut.truncated = true;
    let started = cut.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.command, ["[kworker/0:1]"], "a command line the programs cut short leaves /proc's");

    let mut unread = seen;
    unread.command = None;
    assert_eq!(unread.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH).command, ["[kworker/0:1]"]);
}

#[test]
fn a_process_that_started_another_program_since_is_reported_as_it_was() {
    let proc = FakeProc::new();
    proc.process(300, "sleep", 1, EXEC_TICK, &proc.file("bin/sleep"), &["sleep", "60"]);
    let seen = seen("env", "/usr/bin/env", Some(&["env", "sleep", "60"]));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.name, "env");
    assert_eq!(started.program.as_deref(), Some(Path::new("/usr/bin/env")));
    assert_eq!(started.command, ["env", "sleep", "60"]);
    assert_eq!(started.key.start, EXEC_TICK, "it is still the same process");
}

#[test]
fn a_process_that_runs_a_deleted_file_and_renamed_itself_still_runs_a_deleted_file() {
    let proc = FakeProc::new();
    proc.process(300, "kworker", 1, EXEC_TICK, Path::new("/tmp/.x/payload (deleted)"), &["kworker"]);
    let seen = seen("payload", "/tmp/.x/payload", Some(&["/tmp/.x/payload", "--beacon"]));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert!(started.program_deleted, "the file it runs is gone, whatever it calls itself");
    assert_eq!(started.program.as_deref(), Some(Path::new("/tmp/.x/payload")));
    assert_eq!(started.name, "kworker");
    assert_eq!(started.command, ["/tmp/.x/payload", "--beacon"], "the command line it was started with");
}

#[test]
fn a_process_that_renamed_itself_keeps_the_program_proc_resolved() {
    let proc = FakeProc::new();
    let program = proc.file("rootfs/opt/agent/agent");
    proc.process(300, "agent: worker", 1, EXEC_TICK, &program, &["agent: worker"]);
    proc.root(300, &proc.folder.path().join("rootfs"));
    let seen = seen("agent", "/opt/agent/agent", Some(&["/opt/agent/agent", "--worker"]));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.name, "agent: worker");
    assert_eq!(started.program.as_deref(), Some(program.as_path()));
    assert_eq!(started.command, ["/opt/agent/agent", "--worker"]);
}

#[test]
fn a_later_process_with_the_same_number_is_not_taken_for_the_one_that_started() {
    let proc = FakeProc::new();
    proc.process(300, "nginx", 1, EXEC_TICK + 200, &proc.file("usr/sbin/nginx"), &["nginx"]);
    let seen = seen("nginx", "/usr/sbin/nginx", Some(&["nginx", "-g", "daemon off;"]));

    let started = seen.started(&proc.processes(), 300, SystemTime::UNIX_EPOCH);
    assert_eq!(started.key, ProcessKey { pid: 300, start: ticks(seen.forked_ns) });
    assert_eq!(started.command, ["nginx", "-g", "daemon off;"]);
    assert_eq!(started.program.as_deref(), Some(Path::new("/usr/sbin/nginx")), "nothing of the later one is taken");
}

/// Loads the programs into the running kernel and hears a command that ends at once. Loading
/// needs root, so CI runs this test on its own with `sudo`.
#[cfg(borg_ebpf)]
#[tokio::test]
#[ignore = "loads programs into the kernel, which needs root"]
async fn the_programs_load_and_report_a_command_that_ends_at_once() {
    let mut events = listen().expect("the programs load");
    let marker = format!("borg-ebpf-check-{}", std::process::id());
    let mut child = std::process::Command::new("/bin/sh").args(["-c", "exit 0", &marker]).spawn().unwrap();
    let pid = child.id();
    assert!(child.wait().unwrap().success());
    let layout = fs::read(KERNEL_BTF).ok().and_then(|btf| btf::Layout::read(&btf));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let executed = loop {
        let executed = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("the start is reported in time")
            .expect("the programs keep sending");
        if executed.pid == pid {
            break executed;
        }
    };
    let seen = executed.seen.as_deref().expect("the programs read it");
    assert_eq!(seen.program, Path::new("/bin/sh"));
    assert_eq!(seen.parent, std::process::id(), "the test started it");
    assert!(seen.forked_ns > 0 && seen.forked_ns <= seen.exec_ns);
    if let Some(layout) = layout {
        let command = ["/bin/sh", "-c", "exit 0", &marker].map(str::to_owned);
        assert_eq!(seen.command.as_deref(), Some(&command[..]));
        if layout.exe.is_some() {
            let shell = fs::canonicalize("/bin/sh").unwrap();
            assert_eq!(seen.executable.as_deref(), Some(shell.as_path()), "the file /bin/sh leads to");
        }
    }
    let started = executed.started(&Processes::default(), SystemTime::now()).expect("it is reported");
    assert_eq!(started.key.pid, pid);
    let program = seen.executable.as_deref().unwrap_or(Path::new("/bin/sh"));
    assert_eq!(started.program.as_deref(), Some(program), "the command has already ended");
    assert_eq!(started.ancestors.first().map(|parent| parent.pid), Some(std::process::id()));

    if layout.is_some_and(|layout| layout.exe.is_some()) {
        // A program run from memory: no folder, and no name on disk.
        use std::os::fd::AsRawFd;
        let memory = rustix::fs::memfd_create("borg-ebpf-check", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        let mut writer = fs::File::from(memory);
        std::io::Write::write_all(&mut writer, &fs::read("/bin/true").unwrap()).unwrap();
        let reader = fs::File::open(format!("/proc/self/fd/{}", writer.as_raw_fd())).unwrap();
        drop(writer);
        let mut child =
            std::process::Command::new(format!("/proc/self/fd/{}", reader.as_raw_fd())).arg(&marker).spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        let executed = loop {
            let executed = tokio::time::timeout_at(deadline, events.recv())
                .await
                .expect("the start from memory is reported in time")
                .expect("the programs keep sending");
            if executed.pid == pid {
                break executed;
            }
        };
        let seen = executed.seen.as_deref().expect("the programs read it");
        let executable = seen.executable.as_deref().expect("the programs name the file");
        assert!(executable.starts_with("/memfd:borg-ebpf-check"), "{executable:?}");
        assert!(seen.unlinked, "a file in memory has no name on disk");
    }
}
