//! Runs the Docker view's requests through the `docker` CLI: list and inspect queries, one
//! at a time, actions and file work alongside them, the stats stream while the view is
//! open, and folder listings for panels that show a container's filesystem.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use borg_core::{DockerData, DockerRequest, FileOp, Msg};
use borg_term::{CommandBuilder, Emulator, GridSize, Pty, PtyEvent, Screen, Scroll, ShellSignal, TermMode};
use borg_vfs::{Batches, Entry, EntryKind};
use futures::StreamExt;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};

/// How long to wait before restarting a stats stream that ended, such as when the engine
/// restarts.
const WATCH_RETRY: Duration = Duration::from_secs(5);

pub struct Docker {
    messages: UnboundedSender<Msg>,
    query: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
    /// Where container files are copied out for viewing; removed on exit.
    scratch: PathBuf,
}

impl Docker {
    pub fn new(messages: UnboundedSender<Msg>, scratch: PathBuf) -> Self {
        Self { messages, query: None, watch: None, scratch }
    }

    pub fn request(&mut self, id: u64, request: DockerRequest) {
        let messages = self.messages.clone();
        let query = request.is_query();
        let scratch = self.scratch.join(id.to_string());
        let task = tokio::spawn(async move {
            let result = match request {
                DockerRequest::List => {
                    let (context, containers) = tokio::join!(borg_docker::context(), borg_docker::list());
                    containers.map(|containers| DockerData::Listed { context: context.ok(), containers })
                }
                DockerRequest::Inspect(id) => borg_docker::inspect(&id)
                    .await
                    .map(|details| DockerData::Inspected { id, details: Box::new(details) }),
                DockerRequest::Act { action, ids } => borg_docker::act(action, &ids).await.map(|()| DockerData::Done),
                DockerRequest::Files { container, op } => files(&container, op, &scratch).await,
            };
            let _ = messages.send(Msg::Docker { id, result: result.map_err(|error| format!("{error:#}")) });
        });
        // Actions run to the end; a newer query replaces one still running, whose answer
        // the view would ignore anyway.
        if query && let Some(previous) = self.query.replace(task) {
            previous.abort();
        }
    }

    pub fn watch(&mut self, on: bool) {
        if let Some(task) = self.watch.take() {
            task.abort();
        }
        if !on {
            return;
        }
        let messages = self.messages.clone();
        self.watch = Some(tokio::spawn(async move {
            loop {
                let sender = messages.clone();
                let _ = borg_docker::watch(move |frame| {
                    let _ = sender.send(Msg::DockerStats(frame));
                })
                .await;
                tokio::time::sleep(WATCH_RETRY).await;
            }
        }));
    }

    /// Stops the running query once the view no longer waits for it.
    pub fn cancel(&mut self) {
        if let Some(task) = self.query.take() {
            task.abort();
        }
    }
}

