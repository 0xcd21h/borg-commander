use std::path::PathBuf;

use borg_term::{Scroll, ShellSignal};
use borg_vfs::Entry;
use crossterm::event::KeyEvent;

use borg_jobs::{Event, Spec};

use crate::{Aside, JobCommand, Peeked, ShellView, Side, Viewer};

#[derive(Debug)]
pub enum Msg {
    Sized {
        id: u64,
        entries: Vec<Entry>,
    },
    SizeProgress {
        id: u64,
        progress: crate::SearchProgress,
    },
    SizesFinished {
        id: u64,
        error: Option<String>,
    },
    MemorySearchProgress {
        id: u64,
        scanned: u64,
        total: u64,
        hits: usize,
    },
    SizeChildren {
        side: Side,
        generation: u64,
        parent: PathBuf,
        result: Result<(Vec<Entry>, bool), String>,
    },
    Activity {
        id: u64,
        result: Result<crate::ActivityData, String>,
    },
    Docker {
        id: u64,
        result: Result<crate::DockerData, String>,
    },
    /// One `docker stats` sample: each running container's id and resource use.
    DockerStats(Vec<(String, borg_docker::Stats)>),
    /// A container shell printed something; `mode` is its terminal's input mode now.
    DockerShellOutput {
        container: String,
        mode: borg_term::TermMode,
    },
    DockerShellClosed {
        container: String,
    },
    Agent(crate::AgentEvent),
    RemoteConsoleOutput {
        echoed: bool,
        machine: String,
        session: String,
        view: ShellView,
        signals: Vec<ShellSignal>,
    },
    RemoteConsoleClosed {
        machine: String,
        session: String,
        error: String,
    },
    RemoteNames(Result<crate::RemoteNames, String>),
    RemoteClients(Vec<crate::RemoteClient>),
    RemoteSessionDeleted {
        machine: String,
        session: String,
        result: Result<(), String>,
    },
    RemoteMirrorConnected(Result<(), String>),
    RemoteHostForgotten {
        machine: String,
        result: Result<(), String>,
    },
    RemoteCatalog {
        choices: Vec<crate::RemoteChoice>,
        complete: bool,
    },
    MountRemote {
        side: Side,
        machine: String,
        session: String,
        name: String,
    },
    /// A signal from a shell that is not the console: the host's, or a container's.
    ParkedSignal {
        container: Option<String>,
        signal: ShellSignal,
    },
    SearchProgress {
        side: Side,
        generation: u64,
        progress: crate::SearchProgress,
    },
    Key(KeyEvent),
    Pointer(crate::Pointer),
    Paste(String),
    Resize {
        cols: u16,
        rows: u16,
    },
    Batch {
        side: Side,
        generation: u64,
        entries: Vec<Entry>,
    },
    Listed {
        side: Side,
        generation: u64,
        error: Option<String>,
    },
    Observe(ShellView),
    ShellOutput(ShellView),
    Signal(ShellSignal),
    Tick,
    Opened(Viewer),
    /// Why the viewer could not open `path`.
    OpenFailed {
        path: PathBuf,
        notice: String,
    },
    /// A file read for the editor, or why it was not.
    EditorOpened(Result<crate::EditorFile, crate::OpenError>),
    /// The editor's save landed, with the file's new stamp, or did not.
    EditorSaved(Result<crate::FileStamp, crate::SaveError>),
    ShellExited,
    Notice(String),
    /// An index for instant search was built, with this many entries, or failed.
    Indexed {
        root: PathBuf,
        entries: usize,
        error: Option<String>,
    },
    Job {
        id: u64,
        event: Event,
    },
    Peeked(Peeked),
    GitStatus {
        side: Side,
        generation: u64,
        dir: PathBuf,
        result: Result<Option<borg_git::Repository>, String>,
    },
    GitFinished {
        id: u64,
        result: Result<Option<Viewer>, String>,
    },
    Backup(crate::BackupEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    CancelShellExit {
        token: String,
    },
    MeasureSizes {
        id: u64,
        root: PathBuf,
    },
    CancelMeasure,
    CancelMemorySearch,
    SizeChildren {
        side: Side,
        generation: u64,
        root: PathBuf,
        hidden: bool,
        parent: PathBuf,
    },
    Activity {
        id: u64,
        request: crate::ActivityRequest,
    },
    Docker {
        id: u64,
        request: crate::DockerRequest,
    },
    /// Starts or stops streaming container stats.
    DockerWatch(bool),
    DockerShell(crate::ShellRequest),
    /// The command line and Ctrl-O console now belong to this shell.
    UseConsole(crate::Console),
    MirrorRemote {
        machine: String,
        session: String,
    },
    OpenRemotes,
    CancelRemoteConnection,
    ForgetRemoteHost {
        machine: String,
    },
    DeleteRemoteSession {
        machine: String,
        session: String,
    },
    CloseRemoteConsole {
        machine: String,
        session: String,
    },
    SaveRemoteName {
        identity: String,
        session: Option<String>,
        name: String,
    },
    MountRemote {
        side: Side,
        machine: String,
        session: String,
        name: String,
    },
    ApprovalModeOff,
    /// The user's answer to an agent's command.
    AgentAnswer {
        id: u64,
        approved: bool,
    },
    /// Puts text on the clipboard of the computer the user sits at, through the terminal.
    Copy(String),
    /// Shows text alone in the terminal, outside borg's screen, for the user to select and
    /// copy where the terminal does not take the clipboard; a key brings borg back.
    ShowPlain(String),
    /// Makes a pairing link for borg cloud, for the Connect dialog.
    StartPairing,
    /// Saves the relay borg cloud goes through, empty for none, and makes a pairing link there.
    SetRelay(String),
    /// Forgets every computer paired through borg cloud, and ends their sessions.
    ForgetComputers,
    ExportMemory {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    SaveGame2048,
    Search {
        side: Side,
        generation: u64,
        request: crate::SearchRequest,
    },
    RefreshCollection {
        side: Side,
        generation: u64,
        entries: Vec<Entry>,
    },
    CancelSearch(Side),
    SaveCollections,
    /// Writes the search history, or removes it once it is empty.
    SaveSearchHistory,
    /// Saves the folders indexed for instant search, builds new ones and drops removed ones.
    SetIndexRoots(Vec<PathBuf>),
    List {
        side: Side,
        generation: u64,
        dir: PathBuf,
    },
    Write(Vec<u8>),
    /// Types into the console and keeps the screen as it was until the shell's next prompt,
    /// so a `cd` Borg types neither flashes nor stays in the console's history.
    WriteConcealed(Vec<u8>),
    /// Move the idle shell into a folder without typing a command (see `ShellStatus::silent_cd`).
    /// `typed` is the hidden `cd` command line to send if that is not possible.
    ChangeShellDir {
        dir: PathBuf,
        typed: Vec<u8>,
    },
    ScrollConsole(Scroll),
    View(PathBuf),
    /// Reads a file for the built-in editor.
    OpenEditor(PathBuf),
    /// Writes the editor's text, unless the file is no longer `expected` and not `force`d.
    SaveFile {
        path: PathBuf,
        bytes: Vec<u8>,
        expected: crate::FileStamp,
        force: bool,
    },
    RemoteView {
        machine: String,
        session: String,
        path: PathBuf,
    },
    SaveConfig,
    SetLanguage(borg_i18n::Locale),
    StartJob {
        id: u64,
        spec: Spec,
    },
    StartRemoteCopy {
        id: u64,
        from: Option<crate::RemoteMount>,
        to: Option<crate::RemoteMount>,
        sources: Vec<PathBuf>,
        target: PathBuf,
    },
    /// A copy job between a container and a host folder; `inbound` copies into the container.
    StartDockerCopy {
        id: u64,
        container: String,
        inbound: bool,
        sources: Vec<PathBuf>,
        target: PathBuf,
    },
    JobControl {
        id: u64,
        command: JobCommand,
    },
    Peek {
        aside: Aside,
        path: PathBuf,
        size: (usize, usize),
    },
    GitScan {
        side: Side,
        generation: u64,
        dir: PathBuf,
    },
    GitTool {
        id: u64,
        tool: borg_git::Tool,
        dir: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// Reads or changes Time Machine exclusions (macOS).
    Backup(crate::BackupRequest),
    Quit,
}
