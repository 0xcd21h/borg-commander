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
    /// What a run of the entropy scan found since its last message; `done` with its last.
    EntropyScan {
        id: u64,
        found: Result<crate::EntropyReport, String>,
        done: bool,
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
    Network {
        id: u64,
        result: Result<crate::NetworkSample, String>,
    },
    StartupItems {
        id: u64,
        result: Result<crate::StartupReport, String>,
    },
    Alerts {
        id: u64,
        result: Result<crate::AlertsReply, String>,
    },
    /// The alert log changed under Borg: the watcher wrote one, or another Borg read them.
    AlertsChanged(Box<crate::AlertLog>),
    /// Names for addresses the network screen asked about; `None` where there is none.
    HostNames(Vec<(std::net::IpAddr, Option<String>)>),
    /// How far the running capture got.
    CaptureProgress {
        id: u64,
        progress: crate::CaptureProgress,
    },
    /// The running capture reached a limit, and is being saved.
    CaptureLimited {
        id: u64,
        limit: crate::CaptureLimit,
    },
    /// The running capture keeps the TLS key log at `path`, empty for now, and Borg's proxy
    /// listens as `proxy` says when the capture sends HTTPS through it.
    CaptureKeys {
        id: u64,
        path: PathBuf,
        proxy: Option<crate::ProxyEndpoint>,
    },
    /// Borg's proxy opened a connection for a program during the running capture.
    CaptureProxied {
        id: u64,
        proxied: crate::Proxied,
    },
    /// A capture ended: the file it was saved to and what it caught, or why it has none.
    CaptureDone {
        id: u64,
        result: Result<(PathBuf, crate::Capture), crate::CaptureFailure>,
    },
    /// A capture file opened from a panel, read.
    CaptureOpened {
        path: PathBuf,
        result: Result<crate::Capture, String>,
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
    /// What the borg of a host session a panel here shows said about the asks waiting there.
    HostAsks {
        machine: String,
        session: String,
        event: crate::HostEvent,
    },
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
    RemoteHostUpdated {
        machine: String,
        result: Result<crate::HostUpdate, String>,
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
    /// A right-click at a cell: `pick` puts the cursor on the row under it, and the context
    /// menu opens there.
    ContextMenu {
        pick: crate::Pointer,
        column: u16,
        row: u16,
    },
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
    /// Files a rules search found, each with the rules it matched, worst first.
    RuleHits {
        side: Side,
        generation: u64,
        found: Vec<(Entry, Vec<borg_rules::RuleMatch>)>,
    },
    /// Programs a search for packed ones found, each with the entropy of its code.
    PackedHits {
        side: Side,
        generation: u64,
        found: Vec<(Entry, borg_view::entropy::Tally)>,
    },
    Listed {
        side: Side,
        generation: u64,
        error: Option<String>,
    },
    /// How much of an archive its first listing has read, of `total` bytes.
    ArchiveReading {
        side: Side,
        generation: u64,
        read: u64,
        total: u64,
    },
    /// An archive opened for listing; `left_out` entries have names or kinds Borg does not unpack.
    ArchiveOpened {
        side: Side,
        generation: u64,
        format: borg_archive::Format,
        left_out: usize,
    },
    /// An archive that only an installed program reads, and none is; `installer` installs one.
    ArchiveNeedsTool {
        side: Side,
        generation: u64,
        installer: Option<crate::Installer>,
    },
    ComputerKeyLocked {
        wrong: bool,
    },
    /// An archive that needs a password, or a better one than it was given.
    ArchiveLocked {
        side: Side,
        generation: u64,
        wrong: bool,
    },
    Observe(ShellView),
    ShellOutput(ShellView),
    /// The names of `dir`'s subfolders, none when it could not be read.
    Subfolders {
        dir: PathBuf,
        names: Vec<String>,
    },
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
    /// How many answers keep Borg from asking before long jobs, after a job may have kept one.
    KeptAnswers(usize),
    /// The editor in `slot` opened a file unpacked from an archive, which saves write back.
    ArchiveEditorOpened {
        slot: crate::EditorSlot,
        packed: crate::PackedFile,
        result: Result<crate::EditorFile, crate::OpenError>,
    },
    /// The save of the editor in a slot landed, with the file's new stamp, or did not.
    EditorSaved(crate::EditorSlot, Result<crate::FileStamp, crate::SaveError>),
    /// What the search `id` through the viewer's or the quick view's text found, or why it
    /// could not look.
    FoundInPager {
        id: u64,
        slot: crate::PagerSlot,
        found: Result<crate::FoundInPager, String>,
    },
    /// How far the search `id` got through a large file.
    SearchingPager {
        id: u64,
        slot: crate::PagerSlot,
        percent: u8,
    },
    /// The entropy of the viewer's source that `id` asked for, or why it could not be read.
    ViewerProfiled {
        id: u64,
        profile: Result<borg_view::entropy::Profile, String>,
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
    /// What Borg keeps where, for the Borg's files window.
    Storage(crate::StorageReport),
    /// How the background service stands, after the Remote menu asked.
    Service(crate::ServiceCheck),
    /// The service runs Borg `version`, which the program file holds now: the session moves
    /// to it once nothing here would be lost.
    Relaunch {
        version: String,
    },
    /// A release newer than the running Borg, which the daily check found.
    UpdateAvailable(crate::UpdateOffer),
    /// What counting or checking the files of the delete dialog `id` found.
    DeleteChecked {
        id: u64,
        found: crate::DeleteFound,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    CancelShellExit {
        token: String,
    },
    /// Counts what deleting `sources` removes and finds the programs that have them open.
    CheckDelete {
        id: u64,
        sources: Vec<PathBuf>,
    },
    MeasureSizes {
        id: u64,
        root: PathBuf,
    },
    CancelMeasure,
    CancelMemorySearch,
    /// Reads the memory of `processes` for the entropy scan, as administrator with `admin`.
    EntropyScan {
        id: u64,
        scope: crate::EntropyScope,
        processes: Vec<crate::ProcessKey>,
        admin: bool,
    },
    /// Stops the entropy scan, which reports the processes it did not reach.
    StopEntropyScan,
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
    /// Samples the machine's sockets and interfaces for the network screen.
    Network {
        id: u64,
    },
    /// Reads the places programs start from for the startup items screen.
    StartupItems {
        id: u64,
    },
    /// Reads the alert log, or does what the Alerts page asks of a process.
    Alerts {
        id: u64,
        request: crate::AlertsRequest,
    },
    /// Looks up the names of addresses, in the background.
    ResolveHosts(Vec<std::net::IpAddr>),
    /// Runs `tcpdump` for the network screen's capture.
    Capture {
        id: u64,
        request: crate::CaptureRequest,
    },
    /// Stops the running capture, which then saves what it caught.
    StopCapture {
        id: u64,
    },
    /// Reads a pcap or pcapng file for the traffic viewer.
    OpenCapture(PathBuf),
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
    /// Updates a paired host to the latest Borg and waits for it to come back.
    UpdateRemoteHost {
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
    /// The user's answer to an ask that waits in a host session a panel here shows.
    AnswerHost {
        machine: String,
        session: String,
        answer: crate::HostAnswer,
    },
    /// Turns off Approve all in a host session a panel here shows.
    StopHostApproveAll {
        machine: String,
        session: String,
    },
    /// Puts text on the clipboard of the computer the user sits at, through the terminal.
    Copy(String),
    /// Shows text alone in the terminal, outside borg's screen, for the user to select and
    /// copy where the terminal does not take the clipboard; a key brings borg back.
    ShowPlain(String),
    /// Makes a pairing link for borg cloud, for the Connect dialog.
    StartPairing,
    UnlockComputerKey(crate::Password),
    /// Saves the relay borg cloud goes through, empty for none, and makes a pairing link there.
    SetRelay(String),
    /// Forgets every computer paired through borg cloud, and ends their sessions.
    ForgetComputers,
    /// Adds an entry to the log of what agents did that the session keeps on disk.
    SaveAgentNote(String),
    /// Removes the session's saved log of what agents did.
    ForgetAgentLog,
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
    /// Writes the recent folders, or removes the file once there are none.
    SaveFolders,
    /// Saves the hotlist in the configuration.
    SetHotlist(Vec<PathBuf>),
    /// Reads the names of a folder's subfolders for the picker above the command line.
    ReadSubfolders(PathBuf),
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
    /// Loads `path` for the viewer, on the match a contents search found at `at`.
    View {
        path: PathBuf,
        at: Option<crate::FirstMatch>,
    },
    /// Looks for `query` in the file the viewer or the quick view shows, in the background,
    /// stopping any search before through the same one.
    FindInPager {
        id: u64,
        slot: crate::PagerSlot,
        query: borg_view::Query,
        from: u64,
        direction: crate::Direction,
        /// Count the matches too, which reads all of the file.
        count: bool,
    },
    StopFindInPager(crate::PagerSlot),
    /// Reads the entropy of the viewer's source in the background, for the strip beside its
    /// hex rows, stopping any read before.
    ProfileViewer {
        id: u64,
    },
    StopProfileViewer,
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
    /// A job that unpacks entries of an archive.
    StartUnpack {
        id: u64,
        request: crate::UnpackRequest,
    },
    /// A job that replaces text in files.
    StartReplace {
        id: u64,
        request: crate::ReplaceRequest,
    },
    /// A job that changes an archive.
    StartRewrite {
        id: u64,
        request: crate::RewriteRequest,
    },
    /// Forgets the answers that keep Borg from asking before long jobs.
    AskAgain,
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
    /// Measures Borg's places and finds the leftovers.
    ScanStorage,
    /// Removes those of these leftovers that are still left over, then measures again.
    CleanStorage(Vec<PathBuf>),
    /// Looks at how the background service stands on this machine.
    CheckService,
    /// Starts the newer Borg in place of this one, handing it the shell and the session.
    Relaunch,
    /// Remembers that you skipped this release, so the daily check offers the next one only.
    SkipUpdate(String),
    /// Turns the daily look for a new release off in the config.
    StopUpdateChecks,
    Quit,
}
