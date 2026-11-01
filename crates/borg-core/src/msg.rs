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
    /// Files a contents search found, each with its first matching line.
    Found {
        side: Side,
        generation: u64,
        found: Vec<(Entry, crate::LineMatch)>,
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
    /// A file read for the editor in a slot, or why it was not.
    EditorOpened(crate::EditorSlot, Result<crate::EditorFile, crate::OpenError>),
    /// The save of the editor in a slot landed, with the file's new stamp, or did not.
    EditorSaved(crate::EditorSlot, Result<crate::FileStamp, crate::SaveError>),
    /// What the viewer's search `id` found, or why it could not look.
    FoundInViewer {
        id: u64,
        found: Result<crate::FoundInViewer, String>,
    },
    /// How far the viewer's search `id` got through a large file.
    SearchingViewer {
        id: u64,
        percent: u8,
    },
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
    /// Looks for `query` in the viewer's file in the background, stopping any search before.
    FindInViewer {
        id: u64,
        query: borg_view::Query,
        from: u64,
        direction: crate::Direction,
        /// Count the matches too, which reads all of the file.
        count: bool,
    },
    StopFindInViewer,
    /// Reads a file for the built-in editor in `slot`.
    OpenEditor {
        path: PathBuf,
        slot: crate::EditorSlot,
    },
    /// Writes the text of the editor in `slot`, unless the file is no longer `expected` and not
    /// `force`d.
    SaveFile {
        path: PathBuf,
        bytes: Vec<u8>,
        expected: crate::FileStamp,
        force: bool,
        slot: crate::EditorSlot,
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
        at: Option<crate::FirstMatch>,
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
