//! Asks shared between borgs. A host's borg tells the borgs whose panels show its session what
//! waits there for an answer, and takes the first answer that comes, from its own screen or from
//! one of theirs. Those borgs show the asks in the same dialog as their own.

use borg_i18n::tr;
use serde::{Deserialize, Serialize};

use super::{Approval, Ask, decide, waits_here};
use crate::{AppState, Effect};

/// How many ended asks a host keeps telling its viewers about, so each learns how its dialog
/// ended even when it asked again only after a few more had come and gone.
const ENDED_LIMIT: usize = 32;

/// An ask waiting in a host's borg, by the number it has there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiting {
    pub id: u64,
    pub ask: Ask,
}

/// How an ask ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum End {
    /// Answered on the computer `by`, from its borg; none for the host's own screen.
    Answered { approved: bool, by: Option<String> },
    /// The agent stopped waiting, or its connection closed.
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ended {
    pub id: u64,
    pub end: End,
}

/// What a host's borg tells one viewer about its session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskFeed {
    /// This run of the host's borg. An answer names it, so an answer meant for an earlier run
    /// cannot reach an ask of this one that has the same number.
    pub run: String,
    /// Grows each time any of the rest changes.
    pub version: u64,
    pub waiting: Vec<Waiting>,
    pub ended: Vec<Ended>,
    /// The paired computers the session approves everything for.
    pub approve_all: Vec<String>,
    /// The computer that controls the session, when that is not the viewer's: the viewer then
    /// shows the asks without answering them.
    pub view_only: Option<String>,
}

/// A viewer's answer to an ask of a host's borg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAnswer {
    pub run: String,
    pub id: u64,
    pub approved: bool,
}

/// Why a host took no answer from a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The answer is for an ask of an earlier run of the host's borg.
    OtherRun,
    /// The ask was answered first, or it ended.
    Ended,
    /// Another computer controls the session.
    ViewOnly,
}

/// What a host's borg shows its viewers, taken again from its state whenever that changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskBoard {
    version: u64,
    waiting: Vec<Waiting>,
    ended: Vec<Ended>,
    approve_all: Vec<String>,
    controller: Option<String>,
}

impl AskBoard {
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Takes in what waits in `state` and the computer that controls the session, under a new
    /// version when any of it changed. Asks never change, so their numbers tell them apart.
    pub fn refresh(&mut self, state: &AppState, controller: Option<&str>) -> bool {
        let ids = waiting_ids(state);
        let same = self.waiting.iter().map(|waiting| waiting.id).eq(ids.iter().copied())
            && self.ended.iter().eq(state.agent.ended.iter())
            && self.approve_all == state.agent.auto_approve
            && self.controller.as_deref() == controller;
        if same {
            return false;
        }
        self.version += 1;
        self.waiting = ids.into_iter().filter_map(|id| asked(state, id)).collect();
        self.ended = state.agent.ended.iter().cloned().collect();
        self.approve_all = state.agent.auto_approve.clone();
        self.controller = controller.map(str::to_owned);
        true
    }

    /// What `viewer`, a paired computer by the name its connection carries, hears.
    pub fn feed(&self, run: &str, viewer: &str) -> AskFeed {
        AskFeed {
            run: run.to_owned(),
            version: self.version,
            waiting: self.waiting.clone(),
            ended: self.ended.clone(),
            approve_all: self.approve_all.clone(),
            view_only: self.controller.clone().filter(|controller| controller != viewer),
        }
    }
}

/// The numbers of the asks waiting here, oldest first: the one the dialog shows, then the rest.
fn waiting_ids(state: &AppState) -> Vec<u64> {
    let shown = state.approval.as_ref().filter(|approval| approval.host.is_none()).map(|approval| approval.id);
    shown.into_iter().chain(state.agent.asks.iter().map(|(id, _)| *id)).collect()
}

fn asked(state: &AppState, id: u64) -> Option<Waiting> {
    let shown = state.approval.as_ref().filter(|approval| approval.host.is_none() && approval.id == id);
    let ask = shown
        .map(|approval| &approval.ask)
        .or_else(|| state.agent.asks.iter().find(|(waiting, _)| *waiting == id).map(|(_, ask)| ask))?;
    Some(Waiting { id, ask: ask.clone() })
}

/// Notes how an ask here ended, for the viewers that show it too.
pub(super) fn ended(state: &mut AppState, id: u64, end: End) {
    let log = &mut state.agent.ended;
    log.push_back(Ended { id, end });
    while log.len() > ENDED_LIMIT {
        log.pop_front();
    }
}

