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
    /// Killing the process an alert is about, which cannot be taken back.
    AlertKill {
        alert: u64,
        process: crate::ProcessKey,
    },
    /// A capture the system refused: Yes runs it again as administrator.
    Capture {
        request: Box<crate::CaptureRequest>,
        view_filter: String,
    },
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
    /// What the entropy scan of every process reads, or its last hits.
    EntropyScan(crate::EntropyDialog),
    /// What to capture from the network screen, where and for how long.
    Capture(crate::CaptureDialog),
    /// The command that installs a missing program, before it runs in the shell.
    Install(crate::InstallOffer),
    /// The command that installs or starts the background service, before it runs in the shell.
    Service(crate::ServiceOffer),
    /// Where Borg keeps its own files, how much each place holds, and the leftovers.
    Storage(crate::StorageWindow),
}
