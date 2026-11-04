//! The watcher that runs as root, so it hears every process on the machine and may freeze one
//! that belongs to another user. It is a separate unit the user installs on purpose, and it
//! does nothing else: it reads the kernel's process events and `/proc`, and it stops, continues
//! or ends a process when Borg asks.
//!
//! Borg itself stays unprivileged. The user's Borg listens on a socket only its owner can
//! reach, the root part connects to it, and each side checks the other: Borg accepts a peer
//! that runs as root, and the root part only connects to a socket that belongs to the user it
//! was installed for.

use anyhow::{Context, Result, bail};
use borg_core::{ControlOutcome, ProcessKey, StartedProcess};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{
        mpsc::{Receiver, UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
};

use super::facts::Processes;

/// The socket in the state folder that the root watcher connects to.
pub const SOCKET: &str = "watch.sock";
/// How long Borg waits for the root watcher to say what became of a process. A watcher that has
/// gone away answers nothing, so the wait has an end.
const ACT_WAIT: Duration = Duration::from_secs(5);
/// The user Borg runs as, which only root may be.
const ROOT: u32 = 0;
/// The unit the root watcher runs as.
const UNIT: &str = "borg-commander-watch.service";
const UNITS: &str = "/etc/systemd/system";

/// What the root watcher sends Borg.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FromRoot {
    /// A process has started, with everything `/proc` says about it.
    Started(Box<StartedProcess>),
    /// What became of a process Borg asked about.
    Outcome { id: u64, outcome: ControlOutcome },
}

/// What Borg asks the root watcher to do to a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    Freeze,
    Resume,
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToRoot {
    Act { id: u64, act: Act, process: ProcessKey },
}

/// Borg's end of the root watcher: the processes it reports, and what Borg asks of them.
pub struct RootLink {
    acts: UnboundedSender<ToRoot>,
    waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<ControlOutcome>>>>,
    next_id: u64,
}

impl RootLink {
    /// Waits for the root watcher to connect to a socket in the state folder that nobody else
    /// can reach, and reports the processes it sends.
    pub async fn listen(state: &Path) -> Result<(Self, UnboundedReceiver<StartedProcess>)> {
        let path = state.join(SOCKET);
        let _ = fs::remove_file(&path);
        fs::create_dir_all(state)?;
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let (started, processes) = unbounded_channel();
        let (acts, mut outgoing) = unbounded_channel();
        let waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<ControlOutcome>>>> = Arc::new(Mutex::new(HashMap::new()));
        let answers = Arc::clone(&waiting);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                if !runs_as_root(&stream) {
                    continue;
                }
                let (reader, mut writer) = stream.into_split();
                let mut incoming = lines_of(reader);
                let started = started.clone();
                let answers = Arc::clone(&answers);
                loop {
                    tokio::select! {
                        line = incoming.recv() => match line {
                            Some(line) => receive(&line, &started, &answers),
                            None => break,
                        },
                        act = outgoing.recv() => match act {
                            Some(act) => {
                                let Ok(mut text) = serde_json::to_string(&act) else { continue };
                                text.push('\n');
                                if writer.write_all(text.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                            None => return,
                        },
                    }
                }
            }
        });
        Ok((Self { acts, waiting, next_id: 0 }, processes))
    }

    /// Asks the root watcher to stop, continue or end a process, and waits for its answer.
    pub async fn act(&mut self, act: Act, process: ProcessKey) -> ControlOutcome {
        self.next_id += 1;
        let id = self.next_id;
        let (answer, wait) = oneshot::channel();
        self.waiting.lock().expect("waiting acts").insert(id, answer);
        if self.acts.send(ToRoot::Act { id, act, process }).is_err() {
            self.waiting.lock().expect("waiting acts").remove(&id);
            return ControlOutcome::Gone;
        }
        let answered = tokio::time::timeout(ACT_WAIT, wait).await;
        self.waiting.lock().expect("waiting acts").remove(&id);
        match answered {
            Ok(Ok(outcome)) => outcome,
            _ => ControlOutcome::Denied,
        }
    }
}

/// The lines a connection sends, read on their own so that waiting for one may be given up at
/// any moment without losing what has already arrived.
fn lines_of(reader: tokio::net::unix::OwnedReadHalf) -> UnboundedReceiver<String> {
    let (sender, receiver) = unbounded_channel();
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    receiver
}

fn receive(
    line: &str,
    started: &UnboundedSender<StartedProcess>,
    waiting: &Arc<Mutex<HashMap<u64, oneshot::Sender<ControlOutcome>>>>,
) {
    match serde_json::from_str(line) {
        Ok(FromRoot::Started(process)) => {
            let _ = started.send(*process);
        }
        Ok(FromRoot::Outcome { id, outcome }) => {
            if let Some(answer) = waiting.lock().expect("waiting acts").remove(&id) {
                let _ = answer.send(outcome);
            }
        }
        Err(_) => {}
    }
}

