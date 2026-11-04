//! What counts as unusual when a process starts. Each rule looks at one started process, and
//! at what started recently under the same parent, and says in plain words why it fired.

use borg_i18n::tr;
use borg_rules::Level;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
    time::{Duration, SystemTime},
};

use super::{Baseline, StartedProcess, StreamKind};

/// Shells, whose start outside a terminal is worth knowing about.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "ash", "fish", "csh", "tcsh", "busybox"];
/// Programs that run on their own for a long time and should not be starting shells: web
/// servers, application runtimes, databases and the like.
const SERVICES: &[&str] = &[
    "nginx",
    "apache2",
    "httpd",
    "caddy",
    "lighttpd",
    "haproxy",
    "php-fpm",
    "uwsgi",
    "gunicorn",
    "unicorn",
    "puma",
    "node",
    "deno",
    "bun",
    "java",
    "tomcat",
    "dotnet",
    "postgres",
    "mysqld",
    "mariadbd",
    "mongod",
    "redis-server",
    "memcached",
    "elasticsearch",
    "rabbitmq-server",
    "smbd",
    "vsftpd",
    "dovecot",
    "exim4",
    "postfix",
];
/// Programs that run a script given to them, so the name a process calls itself may be the
/// script's rather than the program's.
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ksh", "fish", "busybox", "python", "python2", "python3", "perl", "ruby", "node",
    "php", "lua", "tclsh", "awk", "env",
];
/// Folders anyone may write to, where a program is seldom installed on purpose.
const TEMP_FOLDERS: &[&str] = &["/tmp", "/var/tmp", "/dev/shm", "/run/shm"];
/// Commands that look around the machine: a few in a row from a process without a terminal is
/// what an intruder's first minutes look like.
const DISCOVERY: &[&str] = &[
    "whoami",
    "id",
    "groups",
    "uname",
    "hostname",
    "hostnamectl",
    "w",
    "who",
    "last",
    "lastlog",
    "ps",
    "top",
    "netstat",
    "ss",
    "ifconfig",
    "ip",
    "arp",
    "route",
    "lsof",
    "env",
    "printenv",
    "crontab",
    "lscpu",
    "lsb_release",
    "mount",
    "df",
    "dmidecode",
    "sudo",
];
/// How long a run of discovery commands may take to count as one.
const DISCOVERY_WINDOW: Duration = Duration::from_secs(20);
/// Different discovery commands from one parent that make a run.
const DISCOVERY_RUN: usize = 4;
/// Recent starts kept for the discovery rule.
const HISTORY_LIMIT: usize = 512;
/// Bytes of a program's name the system keeps as the name a process calls itself.
const COMM_LIMIT: usize = 15;
/// Preload variables, which put a library of someone's choosing into a program.
pub const PRELOAD_VARIABLES: &[&str] = &["LD_PRELOAD", "LD_AUDIT", "DYLD_INSERT_LIBRARIES"];

/// A rule the watcher applies to every process that starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WatchRule {
    ShellFromService,
    ShellWithoutTerminal,
    ShellOnSocket,
    DiscoveryRun,
    ProgramInTempFolder,
    ProgramDeleted,
    ProgramWithoutFile,
    PreloadSet,
    NameDiffersFromProgram,
    ParentNeverStartedIt,
}

