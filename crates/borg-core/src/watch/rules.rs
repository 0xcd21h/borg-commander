//! What counts as unusual when a process starts. Each rule looks at one started process, and
//! at what the same program ran lately, and says in plain words why it fired.

use borg_i18n::{Count, tr};
use borg_rules::Level;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, SystemTime},
};

use super::{
    Ancestor, Baseline, StartedProcess, StreamKind,
    baseline::written,
    response::{logged_in, opened_by_program},
};

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
/// Programs that carry a network connection to another program, such as a shell on a terminal
/// they open for it.
const RELAYS: &[&str] = &["socat", "ncat", "nc", "netcat", "nc.openbsd", "nc.traditional"];
/// Programs that run a script given to them, so the name a process calls itself may be the
/// script's rather than the program's.
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ksh", "fish", "busybox", "python", "python2", "python3", "perl", "ruby", "node",
    "php", "lua", "tclsh", "awk", "env",
];
/// Folders anyone may write to, where a program is seldom installed on purpose.
const TEMP_FOLDERS: &[&str] = &["/tmp", "/var/tmp", "/dev/shm", "/run/shm"];
/// Commands that look around the machine: a few from one program, with nobody logged in, is
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
/// How far apart the commands of one run may lie. Somebody looking around by hand takes
/// minutes between commands, so a run may span an hour.
const DISCOVERY_WINDOW: Duration = Duration::from_secs(60 * 60);
/// Different discovery commands from one program that make a run.
const DISCOVERY_RUN: usize = 4;
/// Programs whose recent discovery commands Borg keeps, the ones active most lately.
const SURVEYORS_KEPT: usize = 256;
const MINUTE_SECONDS: u64 = 60;
const HOUR_SECONDS: u64 = 60 * MINUTE_SECONDS;
/// Bytes of a program's name the system keeps as the name a process calls itself.
const COMM_LIMIT: usize = 15;
/// Preload variables, which put a library of someone's choosing into a program.
pub const PRELOAD_VARIABLES: &[&str] = &["LD_PRELOAD", "LD_AUDIT", "DYLD_INSERT_LIBRARIES"];

/// A rule the watcher applies to every process that starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WatchRule {
    ShellFromService,
    ShellWithoutTerminal,
    ShellOnProgramTerminal,
    ShellOnSocket,
    RemoteShellOnTerminal,
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
            Self::ShellOnProgramTerminal => tr!(watch_rule_shell_on_program_terminal),
            Self::ShellOnSocket => tr!(watch_rule_shell_on_socket),
            Self::RemoteShellOnTerminal => tr!(watch_rule_remote_shell_on_terminal),
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
            Self::ShellOnProgramTerminal => "shell-on-program-terminal",
            Self::ShellOnSocket => "shell-on-socket",
            Self::RemoteShellOnTerminal => "remote-shell-on-terminal",
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
            Self::ShellOnSocket
            | Self::RemoteShellOnTerminal
            | Self::ProgramDeleted
            | Self::ProgramWithoutFile
            | Self::PreloadSet => Level::Alert,
            Self::ShellFromService
            | Self::ShellWithoutTerminal
            | Self::ShellOnProgramTerminal
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
            | Self::ShellOnProgramTerminal
            | Self::DiscoveryRun
            | Self::NameDiffersFromProgram
            | Self::ParentNeverStartedIt => true,
            Self::ShellOnSocket
            | Self::RemoteShellOnTerminal
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
    ShellOnProgramTerminal { shell: String, opener: String },
    ShellOnSocket { shell: String },
    RemoteShellOnTerminal { shell: String, opener: String },
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
            Self::ShellOnProgramTerminal { .. } => WatchRule::ShellOnProgramTerminal,
            Self::ShellOnSocket { .. } => WatchRule::ShellOnSocket,
            Self::RemoteShellOnTerminal { .. } => WatchRule::RemoteShellOnTerminal,
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
            Self::ShellOnProgramTerminal { shell, opener } => {
                tr!(watch_why_shell_on_program_terminal, shell = shell, opener = opener)
            }
            Self::ShellOnSocket { shell } => tr!(watch_why_shell_on_socket, shell = shell),
            Self::RemoteShellOnTerminal { shell, opener } => {
                tr!(watch_why_remote_shell_on_terminal, shell = shell, opener = opener)
            }
            Self::DiscoveryRun { parent, commands, seconds } => {
                tr!(watch_why_discovery_run, parent = parent, commands = commands, time = spoken(*seconds))
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

/// A time span in words, in the largest unit it fills whole.
fn spoken(seconds: u64) -> String {
    let (count, unit) = if seconds >= HOUR_SECONDS && seconds.is_multiple_of(HOUR_SECONDS) {
        (seconds / HOUR_SECONDS, Count::Hour)
    } else if seconds >= MINUTE_SECONDS && seconds.is_multiple_of(MINUTE_SECONDS) {
        (seconds / MINUTE_SECONDS, Count::Minute)
    } else {
        (seconds, Count::Second)
    };
    borg_i18n::count(usize::try_from(count).unwrap_or(usize::MAX), unit)
}

/// Where a run of discovery commands comes from: the account they run as and the nearest
/// program above them that is not a shell, so the commands a web server runs through one
/// `sh -c` after another count together. A program that runs a script is told apart by its
/// script.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Surveyor {
    pub uid: u32,
    pub program: String,
}

