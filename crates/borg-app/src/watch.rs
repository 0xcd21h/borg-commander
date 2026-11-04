//! The process watcher: it hears every process start, asks the rules in `borg-core` what to
//! make of it, freezes what cannot wait, and writes an alert with the evidence. It runs inside
//! the background service, so it keeps watching with no terminal open, and every Borg on the
//! machine reads the same log.
//!
//! Events come from the kernel when Borg has the right to hear them, and otherwise from looking
//! at the user's own processes again and again, which can miss a command that is gone before
//! the next look.

use anyhow::{Context, Result};
use borg_core::{
    Alert, AlertLog, AlertState, AlertsReply, AlertsRequest, Allowance, Baseline, ControlOutcome, Evidence, Guard,
    Limits, Msg, Outcome, ProcessKey, Response, StartedProcess, WatchSource, WatchStatus, Watcher,
};
use borg_i18n::tr;
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::{
        mpsc::{Receiver, UnboundedReceiver, UnboundedSender, channel, unbounded_channel},
        oneshot,
    },
    task::JoinHandle,
};

pub mod facts;
mod freeze;
mod log;
mod netlink;
mod root;

pub use root::{Act as WatchAct, helper as root_helper, install as install_root, uninstall as uninstall_root};

use facts::Processes;

/// How often the polling source looks at the process list.
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// How often a Borg that only shows the page looks at the log.
const READ_EVERY: Duration = Duration::from_secs(5);
/// How often the watcher writes the baseline it has learned.
const LEARN_EVERY: Duration = Duration::from_secs(60);
/// Bytes of a program hashed for the evidence.
const HASH_LIMIT: u64 = 256 * 1024 * 1024;
/// Connections of one process kept as evidence.
const CONNECTIONS_KEPT: usize = 16;
/// Process numbers the kernel's events may queue for the watcher. When the watcher falls this
/// far behind, the kernel drops events instead of Borg's memory growing without end.
const EVENTS_QUEUED: usize = 4096;

/// What the configuration says the watcher does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub on: bool,
    pub freeze: bool,
    pub learning: Duration,
    pub limits: Limits,
    pub allow: Vec<Allowance>,
    pub keep_running: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            on: false,
            freeze: true,
            learning: Duration::from_secs(borg_core::LEARNING_DAYS * 24 * 60 * 60),
            limits: Limits::default(),
            allow: Vec::new(),
            keep_running: Vec::new(),
        }
    }
}

/// The name of a user, or their number when the machine has no name for them.
pub fn user_name(uid: u32) -> String {
    uzers::get_user_by_uid(uid).map_or_else(|| uid.to_string(), |user| user.name().to_string_lossy().into_owned())
}

/// The Alerts page's side of the watcher: it reads the log the watcher writes and does what the
/// page asks of a process.
pub struct Alerts {
    messages: UnboundedSender<Msg>,
    task: Option<JoinHandle<()>>,
    reader: Option<JoinHandle<()>>,
}

impl Alerts {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, task: None, reader: None }
    }

    /// Answers the page in the background, in place of a request still running.
    pub fn request(&mut self, id: u64, request: AlertsRequest) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let messages = self.messages.clone();
        self.task = Some(tokio::spawn(async move {
            let result = match log::folder() {
                Ok(state) => answer(&state, &request).await,
                Err(error) => Err(error),
            };
            let _ = messages.send(Msg::Alerts { id, result: result.map_err(|error| format!("{error:#}")) });
        }));
    }

    /// Follows the log while Borg is open, so the count of unread alerts is right without the
    /// page being open. It reads the log only when the file has changed.
    pub fn follow_log(&mut self) {
        if self.reader.is_some() {
            return;
        }
        let messages = self.messages.clone();
        self.reader = Some(tokio::spawn(async move {
            let mut seen = None;
            loop {
                let state = log::folder().ok();
                if let Some(state) = state {
                    let changed = tokio::task::spawn_blocking({
                        let state = state.clone();
                        move || (log::log_changed(&state), log::read_log(&state))
                    })
                    .await;
                    if let Ok((at, read)) = changed
                        && at.is_some()
                        && seen != at
                    {
                        seen = at;
                        let _ = messages.send(Msg::AlertsChanged(Box::new(read)));
                    }
                }
                tokio::time::sleep(READ_EVERY).await;
            }
        }));
    }
}

