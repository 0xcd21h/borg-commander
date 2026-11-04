use super::*;
use borg_core::StreamKind;
use std::time::Duration;

/// The device number of `/dev/pts/0`.
const TERMINAL_DEVICE: u32 = 34816;

/// A `stat` line as Linux writes it: the number, the name in brackets, the state, then the rest.
fn stat_line(name: &str, parent: u32, terminal: u32, started_at: u64) -> String {
    session_stat_line(name, parent, 100, terminal, started_at)
}

/// A `stat` line of a process in the session that `session` started.
fn session_stat_line(name: &str, parent: u32, session: u32, terminal: u32, started_at: u64) -> String {
    let mut fields =
        vec!["S".to_owned(), parent.to_string(), session.to_string(), session.to_string(), terminal.to_string()];
    while fields.len() < STARTED_AT {
        fields.push("0".to_owned());
    }
    fields.push(started_at.to_string());
    format!("42 ({name}) {}\n", fields.join(" "))
}

#[test]
fn a_program_name_with_spaces_and_brackets_does_not_move_the_fields() {
    let stat = parse_stat(&stat_line("Web Content (tab)", 1234, 0, 998877)).expect("a whole stat line");
    assert_eq!(stat.name, "Web Content (tab)");
    assert_eq!((stat.parent, stat.session, stat.terminal, stat.started_at), (1234, 100, 0, 998877));
}

#[test]
fn a_stat_line_that_stops_early_says_nothing() {
    assert_eq!(parse_stat("42 (sh) S 1"), None);
    assert_eq!(parse_stat("nothing like a stat line"), None);
    assert_eq!(parse_stat(""), None);
}

#[test]
fn the_user_comes_from_the_first_number_on_the_uid_line() {
    let status = "Name:\tsh\nState:\tS (sleeping)\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\n";
    assert_eq!(parse_uid(status), Some(1000));
    assert_eq!(parse_uid("Name:\tsh\n"), None);
}

#[test]
fn only_preload_variables_are_kept_out_of_an_environment() {
    let environ = b"PATH=/usr/bin\0LD_PRELOAD=/tmp/x.so\0HOME=/root\0LD_AUDIT=\0DYLD_INSERT_LIBRARIES=/tmp/y.dylib\0";
    assert_eq!(
        preloads(environ),
        [
            ("LD_PRELOAD".to_owned(), "/tmp/x.so".to_owned()),
            ("DYLD_INSERT_LIBRARIES".to_owned(), "/tmp/y.dylib".to_owned())
        ],
        "a variable with nothing in it puts no library anywhere"
    );
    assert!(preloads(b"").is_empty());
}

#[test]
fn a_streams_link_says_what_it_leads_to() {
    assert_eq!(stream_kind("socket:[12345]"), StreamKind::Socket);
    assert_eq!(stream_kind("pipe:[999]"), StreamKind::Pipe);
    assert_eq!(stream_kind("/dev/pts/3"), StreamKind::Terminal);
    assert_eq!(stream_kind("/dev/tty1"), StreamKind::Terminal);
    assert_eq!(stream_kind("/var/log/app.log"), StreamKind::File);
    assert_eq!(stream_kind("anon_inode:[eventpoll]"), StreamKind::Other);
}

#[test]
fn a_deleted_program_is_told_apart_from_one_with_a_name_that_ends_in_the_same_words() {
    assert_eq!(program_of("/usr/bin/curl"), ("/usr/bin/curl".into(), false));
    assert_eq!(program_of("/tmp/.x/miner (deleted)"), ("/tmp/.x/miner".into(), true));
    assert_eq!(program_of("/opt/deleted"), ("/opt/deleted".into(), false));
}

#[test]
fn a_command_line_reads_as_its_arguments_without_the_zero_bytes() {
    assert_eq!(command_of(b"curl\0-s\0http://example/u.sh\0"), ["curl", "-s", "http://example/u.sh"]);
    assert_eq!(command_of(b""), Vec::<String>::new());
}

/// A folder that looks like `/proc`, with one process per folder.
struct FakeProc {
    folder: tempfile::TempDir,
}

impl FakeProc {
    fn new() -> Self {
        Self { folder: tempfile::tempdir().expect("a temporary folder") }
    }

    fn processes(&self) -> Processes {
        Processes::at(self.folder.path())
    }

    /// One process, with the files the watcher reads of it.
    fn process(&self, pid: u32, name: &str, parent: u32, program: &str, command: &[&str]) -> &Self {
        let at = self.folder.path().join(pid.to_string());
        fs::create_dir_all(at.join("fd")).expect("the process folder");
        fs::write(at.join("stat"), stat_line(name, parent, 0, u64::from(pid) * 10)).expect("stat");
        fs::write(at.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").expect("status");
        fs::write(at.join("cmdline"), command.join("\0")).expect("cmdline");
        fs::write(at.join("environ"), "PATH=/usr/bin\0").expect("environ");
        std::os::unix::fs::symlink(program, at.join("exe")).expect("exe");
        self
    }

    /// Puts a process on a terminal, in the session that `session` started.
    fn on_terminal(&self, pid: u32, name: &str, parent: u32, session: u32) -> &Self {
        let line = session_stat_line(name, parent, session, TERMINAL_DEVICE, u64::from(pid) * 10);
        fs::write(self.folder.path().join(pid.to_string()).join("stat"), line).expect("stat");
        self
    }

    /// What one of a process's standard streams leads to.
    fn stream(&self, pid: u32, stream: u32, target: &str) -> &Self {
        let at = self.folder.path().join(pid.to_string()).join("fd").join(stream.to_string());
        std::os::unix::fs::symlink(target, at).expect("a stream");
        self
    }
}

#[test]
fn a_process_reads_with_its_program_streams_and_the_parents_above_it() {
    let proc = FakeProc::new();
    proc.process(1, "systemd", 0, "/sbin/init", &["/sbin/init"]);
    proc.process(100, "nginx", 1, "/usr/sbin/nginx", &["nginx: worker process"]);
    proc.process(200, "sh", 100, "/bin/sh", &["sh", "-i"]);
    proc.stream(200, 0, "socket:[4242]").stream(200, 1, "socket:[4242]").stream(200, 2, "/dev/null");
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);