impl Surveyor {
    pub fn of(process: &StartedProcess) -> Option<Self> {
        let origin = origin(process)?;
        let mut program = written(origin.program.as_deref(), &origin.name);
        if runs_scripts(origin.program_name())
            && let Some(script) = origin.command.split_whitespace().nth(1)
        {
            program = format!("{program} {script}");
        }
        Some(Self { uid: process.uid, program })
    }
}

/// The nearest process above this one that is not a shell, or the farthest one Borg knows.
fn origin(process: &StartedProcess) -> Option<&Ancestor> {
    process.ancestors.iter().find(|ancestor| !is_shell(ancestor.program_name())).or(process.ancestors.last())
}

/// Whether a program is one of the commands that look around the machine.
pub(super) fn looks_around(program: &str) -> bool {
    DISCOVERY.contains(&program)
}

/// The discovery commands each program ran lately with nobody logged in, so a run of them can
/// be seen as one however far apart they lie within the window.
#[derive(Debug, Clone, Default)]
pub struct History {
    surveys: BTreeMap<Surveyor, BTreeMap<String, SystemTime>>,
}

impl History {
    pub fn note(&mut self, process: &StartedProcess, first_person: u32) {
        let program = process.program_name();
        if !looks_around(program) || logged_in(process, first_person) {
            return;
        }
        let Some(surveyor) = Surveyor::of(process) else { return };
        let at = process.at;
        self.surveys.retain(|_, commands| {
            commands.retain(|_, seen| within_window(*seen, at));
            !commands.is_empty()
        });
        if !self.surveys.contains_key(&surveyor)
            && self.surveys.len() >= SURVEYORS_KEPT
            && let Some(stalest) = self
                .surveys
                .iter()
                .min_by_key(|(_, commands)| commands.values().max().copied())
                .map(|(key, _)| key.clone())
        {
            self.surveys.remove(&stalest);
        }
        self.surveys.entry(surveyor).or_default().insert(program.to_owned(), at);
    }

    /// The different discovery commands a program ran within the window ending at `at`,
    /// counting `current`, which has just started and is not in the history yet.
    fn run(&self, surveyor: &Surveyor, current: &str, at: SystemTime) -> BTreeSet<String> {
        let mut commands: BTreeSet<String> = self
            .surveys
            .get(surveyor)
            .into_iter()
            .flatten()
            .filter(|(_, seen)| within_window(**seen, at))
            .map(|(command, _)| command.clone())
            .collect();
        if looks_around(current) {
            commands.insert(current.to_owned());
        }
        commands
    }
}

fn within_window(seen: SystemTime, at: SystemTime) -> bool {
    at.duration_since(seen).is_ok_and(|age| age <= DISCOVERY_WINDOW)
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

/// Whether the program that opened a shell's terminal carries a network connection: its own
/// input, output or errors lead to the network, or it is a relay such as `socat`.
fn carries_network(opener: &Ancestor) -> bool {
    opener.streams.contains(&StreamKind::Socket) || RELAYS.contains(&opener.program_name())
}

/// Every rule that fires on `process`, worst level first. `history` must already hold the
/// starts before this one. A copy of its parent faces only the rules learning never quiets:
/// its parent's start has been judged already, while a forced library or a socket can be the
/// copy's own. `first_person` is the first account number the system gives a person, which
/// says whose terminal multiplexer counts as somebody logging in.
pub fn examine(
    process: &StartedProcess,
    baseline: &Baseline,
    history: &History,
    first_person: u32,
    now: SystemTime,
) -> Vec<Finding> {
    let known = baseline.knows(process) || process.copy_of_parent();
    let mut findings: Vec<Finding> = Vec::new();
    let program = process.program_name().to_owned();
    let parent_name = process.parent().map(|parent| parent.program_name().to_owned());
    if is_shell(&program) {
        if process.streams().contains(&StreamKind::Socket) {
            findings.push(Finding::ShellOnSocket { shell: program.clone() });
        }
        let opener = opened_by_program(process, first_person);
        let remote = opener.filter(|opener| carries_network(opener));
        if let Some(opener) = remote {
            findings.push(Finding::RemoteShellOnTerminal {
                shell: program.clone(),
                opener: opener.program_name().to_owned(),
            });
        }
        if let Some(parent) = process.parent() {
            let parent_program = parent.program_name().to_owned();
            if SERVICES.contains(&parent_program.as_str()) {
                findings.push(Finding::ShellFromService { shell: program.clone(), parent: parent_program });
            } else if !parent.terminal && !process.has_terminal() {
                findings.push(Finding::ShellWithoutTerminal { shell: program.clone(), parent: parent_program });
            } else if !parent.terminal
                && remote.is_none()
                && let Some(opener) = opener
            {
                findings.push(Finding::ShellOnProgramTerminal {
                    shell: program.clone(),
                    opener: opener.program_name().to_owned(),
                });
            }
        }
    }
    let surveyor = Surveyor::of(process).filter(|_| !logged_in(process, first_person));
    let mut surveyed = false;
    if let Some(surveyor) = &surveyor {
        let run = history.run(surveyor, &program, now);
        if run.len() == DISCOVERY_RUN {
            surveyed = process.copy_of_parent()
                || baseline.allows(process)
                || run.iter().all(|command| baseline.surveyed(surveyor, command));
            findings.push(Finding::DiscoveryRun {
                parent: origin(process).map(Ancestor::program_name).unwrap_or_default().to_owned(),
                commands: run.len(),
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
    findings.retain(|finding| match finding.rule() {
        WatchRule::DiscoveryRun => !surveyed,
        rule => !(known && rule.learnable()),
    });
    findings.sort_by_key(|finding| (finding.level(), finding.rule()));
    findings
}