impl Drop for Alerts {
    fn drop(&mut self) {
        for task in [self.task.take(), self.reader.take()].into_iter().flatten() {
            task.abort();
        }
    }
}

/// What the page asked for, done: the log as it stands now, and what to tell the user.
async fn answer(state: &Path, request: &AlertsRequest) -> Result<AlertsReply> {
    let processes = Processes::default();
    let mut log = log::read_log(state);
    let notice = match request {
        AlertsRequest::Read => None,
        AlertsRequest::Open => {
            if log.unread() > 0 {
                log.mark_seen();
                log::write_log(state, &log)?;
            }
            None
        }
        AlertsRequest::Resume { alert, process } => {
            act(&processes, &mut log, *alert, *process, Asks::Resume, state).await?
        }
        AlertsRequest::Kill { alert, process } => {
            act(&processes, &mut log, *alert, *process, Asks::Kill, state).await?
        }
        AlertsRequest::Allow { alert, allowance } => {
            allow(state, &mut log, *alert, allowance.clone())?;
            Some(tr!(watch_allowed).into())
        }
    };
    Ok(AlertsReply { log, notice })
}

/// What the Alerts page asks of a process it has an alert about.
enum Asks {
    Resume,
    Kill,
}

/// Lets a frozen process go on, or ends it, and writes down what became of it.
async fn act(
    processes: &Processes,
    log: &mut AlertLog,
    alert: u64,
    process: ProcessKey,
    asks: Asks,
    state: &Path,
) -> Result<Option<String>> {
    let name = log.find(alert).map_or_else(|| process.pid.to_string(), |alert| alert.program().to_owned());
    let mut outcome = match asks {
        Asks::Resume => freeze::resume(processes, process),
        Asks::Kill => freeze::kill(processes, process),
    };
    if outcome == ControlOutcome::Denied {
        let through_the_watcher = match asks {
            Asks::Resume => WatchAct::Resume,
            Asks::Kill => WatchAct::Kill,
        };
        if let Some(answer) = crate::service::watch_act(through_the_watcher, process).await {
            outcome = answer;
        }
    }
    let (next, notice) = match (&asks, outcome) {
        (Asks::Resume, ControlOutcome::Done) => {
            (AlertState::Resumed, tr!(watch_resumed, name = name, pid = process.pid))
        }
        (Asks::Kill, ControlOutcome::Done) => (AlertState::Killed, tr!(watch_killed, name = name, pid = process.pid)),
        (_, ControlOutcome::Gone) => (AlertState::Gone, tr!(watch_act_gone, name = name, pid = process.pid)),
        (_, ControlOutcome::Denied) => {
            return Ok(Some(tr!(watch_act_denied, name = name, pid = process.pid)));
        }
    };
    if let Some(alert) = log.find(alert) {
        alert.state = next;
    }
    log::write_log(state, log)?;
    Ok(Some(notice))
}

/// Writes an allowance to the file the watcher reads, and marks the alert as allowed.
fn allow(state: &Path, log: &mut AlertLog, alert: u64, allowance: Allowance) -> Result<()> {
    log::allow(state, allowance)?;
    if let Some(alert) = log.find(alert) {
        alert.state = AlertState::Allowed;
    }
    log::write_log(state, log)
}

/// Where the watcher hears about processes: from the kernel when Borg itself may listen, from
/// the root watcher once it connects, and meanwhile from the user's own process list. The root
/// watcher may start at any time, so Borg keeps listening for it and stops polling once it
/// speaks.
struct Sources {
    kernel: Option<Receiver<u32>>,
    root: Option<UnboundedReceiver<StartedProcess>>,
    link: Option<root::RootLink>,
    own: Option<Polling>,
    heard_root: bool,
}

