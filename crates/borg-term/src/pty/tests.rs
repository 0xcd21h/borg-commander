use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;

const WAIT: Duration = Duration::from_secs(10);
const SIZE: GridSize = GridSize { cols: 100, rows: 30 };
/// Set for this test binary when it runs as the launcher of the leading test.
const LEADING: &str = "BORG_TERM_TEST_LEADING";
const LEADING_TEST: &str = "pty::tests::the_launcher_gives_the_command_the_terminal_as_its_own";
const EXECUTABLE: u32 = 0o755;

/// A launcher that runs the command without leading the terminal, for tests that need no
/// controlling terminal.
fn passing_launcher(dir: &tempfile::TempDir) -> PathBuf {
    let path = dir.path().join("launcher");
    std::fs::write(&path, "#!/bin/sh\nshift\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(EXECUTABLE)).unwrap();
    path
}

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}

/// The terminal's output until it shows `wanted`.
async fn read_until(events: &mut UnboundedReceiver<PtyEvent>, wanted: &str) -> String {
    read_while(events, |seen| !seen.contains(wanted)).await
}

/// The terminal's output once `more` is false of it.
async fn read_while(events: &mut UnboundedReceiver<PtyEvent>, more: impl Fn(&str) -> bool) -> String {
    let mut seen = String::new();
    tokio::time::timeout(WAIT, async {
        while more(&seen) {
            match events.recv().await {
                Some(PtyEvent::Output(bytes)) => seen.push_str(&String::from_utf8_lossy(&bytes)),
                Some(PtyEvent::Closed) | None => break,
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the terminal stopped short in {seen:?}"));
    seen
}

#[tokio::test]
async fn a_program_writes_through_its_terminal_and_reads_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, mut events) = unbounded_channel();
    let pty = Pty::spawn(&passing_launcher(&dir), &shell("stty size; echo done"), SIZE, sender).unwrap();
    let seen = read_until(&mut events, "done").await;
    assert!(seen.contains("30 100"), "{seen:?}");
    drop(pty);
}

#[tokio::test]
async fn a_resized_terminal_tells_its_program_the_new_size() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, mut events) = unbounded_channel();
    let script = "echo ready; read line; stty size; echo done";
    let mut pty = Pty::spawn(&passing_launcher(&dir), &shell(script), SIZE, sender).unwrap();
    read_until(&mut events, "ready").await;
    pty.resize(GridSize { cols: 120, rows: 40 }).unwrap();
    pty.write(b"\n").unwrap();
    let seen = read_until(&mut events, "done").await;
    assert!(seen.contains("40 120"), "{seen:?}");
}

#[tokio::test]
async fn a_detached_terminal_keeps_its_program_for_the_next_owner() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, mut first) = unbounded_channel();
    let script = "echo ready; read line; echo got:$line";
    let mut pty = Pty::spawn(&passing_launcher(&dir), &shell(script), SIZE, sender).unwrap();
    read_until(&mut first, "ready").await;
    let detached = pty.detach().unwrap();
    drop(pty);
    let (sender, mut second) = unbounded_channel();
    let mut adopted = Pty::adopt(detached, sender).unwrap();
    adopted.write(b"hello\n").unwrap();
    let seen = read_until(&mut second, "got:hello").await;
    assert!(seen.contains("got:hello"));
    assert!(
        std::iter::from_fn(|| first.try_recv().ok()).all(|event| !matches!(event, PtyEvent::Output(_))),
        "the first owner reads nothing once it has let go"
    );
    adopted.stop().unwrap();
}

#[tokio::test]
async fn stopping_ends_a_program_that_ignores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, mut events) = unbounded_channel();
    let mut pty = Pty::spawn(&passing_launcher(&dir), &shell("echo up; sleep 30"), SIZE, sender).unwrap();
    read_until(&mut events, "up").await;
    pty.stop().unwrap();
    assert!(pty.exited().unwrap());
}

#[tokio::test]
async fn the_launcher_gives_the_command_the_terminal_as_its_own() {
    if std::env::var_os(LEADING).is_some() {
        let error = lead(OsStr::new("/bin/sh"), &["-c".into(), "echo leader:$$; sleep 30".into()]);
        panic!("{error}");
    }
    let master = open_master().unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let terminal =
        rustix::fs::open(name.as_c_str(), OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC, Mode::empty()).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([LEADING_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(LEADING, "1")
        .stdin(Stdio::from(terminal.try_clone().unwrap()))
        .stdout(Stdio::from(terminal.try_clone().unwrap()))
        .stderr(Stdio::from(terminal))
        .spawn()
        .unwrap();
    let (sender, mut events) = unbounded_channel();
    let pty = Pty::start(master, pid(child.id()).unwrap(), sender).unwrap();
    let seen =
        read_while(&mut events, |seen| seen.split_once("leader:").is_none_or(|(_, rest)| !rest.contains('\n'))).await;
    assert!(seen.contains(&format!("leader:{}", child.id())), "{seen:?}");
    assert_eq!(pty.foreground(), Foreground::Shell);
    child.kill().unwrap();
    child.wait().unwrap();
}