    let started = proc.processes().started(200, at).expect("the process is there");
    assert_eq!(started.key, ProcessKey { pid: 200, start: 2000 });
    assert_eq!(started.name, "sh");
    assert_eq!(started.program.as_deref(), Some(Path::new("/bin/sh")));
    assert!(!started.program_deleted);
    assert_eq!(started.command, ["sh", "-i"]);
    assert_eq!(started.uid, 1000);
    assert_eq!(started.terminal, None, "a process with no terminal device belongs to none");
    assert_eq!(started.streams(), [StreamKind::Socket, StreamKind::Socket, StreamKind::File]);
    assert_eq!(started.session_parent, None, "a process with no terminal opened none");
    assert_eq!(started.at, at);
    let parents: Vec<&str> = started.ancestors.iter().map(|parent| parent.name.as_str()).collect();
    assert_eq!(parents, ["nginx", "systemd"], "the chain stops at the first process");
    assert_eq!(started.parent().map(|parent| parent.command.as_str()), Some("nginx: worker process"));
}

#[test]
fn a_terminal_says_which_program_started_its_session() {
    let proc = FakeProc::new();
    proc.process(1, "systemd", 0, "/sbin/init", &["/sbin/init"]);
    proc.process(50, "sshd", 1, "/usr/sbin/sshd", &["sshd: me@pts/0"]);
    proc.process(60, "bash", 50, "/usr/bin/bash", &["-bash"]).on_terminal(60, "bash", 50, 60);
    proc.process(70, "vim", 60, "/usr/bin/vim", &["vim"]).on_terminal(70, "vim", 60, 60);
    proc.stream(70, 0, "/dev/pts/0");
    let at = SystemTime::UNIX_EPOCH;

    let editor = proc.processes().started(70, at).expect("the process is there");
    assert_eq!(editor.terminal.as_deref(), Some("/dev/pts/0"));
    assert_eq!(editor.session_parent.map(|opener| opener.name), Some("sshd".into()));
    let shell = proc.processes().started(60, at).expect("the session's first process is there");
    assert_eq!(shell.session_parent.map(|opener| opener.pid), Some(50), "the first process counts too");

    proc.process(80, "less", 1, "/usr/bin/less", &["less"]).on_terminal(80, "less", 1, 79);
    let orphan = proc.processes().started(80, at).expect("the process is there");
    assert_eq!(orphan.session_parent, None, "a session whose first process has gone says nothing");
}

#[test]
fn a_process_that_has_gone_reads_as_nothing_rather_than_half_of_one() {
    let proc = FakeProc::new();
    assert_eq!(proc.processes().started(404, SystemTime::UNIX_EPOCH), None);
    assert_eq!(proc.processes().key(404), None);
    assert!(!proc.processes().running(ProcessKey { pid: 404, start: 1 }));
}

#[test]
fn identity_is_the_number_together_with_the_moment_it_started() {
    let proc = FakeProc::new();
    proc.process(300, "sleep", 1, "/bin/sleep", &["sleep", "30"]);
    let processes = proc.processes();
    let key = processes.key(300).expect("the process is there");
    assert!(processes.running(key));
    assert!(!processes.running(ProcessKey { start: key.start + 1, ..key }), "a number used again is another process");
}

#[test]
fn a_user_sees_their_own_processes_and_the_tree_lists_parents_before_children() {
    let proc = FakeProc::new();
    proc.process(1, "systemd", 0, "/sbin/init", &["/sbin/init"]);
    proc.process(10, "bash", 1, "/bin/bash", &["bash"]);
    proc.process(11, "make", 10, "/usr/bin/make", &["make"]);
    proc.process(12, "cc", 11, "/usr/bin/cc", &["cc"]);
    let processes = proc.processes();
    let uid = rustix::process::getuid().as_raw();
    assert_eq!(processes.all(), [1, 10, 11, 12]);
    assert_eq!(processes.of_user(uid), [1, 10, 11, 12]);
    assert_eq!(processes.of_user(uid + 1), Vec::<u32>::new());
    assert_eq!(processes.started(10, SystemTime::UNIX_EPOCH).map(|p| p.uid), Some(1000), "status says who it runs as");
    assert_eq!(processes.tree(10), [10, 11, 12]);
    assert_eq!(processes.tree(12), [12]);
}

#[test]
fn a_process_that_claims_to_be_its_own_parent_does_not_loop() {
    let mut parents = BTreeMap::new();
    parents.insert(5_u32, 5_u32);
    assert_eq!(descendants(&parents, 5), [5]);
    parents.insert(6, 5);
    parents.insert(5, 6);
    assert_eq!(descendants(&parents, 5), [5, 6]);
}