/// Whether a viewer's answer can be taken: it names this run of borg and an ask that still
/// waits here, and no other computer controls the session.
pub fn check_answer(
    state: &AppState,
    answer: &HostAnswer,
    run: &str,
    viewer: &str,
    controller: Option<&str>,
) -> Result<(), Refusal> {
    if answer.run != run {
        return Err(Refusal::OtherRun);
    }
    if !waits_here(state, answer.id) {
        return Err(Refusal::Ended);
    }
    if controller.is_some_and(|controller| controller != viewer) {
        return Err(Refusal::ViewOnly);
    }
    Ok(())
}

/// A viewer's answer, taken after [`check_answer`]: the ask leaves the dialog or the queue, a
/// notice names the computer that answered, and the answer is carried out as one given here.
pub(super) fn answered_from(state: &mut AppState, id: u64, approved: bool, by: &str) -> Vec<Effect> {
    let ask = match state.approval.take_if(|approval| approval.host.is_none() && approval.id == id) {
        Some(approval) => approval.ask,
        None => {
            let Some(index) = state.agent.asks.iter().position(|(waiting, _)| *waiting == id) else {
                return Vec::new();
            };
            state.agent.asks.remove(index).1
        }
    };
    state.notice = Some(tr!(agent_answered_from, computer = computer_name(by), summary = ask.summary));
    decide(state, id, &ask, approved, Some(by.to_owned()))
}

/// The name of a paired computer, without the start of its key that its connection's name
/// carries.
pub fn computer_name(origin: &str) -> &str {
    origin.rsplit_once(" [").map_or(origin, |(name, _)| name)
}

/// A host session one of the panels here shows, and what its borg last said waits there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedHost {
    pub machine: String,
    pub session: String,
    feed: Option<AskFeed>,
    /// Asks answered here, or closed here without an answer, which do not show again.
    settled: Vec<u64>,
}

impl WatchedHost {
    /// The paired computers the host session approves everything for, as its borg last said.
    pub fn approve_all(&self) -> &[String] {
        self.feed.as_ref().map_or(&[], |feed| &feed.approve_all)
    }
}

/// What a viewer hears about a host session one of its panels shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Feed(AskFeed),
    /// The connection to the host dropped, and borg is opening it again.
    Lost,
    /// No panel shows the session any more.
    Unwatched,
    /// The host took no answer from here.
    Refused(Refusal),
    /// The answer to this ask did not reach the host, where it still waits.
    Unreached(u64),
    /// The host turned off Approve all, as asked from here.
    ApproveAllOff,
    /// The host could not be reached to turn off Approve all.
    ApproveAllStillOn,
}

/// The host session an ask in the dialog here waits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAsk {
    pub machine: String,
    pub session: String,
    /// The host as the panels name it.
    pub server: String,
    pub run: String,
    /// The computer that controls the session, when another one does: the dialog shows the
    /// ask without answering it.
    pub view_only: Option<String>,
}

impl HostAsk {
    fn of(&self, machine: &str, session: &str) -> bool {
        self.machine == machine && self.session == session
    }
}