impl WatchRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::ShellFromService => tr!(watch_rule_shell_from_service),
            Self::ShellWithoutTerminal => tr!(watch_rule_shell_without_terminal),
            Self::ShellOnSocket => tr!(watch_rule_shell_on_socket),
            Self::DiscoveryRun => tr!(watch_rule_discovery_run),
            Self::ProgramInTempFolder => tr!(watch_rule_program_in_temp_folder),
            Self::ProgramDeleted => tr!(watch_rule_program_deleted),
            Self::ProgramWithoutFile => tr!(watch_rule_program_without_file),
            Self::PreloadSet => tr!(watch_rule_preload_set),
            Self::NameDiffersFromProgram => tr!(watch_rule_name_differs),
            Self::ParentNeverStartedIt => tr!(watch_rule_parent_never_started_it),
        }
    }

    /// The rule's short name, for the filter, the configuration and agents.
    pub fn code(self) -> &'static str {
        match self {
            Self::ShellFromService => "shell-from-service",
            Self::ShellWithoutTerminal => "shell-without-terminal",
            Self::ShellOnSocket => "shell-on-socket",
            Self::DiscoveryRun => "discovery-run",
            Self::ProgramInTempFolder => "program-in-temp-folder",
            Self::ProgramDeleted => "program-deleted",
            Self::ProgramWithoutFile => "program-without-file",
            Self::PreloadSet => "preload-set",
            Self::NameDiffersFromProgram => "name-differs-from-program",
            Self::ParentNeverStartedIt => "parent-never-started-it",
        }
    }

    pub fn level(self) -> Level {
        match self {
            Self::ShellOnSocket | Self::ProgramDeleted | Self::ProgramWithoutFile | Self::PreloadSet => Level::Alert,
            Self::ShellFromService
            | Self::ShellWithoutTerminal
            | Self::DiscoveryRun
            | Self::ProgramInTempFolder
            | Self::NameDiffersFromProgram => Level::Warning,
            Self::ParentNeverStartedIt => Level::Notice,
        }
    }

    /// Whether the baseline can quiet it: a start this machine does every day is normal, while
    /// a shell on a socket or a preloaded library never becomes normal by happening often.
    pub fn learnable(self) -> bool {
        match self {
            Self::ShellFromService
            | Self::ShellWithoutTerminal
            | Self::DiscoveryRun
            | Self::NameDiffersFromProgram
            | Self::ParentNeverStartedIt => true,
            Self::ShellOnSocket
            | Self::ProgramInTempFolder
            | Self::ProgramDeleted
            | Self::ProgramWithoutFile
            | Self::PreloadSet => false,
        }
    }
}

/// Why a rule fired, with the words its sentence needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Finding {
    ShellFromService { shell: String, parent: String },
    ShellWithoutTerminal { shell: String, parent: String },
    ShellOnSocket { shell: String },
    DiscoveryRun { parent: String, commands: usize, seconds: u64 },
    ProgramInTempFolder { program: String, folder: String },
    ProgramDeleted { program: String },
    ProgramWithoutFile { name: String },
    PreloadSet { program: String, variable: String, value: String },
    NameDiffersFromProgram { name: String, program: String },
    ParentNeverStartedIt { parent: String, program: String },
}

impl Finding {
    pub fn rule(&self) -> WatchRule {
        match self {
            Self::ShellFromService { .. } => WatchRule::ShellFromService,
            Self::ShellWithoutTerminal { .. } => WatchRule::ShellWithoutTerminal,
            Self::ShellOnSocket { .. } => WatchRule::ShellOnSocket,
            Self::DiscoveryRun { .. } => WatchRule::DiscoveryRun,
            Self::ProgramInTempFolder { .. } => WatchRule::ProgramInTempFolder,
            Self::ProgramDeleted { .. } => WatchRule::ProgramDeleted,
            Self::ProgramWithoutFile { .. } => WatchRule::ProgramWithoutFile,
            Self::PreloadSet { .. } => WatchRule::PreloadSet,
            Self::NameDiffersFromProgram { .. } => WatchRule::NameDiffersFromProgram,
            Self::ParentNeverStartedIt { .. } => WatchRule::ParentNeverStartedIt,
        }
    }

    pub fn level(&self) -> Level {
        self.rule().level()
    }

    /// Why the rule fired, in one sentence.
    pub fn reason(&self) -> String {
        match self {
            Self::ShellFromService { shell, parent } => {
                tr!(watch_why_shell_from_service, shell = shell, parent = parent)
            }
            Self::ShellWithoutTerminal { shell, parent } => {
                tr!(watch_why_shell_without_terminal, shell = shell, parent = parent)
            }
            Self::ShellOnSocket { shell } => tr!(watch_why_shell_on_socket, shell = shell),
            Self::DiscoveryRun { parent, commands, seconds } => {
                tr!(watch_why_discovery_run, parent = parent, commands = commands, seconds = seconds)
            }
            Self::ProgramInTempFolder { program, folder } => {
                tr!(watch_why_program_in_temp_folder, program = program, folder = folder)
            }
            Self::ProgramDeleted { program } => tr!(watch_why_program_deleted, program = program),
            Self::ProgramWithoutFile { name } => tr!(watch_why_program_without_file, name = name),
            Self::PreloadSet { program, variable, value } => {
                tr!(watch_why_preload_set, program = program, variable = variable, value = value)
            }
            Self::NameDiffersFromProgram { name, program } => {
                tr!(watch_why_name_differs, name = name, program = program)
            }
            Self::ParentNeverStartedIt { parent, program } => {
                tr!(watch_why_parent_never_started_it, parent = parent, program = program)
            }
        }
    }
}

/// The starts the watcher remembers, so a run of commands can be seen as one.
#[derive(Debug, Clone, Default)]
pub struct History {
    starts: VecDeque<Start>,
}

