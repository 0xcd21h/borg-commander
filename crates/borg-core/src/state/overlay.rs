//! The windows over the panels: menus, prompts, questions and the rest.

use crate::{About, Find, JobList, LineInput, Picker, Question};
use std::{path::PathBuf, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuCursor {
    pub menu: usize,
    pub item: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPurpose {
    /// What to put in place of the text a contents search found, in `files`.
    ReplaceInFiles {
        query: Box<borg_view::Query>,
        files: Vec<PathBuf>,
    },
    Search {
        root: PathBuf,
    },
    Collection,
    RemoteName {
        browser: Box<crate::RemoteBrowser>,
        identity: String,
        session: Option<String>,
        original: String,
    },
    ResultsFilter,
    /// A path to exclude from Time Machine, as a fixed path or marked on the item.
    BackupAdd {
        fixed: bool,
    },
    Select,
    Unselect,
    Mkdir,
    Compress {
        sources: Vec<String>,
    },
    Copy {
        sources: Vec<PathBuf>,
        optimize_hdd: bool,
    },
    Move {
        sources: Vec<PathBuf>,
    },
    RemoteCopy {
        sources: Vec<PathBuf>,
        from: Option<crate::RemoteMount>,
        to: Option<crate::RemoteMount>,
    },
    /// Copy between a container's filesystem and a host folder.
    DockerCopy {
        sources: Vec<PathBuf>,
    },
    /// Copy out of the archive the active panel shows, or move out of it or within it.
    Unpack {
        sources: Vec<PathBuf>,
        moving: bool,
    },
    /// Copy or move into the archive the other panel shows.
    Pack {
        sources: Vec<PathBuf>,
        moving: bool,
    },
    /// The password for the archive the panel on `side` opens.
    ArchivePassword {
        side: crate::Side,
    },
    ComputerKeyPassword,
}

impl PromptPurpose {
    /// The checkbox below the field, as its label and whether it is checked.
    pub fn option(&self) -> Option<(&'static str, bool)> {
        match self {
            Self::Copy { optimize_hdd, .. } => Some((borg_i18n::tr!(optimize_copying_for_hard_drives), *optimize_hdd)),
            Self::BackupAdd { fixed } => Some((borg_i18n::tr!(backup_add_fixed), *fixed)),
            _ => None,
        }
    }

    /// Whether the field hides what is typed.
    pub fn masked(&self) -> bool {
        matches!(self, Self::ArchivePassword { .. } | Self::ComputerKeyPassword)
    }

    pub(crate) fn toggle_option(&mut self) {
        match self {
            Self::Copy { optimize_hdd: checked, .. } | Self::BackupAdd { fixed: checked } => *checked = !*checked,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: &'static str,
    pub label: String,
    pub input: LineInput,
    pub purpose: PromptPurpose,
    pub option_focused: bool,
    /// When a key last changed the text, which a password field flares at.
    pub typed: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPurpose {
    RemoteSessionDelete {
        machine: String,
        session: String,
        browser: Box<crate::RemoteBrowser>,
    },
    RemoteHostForget {
        machine: String,
        browser: Box<crate::RemoteBrowser>,
    },
    RemoteHostUpdate {
        machine: String,
        browser: Box<crate::RemoteBrowser>,
    },
    Delete {
        sources: Vec<PathBuf>,
        /// The size and the programs using the files, for files on this computer.
        check: Option<crate::DeleteCheck>,
    },
    /// Removing the leftovers the Borg's files window listed.
    CleanStorage {
        paths: Vec<PathBuf>,
        window: Box<crate::StorageWindow>,
    },
    Quit,
    ShellExit {
        token: String,
    },
    /// `exit` typed in a remote shell: Yes lets that shell end, No keeps it.
    RemoteShellExit {
        token: String,
    },
    Docker {
        action: borg_docker::Action,
        ids: Vec<String>,
    },
    /// Replacing text in files, which keeps no copy of the old text.
    ReplaceInFiles(Box<crate::ReplaceRequest>),
    /// Killing the process an alert is about, which cannot be taken back.
    AlertKill {
        alert: u64,
        process: crate::ProcessKey,
    },
    /// Downloading the image of the helper container that shows a volume's files.
    VolumeHelper {
        volume: String,
    },
    /// A capture the system refused: Yes runs it again as administrator.
    Capture {
        request: Box<crate::CaptureRequest>,
        view_filter: String,
    },
    Scan(Box<crate::SubnetScan>),
    /// Yes drops a host that is gone, or that only listening heard, from what Borg keeps.
    ForgetNeighbor(std::net::IpAddr),
    /// A listen the system refused: Yes runs it again as administrator.
    Listen(Box<crate::ListenRequest>),
    /// Look for networks behind the router internet traffic goes to.
    Discover(Box<crate::Discovery>),
    /// Forget a remembered routed network and the hosts behind it.
    ForgetRoute(std::net::Ipv4Addr, u8),
    /// Yes starts the port scan as a job.
    ScanPorts(Box<crate::PortScanRequest>),
    /// Yes writes the report over the file that is there, and no goes back to the save dialog.
    ReplaceReport(Box<crate::ReportForm>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: &'static str,
    pub message: String,
    pub purpose: ConfirmPurpose,
    pub accept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Game2048,
    Collections(crate::CollectionPicker),
    /// The hotlist and the recent folders.
    Folders(crate::FoldersDialog),
    Search(crate::SearchForm),
    /// What searches leave out, opened from the search dialog.
    Exclusions(crate::ExclusionEditor),
    About(About),
    Find(Find),
    Question(Question),
    Jobs(JobList),
    Themes(Picker),
    Languages(Picker),
    Settings {
        selected: usize,
    },
    Plugins {
        selected: usize,
    },
    Menu(MenuCursor),
    Prompt(Prompt),
    Confirm(Confirm),
    /// The log of what agents did; `scroll` is `None` while it follows the newest entries.
    AgentLog {
        scroll: Option<usize>,
    },
    /// How to connect Claude to this borg.
    Connect(crate::agent::Connect),
    /// Paired Borg machines and their named sessions.
    Remotes(crate::RemoteBrowser),
    /// Time Machine exclusions; the window's lists live in `AppState::backup`.
    TimeMachine,
    /// The signals and priority changes for a process in the monitor.
    ProcessControl(crate::ControlDialog),
    /// What can be done with the item a right-click or Shift-F10 opened it on.
    ContextMenu(crate::ContextMenu),
    /// What the entropy scan of every process reads, or its last hits.
    EntropyScan(crate::EntropyDialog),
    /// What to capture from the network screen, where and for how long.
    Capture(crate::CaptureDialog),
    /// Where to listen for neighbors, for how long and as whom.
    Listen(crate::ListenForm),
    /// The command that installs a missing program, before it runs in the shell.
    Install(crate::InstallOffer),
    /// The command that installs or starts the background service, before it runs in the shell.
    Service(crate::ServiceOffer),
    /// A newer Borg and the command that installs it, before it runs in the shell.
    Update(crate::UpdateOffer),
    /// Where Borg keeps its own files, how much each place holds, and the leftovers.
    Storage(crate::StorageWindow),
    /// Which host to scan for open ports, which ports and how fast.
    PortScan(Box<crate::PortScanForm>),
    /// Which network the subnet scan covers.
    ScanNetwork(Box<crate::ScanForm>),
    /// A port scan finished in the background.
    PortScanDone(Box<crate::PortScanRecord>),
    /// The open ports a scan found.
    PortScanResult(Box<crate::PortScanView>),
    /// Where to save a report, and in which format.
    SaveReport(Box<crate::ReportForm>),
}