impl Drop for Docker {
    fn drop(&mut self) {
        self.cancel();
        self.watch(false);
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

async fn files(container: &str, op: FileOp, scratch: &std::path::Path) -> anyhow::Result<DockerData> {
    match op {
        FileOp::Delete { paths } => borg_docker::remove(container, &paths).await?,
        FileOp::Mkdir { path } => borg_docker::mkdir(container, &path).await?,
        FileOp::Fetch { path } => return borg_docker::fetch(container, &path, scratch).await.map(DockerData::Fetched),
    }
    Ok(DockerData::Done)
}

/// Lists a folder inside a container for a panel, in one batch.
pub fn listing(container: String, dir: PathBuf) -> Batches {
    futures::stream::once(async move {
        let files =
            borg_docker::files(&container, &dir).await.map_err(|error| io::Error::other(format!("{error:#}")))?;
        Ok(files.into_iter().map(|file| entry(&dir, file)).collect())
    })
    .boxed()
}

fn entry(dir: &std::path::Path, file: borg_docker::File) -> Entry {
    const PERMISSION_BITS: u32 = 0o7777;
    let kind = if file.is_dir() {
        EntryKind::Dir
    } else if file.is_link() {
        EntryKind::Symlink { to_dir: file.to_dir }
    } else if file.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Entry {
        source: Some(dir.join(&file.name)),
        missing: false,
        usage: None,
        kind,
        size: file.size,
        modified: u64::try_from(file.modified).ok().map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
        executable: file.executable(),
        permissions: file.mode & PERMISSION_BITS,
        name: file.name,
    }
}

/// Lines of history each container shell keeps.
const SHELL_SCROLLBACK: usize = 2000;
/// Bash when the image has it, otherwise the POSIX shell every image with a shell has, with
/// the prompt `BORG_SH_PS1` describes.
const SHELL_SCRIPT: &str =
    "if command -v bash >/dev/null 2>&1; then exec bash; fi; PS1=$BORG_SH_PS1; export PS1; exec sh";

/// Bash's prompt hook: it reports the last command's exit status, the folder, and where the
/// prompt starts and ends (OSC 133 and OSC 7, as the host's integration does), and opens the
/// prompt with the container's name in reverse video, so the prompt says where it runs.
const BASH_PROMPT: &str = r#"__borg_s=$?; printf '\033]133;D;%s\007\033]7;file://%s%s\007\033]133;A\007' "$__borg_s" "${HOSTNAME:-}" "${PWD//%/%25}"; case "$PS1" in *'133;B'*) ;; *) PS1='\[\033[7m\] '"$BORG_PROMPT"' \[\033[0m\] '"$PS1"'\[\033]133;B\007\]' ;; esac"#;

/// The same for a POSIX shell, which has no prompt hook: the prompt itself carries the
/// markers, expanded each time it is shown.
fn sh_prompt(name: &str) -> String {
    format!("\x1b]133;D;$?\x07\x1b]7;file://$PWD\x07\x1b]133;A\x07\x1b[7m {name} \x1b[0m $PWD # \x1b]133;B\x07")
}

struct Shell {
    pty: Pty,
    emulator: Emulator,
    size: GridSize,
    /// The shell has shown its first prompt, so typed commands reach it.
    prompted: bool,
}

/// An agent's command typed into a shell, collecting what the terminal receives until the
/// shell announces the command's end.
struct Capture {
    command: crate::agent::Typed,
    typed: bool,
    bytes: Vec<u8>,
}

/// A finished agent command: its exit status and output.
pub struct Captured {
    pub command: crate::agent::Typed,
    pub exit: Option<i32>,
    pub output: String,
}

/// Types a command at the shell's prompt, clearing anything half typed there first (Ctrl-U),
/// with a space before it so the shell's history leaves it out.
fn type_command(shell: &mut Shell, command: &str) {
    shell.emulator.scroll(Scroll::Bottom);
    let _ = shell.pty.write(format!("\x15 {command}\r").as_bytes());
}

/// Shells running inside containers, each with its own terminal, kept until they exit.
pub struct Shells {
    shells: HashMap<String, Shell>,
    captures: HashMap<String, Capture>,
    events: UnboundedSender<(String, PtyEvent)>,
    pub incoming: UnboundedReceiver<(String, PtyEvent)>,
}

impl Shells {
    pub fn new() -> Self {
        let (events, incoming) = unbounded_channel();
        Self { shells: HashMap::new(), captures: HashMap::new(), events, incoming }
    }

    pub fn open(
        &mut self,
        container: String,
        prompt: &str,
        size: GridSize,
        workdir: Option<&Path>,
    ) -> anyhow::Result<()> {
        let mut command = CommandBuilder::new("docker");
        command.args(["exec", "-it", "-e", "TERM=xterm-256color", "-e", "HISTCONTROL=ignorespace"]);
        command.args(["-e", &format!("BORG_PROMPT={prompt}"), "-e", &format!("PROMPT_COMMAND={BASH_PROMPT}")]);
        command.args(["-e", "PS0=\x1b]133;C\x07", "-e", &format!("BORG_SH_PS1={}", sh_prompt(prompt))]);
        if let Some(dir) = workdir {
            command.args(["-w", &dir.to_string_lossy()]);
        }
        command.args([&container, "sh", "-c", SHELL_SCRIPT]);
        let (sender, mut receiver) = unbounded_channel();
        let pty = Pty::spawn(command, size, sender)?;
        let events = self.events.clone();
        let name = container.clone();
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if events.send((name.clone(), event)).is_err() {
                    return;
                }
            }
        });
        let emulator = Emulator::new(size, SHELL_SCROLLBACK);
        self.shells.insert(container, Shell { pty, emulator, size, prompted: false });
        Ok(())
    }

    /// Feeds a shell's output to its terminal and returns the terminal's input mode, the
    /// shell's signals (prompts, commands, and folder changes), and an agent's command that
    /// this output finished.
    pub fn feed(&mut self, container: &str, bytes: &[u8]) -> Option<(TermMode, Vec<ShellSignal>, Option<Captured>)> {
        let shell = self.shells.get_mut(container)?;
        let signals = shell.emulator.feed(bytes);
        for reply in shell.emulator.drain_replies() {
            let _ = shell.pty.write(reply.as_bytes());
        }
        shell.prompted |= signals.iter().any(|signal| matches!(signal, ShellSignal::PromptStart));
        let mut captured = None;
        if let Some(capture) = self.captures.get_mut(container) {
            if capture.typed {
                capture.bytes.extend_from_slice(bytes);
                let end = signals.iter().find_map(|signal| match signal {
                    ShellSignal::CommandEnd { exit } => Some(*exit),
                    _ => None,
                });
                if let (Some(exit), Some(capture)) = (end, end.and_then(|_| self.captures.remove(container))) {
                    let output = crate::agent::shell_output(&capture.bytes, &capture.command.line);
                    captured = Some(Captured { command: capture.command, exit, output });
                }
            } else if shell.prompted {
                type_command(shell, &capture.command.line);
                capture.typed = true;
            }
        }
        Some((shell.emulator.mode(), signals, captured))
    }

    /// Types an approved agent command into a container's shell, now if it is at its prompt,
    /// or at its first prompt if it is starting. Without a shell, the command comes back.
    pub fn run(&mut self, container: &str, command: crate::agent::Typed) -> Result<(), crate::agent::Typed> {
        let Some(shell) = self.shells.get_mut(container) else { return Err(command) };
        let typed = shell.prompted;
        if typed {
            type_command(shell, &command.line);
        }
        self.captures.insert(container.to_owned(), Capture { command, typed, bytes: Vec::new() });
        Ok(())
    }

    /// The terminal and process of a shell, while it is the console.
    pub fn parts(&mut self, container: &str) -> Option<(&mut Emulator, &mut Pty)> {
        self.shells.get_mut(container).map(|shell| (&mut shell.emulator, &mut shell.pty))
    }

    pub fn input(&mut self, container: &str, bytes: &[u8]) {
        if let Some(shell) = self.shells.get_mut(container) {
            shell.emulator.scroll(Scroll::Bottom);
            let _ = shell.pty.write(bytes);
        }
    }

    pub fn scroll(&mut self, container: &str, scroll: Scroll) {
        if let Some(shell) = self.shells.get_mut(container) {
            shell.emulator.scroll(scroll);
        }
    }

    /// Resizes a shell's terminal to the panel it shows in.
    pub fn fit(&mut self, container: &str, size: GridSize) {
        if let Some(shell) = self.shells.get_mut(container).filter(|shell| shell.size != size) {
            shell.size = size;
            shell.emulator.resize(size);
            let _ = shell.pty.resize(size);
        }
    }

    /// Forgets a shell that closed, returning an agent command still waiting on it.
    pub fn close(&mut self, container: &str) -> Option<crate::agent::Typed> {
        self.shells.remove(container);
        self.captures.remove(container).map(|capture| capture.command)
    }

    pub fn screen(&self, container: &str) -> Option<Screen<'_>> {
        self.shells.get(container).map(|shell| shell.emulator.screen())
    }
}