/// What the user's own process list looked like last time Borg looked, and what it has yet to
/// report from that look.
struct Polling {
    uid: u32,
    seen: HashSet<u32>,
    queue: VecDeque<u32>,
}

/// A process Borg has heard of: its number, or everything the root watcher read about it.
enum Heard {
    Number(u32),
    Facts(Box<StartedProcess>),
}

impl Sources {
    fn label(&self) -> WatchSource {
        if self.kernel.is_some() || self.heard_root {
            WatchSource::Kernel
        } else if self.own.is_some() {
            WatchSource::OwnProcesses
        } else {
            WatchSource::Off
        }
    }

    /// The next process to look at. Polling stops for good once the root watcher speaks, since
    /// it sees everything the polling could and more.
    async fn next(&mut self, processes: &Processes) -> Result<Heard> {
        if let Some(kernel) = &mut self.kernel {
            let pid = kernel.recv().await.context("the kernel stopped sending process events")?;
            return Ok(Heard::Number(pid));
        }
        loop {
            match (&mut self.root, &mut self.own) {
                (Some(root), Some(own)) => {
                    tokio::select! {
                        process = root.recv() => {
                            if let Some(process) = process {
                                self.heard_root = true;
                                self.own = None;
                                return Ok(Heard::Facts(Box::new(process)));
                            }
                            self.root = None;
                        }
                        () = tokio::time::sleep(LOOK_EVERY) => {
                            if let Some(pid) = own.started(processes) {
                                return Ok(Heard::Number(pid));
                            }
                        }
                    }
                }
                (Some(root), None) => {
                    let process = root.recv().await.context("the root watcher stopped sending processes")?;
                    self.heard_root = true;
                    return Ok(Heard::Facts(Box::new(process)));
                }
                (None, Some(own)) => {
                    tokio::time::sleep(LOOK_EVERY).await;
                    if let Some(pid) = own.started(processes) {
                        return Ok(Heard::Number(pid));
                    }
                }
                (None, None) => anyhow::bail!("nothing is reporting processes any more"),
            }
        }
    }

    /// Stops, continues or ends a process: through the root watcher where there is one, since a
    /// process of another user is not Borg's to signal.
    async fn act(&mut self, processes: &Processes, act: root::Act, process: ProcessKey) -> ControlOutcome {
        if self.heard_root
            && let Some(link) = &mut self.link
        {
            return link.act(act, process).await;
        }
        match act {
            root::Act::Freeze => freeze::freeze(processes, process),
            root::Act::Resume => freeze::resume(processes, process),
            root::Act::Kill => freeze::kill(processes, process),
        }
    }
}

impl Polling {
    fn new(uid: u32, processes: &Processes) -> Self {
        Self { uid, seen: processes.of_user(uid).into_iter().collect(), queue: VecDeque::new() }
    }

    /// The next process that is new since the last look. One look reports everything it found,
    /// in order, before looking again. Numbers that have gone are forgotten, so a number used
    /// again counts as new.
    fn started(&mut self, processes: &Processes) -> Option<u32> {
        if let Some(pid) = self.queue.pop_front() {
            return Some(pid);
        }
        let running: HashSet<u32> = processes.of_user(self.uid).into_iter().collect();
        let mut started: Vec<u32> = running.difference(&self.seen).copied().collect();
        started.sort_unstable();
        self.seen.retain(|pid| running.contains(pid));
        self.seen.extend(&started);
        self.queue.extend(started);
        self.queue.pop_front()
    }
}

/// What a Borg elsewhere on the machine asks the watcher to do to a process, since the watcher
/// may reach processes the user may not signal.
type Asked = (root::Act, ProcessKey, oneshot::Sender<ControlOutcome>);

/// A program whose hash and rule matches the alerts with these numbers wait for.
struct Job {
    alerts: Vec<u64>,
    program: PathBuf,
}