fn runs_as_root(stream: &UnixStream) -> bool {
    stream.peer_cred().is_ok_and(|peer| peer.uid() == ROOT)
}

/// `borg --watch-root SOCKET UID`: reads the kernel's process events as root and sends them to
/// the Borg of the user who owns the socket.
pub async fn helper(args: &[String]) -> Result<()> {
    let [socket, uid] = args else { bail!("usage: borg --watch-root SOCKET UID") };
    let uid: u32 = uid.parse().context("the user is not a number")?;
    let socket = PathBuf::from(socket);
    let owner = fs::metadata(&socket).context("the socket is not there")?;
    if owner.uid() != uid {
        bail!("{} does not belong to user {uid}", socket.display());
    }
    let stream = UnixStream::connect(&socket).await.context("connecting to Borg")?;
    let processes = Processes::default();
    let events = super::kernel_events()?;
    serve(stream, processes, events).await
}

/// Sends every process that starts, and does what Borg asks of one.
async fn serve(stream: UnixStream, processes: Processes, mut events: Receiver<u32>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut incoming = lines_of(reader);
    loop {
        tokio::select! {
            pid = events.recv() => {
                let Some(pid) = pid else { return Ok(()) };
                let Some(started) = processes.started(pid, SystemTime::now()) else { continue };
                if send(&mut writer, &FromRoot::Started(Box::new(started))).await.is_err() {
                    return Ok(());
                }
            }
            line = incoming.recv() => {
                let Some(line) = line else { return Ok(()) };
                let Ok(ToRoot::Act { id, act, process }) = serde_json::from_str(&line) else { continue };
                let outcome = match act {
                    Act::Freeze => super::freeze::freeze(&processes, process),
                    Act::Resume => super::freeze::resume(&processes, process),
                    Act::Kill => super::freeze::kill(&processes, process),
                };
                if send(&mut writer, &FromRoot::Outcome { id, outcome }).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

async fn send(writer: &mut tokio::net::unix::OwnedWriteHalf, message: &FromRoot) -> Result<()> {
    let mut text = serde_json::to_string(message)?;
    text.push('\n');
    writer.write_all(text.as_bytes()).await?;
    Ok(())
}

/// The unit that runs the root watcher, as the user installing it would read it.
fn unit(program: &str, socket: &Path, uid: u32) -> String {
    format!(
        "[Unit]\nDescription=Borg Commander process watcher for user {uid}\nAfter=network-online.target\n\n[Service]\nType=simple\nExecStart=\"{}\" --watch-root \"{}\" {uid}\nRestart=on-failure\nRestartSec=5\nProtectHome=read-only\nNoNewPrivileges=yes\nCapabilityBoundingSet=CAP_NET_ADMIN CAP_KILL CAP_SYS_PTRACE CAP_DAC_READ_SEARCH\n\n[Install]\nWantedBy=multi-user.target\n",
        program.replace('"', "\\\""),
        socket.display()
    )
}

fn state() -> Result<PathBuf> {
    super::log::folder()
}

/// Installs the root watcher, which needs the user's password once. Until it runs, Borg watches
/// only the user's own processes.
pub fn install() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("the root watcher is a Linux feature for now");
    }
    if !Path::new("/run/systemd/system").is_dir() {
        bail!(
            "systemd is not running here; run `borg --watch-root SOCKET UID` as root under this machine's supervisor"
        );
    }
    let state = state()?;
    let socket = state.join(SOCKET);
    let uid = rustix::process::getuid().as_raw();
    let text = unit(&crate::agent::program_path(), &socket, uid);
    let staged = state.join(UNIT);
    fs::write(&staged, text)?;
    run(Command::new("sudo").args(["install", "-m", "0644"]).arg(&staged).arg(Path::new(UNITS).join(UNIT)))?;
    run(Command::new("sudo").args(["systemctl", "daemon-reload"]))?;
    run(Command::new("sudo").args(["systemctl", "enable", "--now", UNIT]))?;
    let _ = fs::remove_file(&staged);
    println!("Installed {UNIT}. Borg now hears every process on this machine.");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = run(Command::new("sudo").args(["systemctl", "disable", "--now", UNIT]));
    run(Command::new("sudo").args(["rm", "--"]).arg(Path::new(UNITS).join(UNIT)))?;
    run(Command::new("sudo").args(["systemctl", "daemon-reload"]))?;
    println!("Removed {UNIT}. Borg watches only your own processes now.");
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    let display = format!("{command:?}");
    let status = command.status().with_context(|| format!("running {display}"))?;
    if !status.success() {
        bail!("{display} exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_runs_borgs_own_program_as_the_watcher_and_nothing_else() {
        let text = unit("/usr/local/bin/borg", Path::new("/home/sam/.local/state/borg/watch.sock"), 1000);
        assert!(text.contains(
            "ExecStart=\"/usr/local/bin/borg\" --watch-root \"/home/sam/.local/state/borg/watch.sock\" 1000"
        ));
        assert!(text.contains("NoNewPrivileges=yes"));
        assert!(text.contains("CapabilityBoundingSet=CAP_NET_ADMIN"));
    }

    #[test]
    fn a_program_path_with_quotes_cannot_break_out_of_the_unit() {
        let text = unit("/tmp/bo\"rg", Path::new("/tmp/watch.sock"), 1);
        assert!(text.contains("ExecStart=\"/tmp/bo\\\"rg\""), "{text}");
    }

    /// Borg takes processes only from a connection that runs as root, so this test, which cannot
    /// be root, stands for every other program on the machine.
    #[tokio::test]
    async fn borg_takes_no_processes_from_a_connection_that_is_not_root() {
        let folder = tempfile::tempdir().unwrap();
        let (_link, mut processes) = RootLink::listen(folder.path()).await.unwrap();
        let socket = folder.path().join(SOCKET);
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600,
            "only its owner can reach the socket"
        );
        let stream = UnixStream::connect(&socket).await.unwrap();
        let (_reader, mut writer) = stream.into_split();
        let started = StartedProcess { name: "sh".into(), ..StartedProcess::default() };
        let mut text = serde_json::to_string(&FromRoot::Started(Box::new(started))).unwrap();
        text.push('\n');
        let _ = writer.write_all(text.as_bytes()).await;
        let heard = tokio::time::timeout(Duration::from_millis(200), processes.recv()).await;
        assert!(heard.is_err(), "nothing that is not the root watcher may report a process");
    }

    #[tokio::test]
    async fn the_lines_the_root_watcher_sends_become_processes_and_answers() {
        let (sender, mut processes) = unbounded_channel();
        let waiting: Arc<Mutex<HashMap<u64, oneshot::Sender<ControlOutcome>>>> = Arc::new(Mutex::new(HashMap::new()));
        let started = StartedProcess { name: "sh".into(), ..StartedProcess::default() };
        let line = serde_json::to_string(&FromRoot::Started(Box::new(started))).unwrap();
        receive(&line, &sender, &waiting);
        assert_eq!(processes.recv().await.map(|process| process.name), Some("sh".into()));

        let (answer, wait) = oneshot::channel();
        waiting.lock().expect("waiting acts").insert(7, answer);
        let line = serde_json::to_string(&FromRoot::Outcome { id: 7, outcome: ControlOutcome::Done }).unwrap();
        receive(&line, &sender, &waiting);
        assert_eq!(wait.await, Ok(ControlOutcome::Done));
        assert!(waiting.lock().expect("waiting acts").is_empty(), "an answer is waited for once");

        receive("not json at all", &sender, &waiting);
        let stray = serde_json::to_string(&FromRoot::Outcome { id: 9, outcome: ControlOutcome::Gone }).unwrap();
        receive(&stray, &sender, &waiting);
    }

    #[tokio::test]
    async fn an_act_nobody_answers_gives_up_instead_of_waiting_for_ever() {
        let folder = tempfile::tempdir().unwrap();
        let (mut link, _processes) = RootLink::listen(folder.path()).await.unwrap();
        tokio::time::pause();
        let asked = tokio::spawn(async move { link.act(Act::Freeze, ProcessKey { pid: 4242, start: 7 }).await });
        tokio::time::advance(ACT_WAIT + Duration::from_secs(1)).await;
        assert_eq!(asked.await.unwrap(), ControlOutcome::Denied);
    }

    #[test]
    fn what_borg_asks_for_and_what_comes_back_read_as_the_same_words_on_both_sides() {
        let act = ToRoot::Act { id: 3, act: Act::Resume, process: ProcessKey { pid: 9, start: 1 } };
        let text = serde_json::to_string(&act).unwrap();
        let ToRoot::Act { id, act, process } = serde_json::from_str(&text).unwrap();
        assert_eq!((id, act, process), (3, Act::Resume, ProcessKey { pid: 9, start: 1 }));
        let answer = serde_json::to_string(&FromRoot::Outcome { id: 3, outcome: ControlOutcome::Denied }).unwrap();
        assert!(matches!(
            serde_json::from_str(&answer).unwrap(),
            FromRoot::Outcome { id: 3, outcome: ControlOutcome::Denied }
        ));
    }
}