#[derive(Debug, Clone)]
struct Start {
    parent: Option<u32>,
    program: String,
    at: SystemTime,
}

impl History {
    pub fn note(&mut self, process: &StartedProcess) {
        self.starts.push_back(Start {
            parent: process.parent().map(|parent| parent.pid),
            program: process.program_name().to_owned(),
            at: process.at,
        });
        while self.starts.len() > HISTORY_LIMIT {
            self.starts.pop_front();
        }
    }

    /// How many different discovery commands one parent started within the window ending at
    /// `at`, counting `current`, which has just started and is not in the history yet.
    fn discovery_run(&self, parent: u32, current: &str, at: SystemTime) -> usize {
        let earlier: BTreeSet<&str> = self
            .starts
            .iter()
            .filter(|start| start.parent == Some(parent))
            .filter(|start| at.duration_since(start.at).is_ok_and(|age| age <= DISCOVERY_WINDOW))
            .filter(|start| DISCOVERY.contains(&start.program.as_str()))
            .map(|start| start.program.as_str())
            .collect();
        let counts = DISCOVERY.contains(&current) && !earlier.contains(current);
        earlier.len() + usize::from(counts)
    }
}

fn is_shell(name: &str) -> bool {
    SHELLS.contains(&name)
}

/// The temporary folder a program lies in, if any.
fn temp_folder(program: Option<&Path>) -> Option<&'static str> {
    let program = program?;
    TEMP_FOLDERS.iter().copied().find(|folder| program.starts_with(folder))
}

/// Whether the name a process calls itself can differ from its program's for an ordinary
/// reason: a program that runs a script is named after the script.
fn runs_scripts(program_name: &str) -> bool {
    INTERPRETERS.contains(&program_name)
}

/// What a process calls itself, cut to what the system keeps.
fn shortened(name: &str) -> &str {
    match name.char_indices().nth(COMM_LIMIT) {
        Some((end, _)) => &name[..end],
        None => name,
    }
}

/// Every rule that fires on `process`, worst level first. `history` must already hold the
/// starts before this one.
pub fn examine(process: &StartedProcess, baseline: &Baseline, history: &History, now: SystemTime) -> Vec<Finding> {
    let known = baseline.knows(process);
    let mut findings: Vec<Finding> = Vec::new();
    let program = process.program_name().to_owned();
    let parent_name = process.parent().map(|parent| parent.program_name().to_owned());
    if is_shell(&program) {
        if process.streams().contains(&StreamKind::Socket) {
            findings.push(Finding::ShellOnSocket { shell: program.clone() });
        }
        if let Some(parent) = process.parent() {
            let parent_program = parent.program_name().to_owned();
            if SERVICES.contains(&parent_program.as_str()) {
                findings.push(Finding::ShellFromService { shell: program.clone(), parent: parent_program });
            } else if !parent.terminal && !process.has_terminal() {
                findings.push(Finding::ShellWithoutTerminal { shell: program.clone(), parent: parent_program });
            }
        }
    }
    if let Some(parent) = process.parent().filter(|parent| !parent.terminal) {
        let commands = history.discovery_run(parent.pid, &program, now);
        if commands == DISCOVERY_RUN {
            findings.push(Finding::DiscoveryRun {
                parent: parent.program_name().to_owned(),
                commands,
                seconds: DISCOVERY_WINDOW.as_secs(),
            });
        }
    }
    if let Some(folder) = temp_folder(process.program.as_deref()) {
        findings.push(Finding::ProgramInTempFolder { program: program.clone(), folder: folder.to_owned() });
    }
    if process.program_deleted {
        findings.push(Finding::ProgramDeleted { program: program.clone() });
    } else if process.program.is_none() {
        findings.push(Finding::ProgramWithoutFile { name: process.name.clone() });
    }
    if let Some((variable, value)) = process.preloads.first() {
        findings.push(Finding::PreloadSet {
            program: program.clone(),
            variable: variable.clone(),
            value: value.clone(),
        });
    }
    if process.program.is_some()
        && process.link.is_none()
        && !runs_scripts(&program)
        && shortened(&program) != shortened(&process.name)
    {
        findings.push(Finding::NameDiffersFromProgram { name: process.name.clone(), program: program.clone() });
    }
    if !known
        && !baseline.learning(now)
        && let Some(parent) = parent_name
    {
        findings.push(Finding::ParentNeverStartedIt { parent, program });
    }
    findings.retain(|finding| !(known && finding.rule().learnable()));
    findings.sort_by_key(|finding| (finding.level(), finding.rule()));
    findings
}