/// What the examiner found in one program, for the alerts that asked.
#[derive(Debug)]
struct Found {
    alerts: Vec<u64>,
    program_hash: Option<String>,
    hits: Vec<borg_rules::RuleMatch>,
    missing: Option<String>,
}

/// Hashes programs and runs Borg's rules and the user's over them on a thread of its own:
/// reading a large program takes long enough to hold up the processes that start meanwhile.
/// Jobs come only with alerts the watcher writes, so the per-minute limits bound the queue.
fn examiner(rules: borg_rules::Rules) -> (UnboundedSender<Job>, UnboundedReceiver<Found>) {
    let (jobs, mut waiting) = unbounded_channel::<Job>();
    let (answers, found) = unbounded_channel();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        while let Some(job) = waiting.blocking_recv() {
            let mut answer =
                Found { alerts: job.alerts, program_hash: hash_of(&job.program), hits: Vec::new(), missing: None };
            match crate::rule_scan::scan_file(&rules, &job.program, &mut buffer) {
                Ok(Some(hits)) => answer.hits = hits,
                Ok(None) => {}
                Err(error) => answer.missing = Some(format!("{error:#}")),
            }
            if answers.send(answer).is_err() {
                return;
            }
        }
    });
    (jobs, found)
}

/// The watcher itself, as the background service runs it.
pub struct Watch {
    processes: Processes,
    settings: Settings,
    /// Programs waiting for their hash and rule matches, and what came of them.
    jobs: UnboundedSender<Job>,
    found: UnboundedReceiver<Found>,
    state: PathBuf,
    watcher: Watcher,
    log: AlertLog,
    sources: Sources,
    asked: UnboundedReceiver<Asked>,
}

/// The service's hold on a running watcher.
pub struct Handle {
    asks: UnboundedSender<Asked>,
    task: JoinHandle<()>,
}