/// Takes in what a host session's borg said, closing the dialog when the ask it shows ended
/// there, with a notice of how.
pub(crate) fn receive(state: &mut AppState, machine: String, session: String, event: HostEvent) -> Vec<Effect> {
    let server = state.remote_host_name(&machine).to_owned();
    let shown = state
        .approval
        .as_ref()
        .and_then(|approval| Some((approval.id, approval.host.as_ref().filter(|host| host.of(&machine, &session))?)))
        .map(|(id, host)| (id, host.run.clone()));
    let index = state.agent.hosts.iter().position(|host| host.machine == machine && host.session == session);
    match event {
        HostEvent::Feed(feed) => {
            if let Some((id, run)) = shown {
                let same_run = run == feed.run;
                if same_run && feed.waiting.iter().any(|waiting| waiting.id == id) {
                    if let Some(host) = state.approval.as_mut().and_then(|approval| approval.host.as_mut()) {
                        host.view_only = feed.view_only.clone();
                    }
                } else if let Some(approval) = state.approval.take() {
                    let end = feed.ended.iter().find(|ended| same_run && ended.id == id).map(|ended| &ended.end);
                    state.notice = Some(ended_notice(end, &server, &approval.ask.summary));
                }
            }
            let host = match index {
                Some(index) => &mut state.agent.hosts[index],
                None => {
                    state.agent.hosts.push(WatchedHost { machine, session, feed: None, settled: Vec::new() });
                    state.agent.hosts.last_mut().expect("a host was just added")
                }
            };
            host.settled.retain(|id| feed.waiting.iter().any(|waiting| waiting.id == *id));
            host.feed = Some(feed);
        }
        HostEvent::Lost => {
            if let Some(index) = index {
                state.agent.hosts[index].feed = None;
            }
            if shown.is_some() {
                state.approval = None;
                state.notice = Some(tr!(agent_host_lost, server = server));
            }
        }
        HostEvent::Unwatched => {
            if let Some(index) = index {
                state.agent.hosts.remove(index);
            }
            if shown.is_some() {
                state.approval = None;
            }
        }
        HostEvent::Refused(refusal) => {
            state.notice = Some(match refusal {
                Refusal::OtherRun => tr!(agent_host_refused_run, server = server),
                Refusal::Ended => tr!(agent_host_refused_ended, server = server),
                Refusal::ViewOnly => tr!(agent_host_refused_view_only, server = server),
            });
        }
        HostEvent::Unreached(id) => {
            if let Some(index) = index {
                state.agent.hosts[index].settled.retain(|settled| *settled != id);
            }
            state.notice = Some(tr!(agent_host_unreachable, server = server));
        }
        HostEvent::ApproveAllOff => state.notice = Some(tr!(agent_host_approve_all_off, server = server)),
        HostEvent::ApproveAllStillOn => state.notice = Some(tr!(agent_host_stop_unreachable, server = server)),
    }
    Vec::new()
}

/// What the viewer hears when an ask it shows ended on the host.
fn ended_notice(end: Option<&End>, server: &str, summary: &str) -> String {
    match end {
        Some(End::Answered { by: None, .. }) => tr!(agent_host_answered, server = server, summary = summary),
        Some(End::Answered { by: Some(by), .. }) => {
            tr!(agent_answered_from, computer = computer_name(by), summary = summary)
        }
        Some(End::Withdrawn) => tr!(agent_withdrawn, summary = summary),
        None => tr!(agent_host_closed, server = server, summary = summary),
    }
}

/// The oldest ask a host session waits for that this borg has not answered or closed.
pub(super) fn next_host_ask(state: &AppState) -> Option<(u64, Ask, HostAsk)> {
    state.agent.hosts.iter().find_map(|host| {
        let feed = host.feed.as_ref()?;
        let waiting = feed.waiting.iter().find(|waiting| !host.settled.contains(&waiting.id))?;
        let found = HostAsk {
            machine: host.machine.clone(),
            session: host.session.clone(),
            server: state.remote_host_name(&host.machine).to_owned(),
            run: feed.run.clone(),
            view_only: feed.view_only.clone(),
        };
        Some((waiting.id, waiting.ask.clone(), found))
    })
}

/// Closes a forwarded ask here, so it does not show again while its host still waits.
pub(super) fn settle(state: &mut AppState, approval: &Approval) {
    let Some(asked) = &approval.host else { return };
    if let Some(host) =
        state.agent.hosts.iter_mut().find(|host| host.machine == asked.machine && host.session == asked.session)
    {
        host.settled.push(approval.id);
    }
}

/// Sends the user's answer to the host the ask waits in.
pub(super) fn answer_host(state: &mut AppState, id: u64, asked: HostAsk, approved: bool) -> Vec<Effect> {
    if let Some(host) =
        state.agent.hosts.iter_mut().find(|host| host.machine == asked.machine && host.session == asked.session)
    {
        host.settled.push(id);
    }
    let answer = HostAnswer { run: asked.run, id, approved };
    vec![Effect::AnswerHost { machine: asked.machine, session: asked.session, answer }]
}

/// Turns off Approve all here and in every host session whose borg approves everything for
/// someone.
pub(crate) fn approve_all_off(state: &AppState) -> Vec<Effect> {
    let hosts = state.agent.hosts.iter().filter(|host| !host.approve_all().is_empty());
    let mut effects: Vec<Effect> = hosts
        .map(|host| Effect::StopHostApproveAll { machine: host.machine.clone(), session: host.session.clone() })
        .collect();
    if effects.is_empty() || !state.agent.auto_approve.is_empty() {
        effects.insert(0, Effect::ApprovalModeOff);
    }
    effects
}