/// A copy between a container and a host folder, run as a copy job: it reports progress
/// item by item, asks before replacing what exists (Overwrite, Skip, Keep both, or the same
/// for all), asks again when an item fails, and stops when cancelled. Runs on a blocking
/// thread, since the job waits for answers.
pub struct CopyJob {
    pub container: String,
    pub inbound: bool,
    pub sources: Vec<PathBuf>,
    pub target: PathBuf,
}

/// Size and modification time of host paths that exist.
fn host_stat(paths: &[PathBuf]) -> HashMap<PathBuf, (u64, i64)> {
    paths
        .iter()
        .filter_map(|path| {
            let meta = std::fs::symlink_metadata(path).ok()?;
            let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some((path.clone(), (meta.len(), i64::try_from(modified).unwrap_or(i64::MAX))))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sticky {
    Ask,
    Always,
    Never,
}

impl CopyJob {
    pub fn run(
        self,
        runtime: &tokio::runtime::Handle,
        control: &borg_jobs::Control,
        emit: &mut dyn FnMut(borg_jobs::Event),
    ) -> borg_jobs::Outcome {
        use borg_jobs::{Ask, Decision, Event, Outcome, Progress, Totals};
        let Self { container, inbound, sources, target } = self;
        let total = Totals { files: sources.len() as u64, bytes: 0 };
        emit(Event::Scanning(total));
        let dests: Vec<PathBuf> =
            sources.iter().map(|source| target.join(source.file_name().unwrap_or_default())).collect();
        let stat_inside = |paths: &[PathBuf]| runtime.block_on(borg_docker::stat(&container, paths));
        let (incoming, existing) = if inbound {
            (Ok(host_stat(&sources)), stat_inside(&dests))
        } else {
            (stat_inside(&sources), Ok(host_stat(&dests)))
        };
        let (incoming, existing) = match (incoming, existing) {
            (Ok(incoming), Ok(existing)) => (incoming, existing),
            (Err(error), _) | (_, Err(error)) => return Outcome::Failed(format!("{error:#}")),
        };
        let mut done = Totals::default();
        let mut skipped = 0;
        let (mut overwrite, mut errors) = (Sticky::Ask, Sticky::Ask);
        for (source, dest) in sources.iter().zip(dests) {
            if !control.proceed() {
                return Outcome::Cancelled { done };
            }
            emit(Event::Progress(Progress { done, total, current: source.clone() }));
            let mut into = target.clone();
            if let Some(&(size, modified)) = existing.get(&dest) {
                let (incoming_size, incoming_modified) = incoming.get(source).copied().unwrap_or_default();
                let decision = match overwrite {
                    Sticky::Always => Decision::Overwrite,
                    Sticky::Never => Decision::Skip,
                    Sticky::Ask => {
                        emit(Event::Asking(Ask::Conflict {
                            source: source.clone(),
                            target: dest.clone(),
                            sizes: (incoming_size, size),
                            newer: incoming_modified > modified,
                        }));
                        control.decide()
                    }
                };
                match decision {
                    Decision::Overwrite | Decision::Retry => {}
                    Decision::OverwriteAll => overwrite = Sticky::Always,
                    Decision::Skip | Decision::SkipAll => {
                        if decision == Decision::SkipAll {
                            overwrite = Sticky::Never;
                        }
                        skipped += 1;
                        continue;
                    }
                    Decision::KeepBoth => into = self::free_name(runtime, &container, inbound, &dest),
                    Decision::Cancel => return Outcome::Cancelled { done },
                }
            }
            loop {
                let copied = runtime.block_on(async {
                    if inbound {
                        borg_docker::copy_in(&container, source, &into).await
                    } else {
                        borg_docker::copy_out(&container, source, &into).await
                    }
                });
                let Err(error) = copied else {
                    done.files += 1;
                    break;
                };
                let decision = if errors == Sticky::Never {
                    Decision::Skip
                } else {
                    emit(Event::Asking(Ask::Failure { path: source.clone(), message: format!("{error:#}") }));
                    control.decide()
                };
                match decision {
                    Decision::Retry => {}
                    Decision::Cancel => return Outcome::Cancelled { done },
                    other => {
                        if matches!(other, Decision::SkipAll | Decision::OverwriteAll) {
                            errors = Sticky::Never;
                        }
                        skipped += 1;
                        break;
                    }
                }
            }
        }
        emit(Event::Progress(Progress { done, total, current: target.clone() }));
        Outcome::Done { done, skipped }
    }
}

/// The first "name (2)" style name free beside `dest`, for Keep both. The candidates are
/// checked in one call, so a container is asked once.
fn free_name(runtime: &tokio::runtime::Handle, container: &str, inbound: bool, dest: &std::path::Path) -> PathBuf {
    const CANDIDATES: usize = 50;
    let mut candidates = Vec::new();
    // Collects the first candidate names by calling every one taken.
    borg_jobs::unique_by(dest, |candidate| {
        candidates.push(candidate.to_path_buf());
        candidates.len() < CANDIDATES
    });
    let taken = if inbound {
        runtime.block_on(borg_docker::stat(container, &candidates)).unwrap_or_default()
    } else {
        host_stat(&candidates)
    };
    borg_jobs::unique_by(dest, |candidate| taken.contains_key(candidate))
}