impl Handle {
    /// Asks the watcher to stop, continue or end a process, where the watcher can reach it.
    pub async fn act(&self, act: root::Act, process: ProcessKey) -> Option<ControlOutcome> {
        let (answer, wait) = oneshot::channel();
        self.asks.send((act, process, answer)).ok()?;
        wait.await.ok()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Watch {
    /// Starts watching: the kernel's events where Borg may hear them, the root watcher once it
    /// connects, and the user's own processes meanwhile. The baseline comes from the state
    /// folder, so a restart does not begin learning again.
    pub async fn start(settings: Settings, asked: UnboundedReceiver<Asked>) -> Result<Self> {
        let state = log::folder()?;
        let processes = Processes::default();
        let baseline = log::read_baseline(&state).map_or_else(
            || Baseline::started(SystemTime::now(), settings.learning),
            |mut baseline| {
                baseline.period = settings.learning;
                baseline
            },
        );
        let kernel = kernel_events().ok();
        let (link, root) = match kernel {
            Some(_) => (None, None),
            None => match root::RootLink::listen(&state).await {
                Ok((link, processes)) => (Some(link), Some(processes)),
                Err(error) => {
                    eprintln!("borg watch: {error:#}");
                    (None, None)
                }
            },
        };
        let own = kernel.is_none().then(|| Polling::new(rustix::process::getuid().as_raw(), &processes));
        let sources = Sources { kernel, root, link, own, heard_root: false };
        let mut watcher = Watcher::new(baseline, settings.limits);
        watcher.baseline.allowed = allowances(&settings, &state);
        let mut log = log::read_log(&state);
        log.watch = WatchStatus {
            source: sources.label(),
            since: watcher.baseline.since,
            learned_at: watcher.baseline.learned_at(),
            freeze: settings.freeze,
            seen: 0,
            suppressed: 0,
        };
        log::write_log(&state, &log)?;
        let (jobs, found) = examiner(crate::rule_scan::compile(crate::rule_scan::folder().as_deref()));
        Ok(Self { processes, settings, jobs, found, state, watcher, log, sources, asked })
    }

    /// Watches until nothing reports processes any more. Each process that starts is looked at,
    /// and what comes of it is written down.
    pub async fn run(&mut self) -> Result<()> {
        let mut learning = tokio::time::interval(LEARN_EVERY);
        loop {
            let heard = tokio::select! {
                heard = self.sources.next(&self.processes) => heard?,
                asked = self.asked.recv() => {
                    if let Some((act, process, answer)) = asked {
                        let outcome = self.sources.act(&self.processes, act, process).await;
                        let _ = answer.send(outcome);
                        self.note_act(act, process, outcome)?;
                    }
                    continue;
                }
                _ = learning.tick() => {
                    self.keep_up()?;
                    continue;
                }
                Some(found) = self.found.recv() => {
                    self.take_in(found)?;
                    continue;
                }
            };
            let now = SystemTime::now();
            let started = match heard {
                Heard::Facts(process) => Some(*process),
                Heard::Number(pid) => self.processes.started(pid, now),
            };
            let Some(started) = started else { continue };
            let guard = Guard {
                own: std::process::id(),
                keep_running: &self.settings.keep_running,
                freeze: self.settings.freeze,
            };
            let outcomes = self.watcher.examine(&started, &guard, now);
            self.record(&started, outcomes, now).await?;
        }
    }

    /// Writes down what the watcher has learned, and takes in the starts the user has allowed
    /// since the last look.
    fn keep_up(&mut self) -> Result<()> {
        self.watcher.baseline.allowed = allowances(&self.settings, &self.state);
        log::write_baseline(&self.state, &self.watcher.baseline)
    }

    /// Freezes what Borg decided to freeze, keeps the connections the process holds, and
    /// writes the alerts. The program's hash and rule matches follow once the examiner has
    /// read it.
    async fn record(&mut self, started: &StartedProcess, outcomes: Vec<Outcome>, now: SystemTime) -> Result<()> {
        self.log.watch.seen = self.watcher.seen();
        self.log.watch.suppressed = self.watcher.suppressed();
        self.log.watch.source = self.sources.label();
        if outcomes.is_empty() {
            return Ok(());
        }
        let mut response = outcomes.first().map(|outcome| outcome.response.clone()).unwrap_or(Response::Alerted);
        if response.froze()
            && self.sources.act(&self.processes, root::Act::Freeze, started.key).await != ControlOutcome::Done
        {
            response = Response::Alerted;
        }
        let program = started.program.clone().filter(|_| !started.program_deleted);
        let evidence = Evidence {
            connections: connections(&self.processes, started.key.pid),
            missing: program.is_none().then(|| tr!(watch_state_gone).into()),
            ..Evidence::default()
        };
        let mut alerts = Vec::new();
        for outcome in outcomes {
            let id = self.log.add(now, outcome.finding, response.clone(), started.clone());
            if let Some(alert) = self.log.find(id) {
                alert.evidence = evidence.clone();
            }
            alerts.push(id);
        }
        if let Some(program) = program {
            let _ = self.jobs.send(Job { alerts, program });
        }
        log::write_log(&self.state, &self.log)
    }

    /// Adds what the examiner found in a program to the alerts that waited for it.
    fn take_in(&mut self, found: Found) -> Result<()> {
        let mut changed = false;
        for id in &found.alerts {
            if let Some(alert) = self.log.find(*id) {
                alert.evidence.program_hash.clone_from(&found.program_hash);
                alert.evidence.hits.clone_from(&found.hits);
                if found.missing.is_some() {
                    alert.evidence.missing.clone_from(&found.missing);
                }
                changed = true;
            }
        }
        if changed { log::write_log(&self.state, &self.log) } else { Ok(()) }
    }

    /// Writes down what became of a process another Borg asked about, so the page shows it
    /// wherever it is open.
    fn note_act(&mut self, act: root::Act, process: ProcessKey, outcome: ControlOutcome) -> Result<()> {
        let state = match (act, outcome) {
            (_, ControlOutcome::Denied) => return Ok(()),
            (_, ControlOutcome::Gone) => AlertState::Gone,
            (root::Act::Freeze, _) => AlertState::Frozen,
            (root::Act::Resume, _) => AlertState::Resumed,
            (root::Act::Kill, _) => AlertState::Killed,
        };
        let mut changed = false;
        for alert in &mut self.log.alerts {
            if alert.process.key == process {
                alert.state = state;
                changed = true;
            }
        }
        if changed { log::write_log(&self.state, &self.log) } else { Ok(()) }
    }
}

/// Every start the user allows: the ones in the configuration and the ones the Alerts page has
/// written down. The watcher never writes them, so it cannot lose one.
fn allowances(settings: &Settings, state: &Path) -> Vec<Allowance> {
    let mut allowed = settings.allow.clone();
    for allowance in log::read_allowances(state) {
        if !allowed.contains(&allowance) {
            allowed.push(allowance);
        }
    }
    allowed
}

/// The kernel's process events, read on a thread of their own and sent on as process numbers.
/// Only an exec counts: right after a fork a process still shows its parent's program, and
/// looking then would both say the wrong thing and teach the baseline the wrong pair. A process
/// that never execs runs the program its parent was already looked at for.
///
/// When processes start faster than the watcher looks at them, the queue fills, the kernel
/// drops events and says so, and the reader carries on: stopping would leave the machine
/// unwatched.
fn kernel_events() -> Result<Receiver<u32>> {
    use rustix::io::Errno;
    let socket = netlink::subscribe().context("hearing the kernel's process events needs root")?;
    let (sender, receiver) = channel(EVENTS_QUEUED);
    std::thread::spawn(move || {
        let mut buffer = [0_u8; netlink::DATAGRAM];
        loop {
            let count = match rustix::net::recv(&socket, &mut buffer, rustix::net::RecvFlags::empty()) {
                Ok((count, _)) => count,
                Err(Errno::NOBUFS | Errno::INTR) => continue,
                Err(_) => break,
            };
            for event in netlink::parse_events(&buffer[..count]) {
                let pid = match event {
                    netlink::ProcEvent::Executed { pid } => Some(pid),
                    netlink::ProcEvent::Forked { .. } | netlink::ProcEvent::Exited { .. } => None,
                };
                if let Some(pid) = pid
                    && sender.blocking_send(pid).is_err()
                {
                    return;
                }
            }
        }
    });
    Ok(receiver)
}

/// The SHA-256 of a program, which says whether the same file turns up again elsewhere.
fn hash_of(program: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read as _;
    let mut file = std::fs::File::open(program).ok()?.take(HASH_LIMIT);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// The connections a process holds, as its sockets' numbers, which say how far it had got.
fn connections(processes: &Processes, pid: u32) -> Vec<String> {
    let folder = processes.root().join(pid.to_string()).join("fd");
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut sockets: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .map(|target| target.to_string_lossy().into_owned())
        .filter(|target| facts::stream_kind(target) == borg_core::StreamKind::Socket)
        .collect();
    sockets.sort();
    sockets.dedup();
    sockets.truncate(CONNECTIONS_KEPT);
    sockets
}

/// Runs the watcher for as long as the settings ask for it, inside the background service.
pub fn start(settings: Settings) -> Option<Handle> {
    if !settings.on {
        return None;
    }
    let (asks, asked) = unbounded_channel();
    let task = tokio::spawn(async move {
        match Watch::start(settings, asked).await {
            Ok(mut watch) => {
                if let Err(error) = watch.run().await {
                    eprintln!("borg watch: {error:#}");
                }
            }
            Err(error) => eprintln!("borg watch: {error:#}"),
        }
    });
    Some(Handle { asks, task })
}

/// What one alert says in a line, for `borg service watch`.
pub fn describe(alert: &Alert) -> String {
    format!("{} · {} · {}", alert.rule().label(), alert.program(), alert.state.label())
}

/// `borg service watch`: where the watcher's events come from, and what it has found.
pub fn report() -> Result<String> {
    let state = log::folder()?;
    let log = log::read_log(&state);
    let mut lines = vec![tr!(watch_watching, source = log.watch.source.label())];
    lines.extend(log.alerts.iter().map(describe));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests;
