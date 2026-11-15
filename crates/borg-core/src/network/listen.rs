//! Listening: Borg hears the broadcast and multicast traffic on a link for a while and puts the
//! hosts that announce themselves on the Neighbors map, with the names and device types they
//! announce. It sends nothing. It runs as a job, through `tcpdump` as a capture does, and goes
//! on with the network screen closed.

use super::{CaptureFailure, NetworkPage, keys};
use crate::{
    AppState, Confirm, ConfirmPurpose, Effect, Overlay, Tool,
    jobs::{self, Detail, Gauge, How, JobKind, Link, Reveal, Unit, Value},
};
use borg_i18n::{Count, count, tr};
use crossterm::event::{KeyCode, KeyEvent};
use std::{net::IpAddr, time::Instant};

mod dns;
mod form;
mod heard;
mod packets;

pub use form::{LISTEN_DURATIONS, ListenField, ListenForm, ListenRequest, listen_interfaces};
pub use heard::{Announced, HeardHost, HeardSwitch, Hearing, MOST_KINDS, Said};
pub use packets::{Announcement, Heard, LONGEST_TEXT, ListenProtocol, SwitchPort, hear};

/// What `tcpdump` keeps: ARP, neighbor discovery, LLDP, CDP, and the UDP ports of mDNS, SSDP,
/// NetBIOS, LLMNR and DHCP.
pub const LISTEN_FILTER: &str = "arp or icmp6 or ether proto 0x88cc or ether dst 01:00:0c:cc:cc:cc or udp port 5353 \
                                 or udp port 1900 or udp port 137 or udp port 5355 or udp port 67 or udp port 68";
/// The hosts heard last that a listen's details name.
const NEWEST_SHOWN: usize = 5;
const SEPARATOR: &str = " · ";

/// What the listener reports while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenEvent {
    /// `tcpdump` started, after the password where the system asked for one.
    Started,
    /// What the frames heard since the last report announced, at a time in seconds since the
    /// epoch. It comes every second, with nothing heard too, so the job's time moves.
    Heard { heard: Vec<Heard>, at: u64 },
    /// Listening ended: its time was up, or why it stopped.
    Finished(Result<(), CaptureFailure>),
}

/// A listen under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listening {
    pub job: u64,
    pub request: ListenRequest,
    /// When `tcpdump` started, once it has.
    pub started: Option<Instant>,
    /// The hosts heard since it started, first heard first.
    pub heard: Vec<IpAddr>,
}

impl Listening {
    fn elapsed(&self, now: Instant) -> u64 {
        self.started.map_or(0, |started| now.saturating_duration_since(started).as_secs())
    }

    fn gauge(&self, now: Instant) -> Gauge {
        match self.request.seconds {
            Some(seconds) => Gauge::of(self.elapsed(now).min(seconds), seconds, Unit::Seconds),
            None => Gauge::counting(self.heard.len() as u64, Unit::Hosts),
        }
    }
}

/// A heard host in a line: its address, its name and where the name came from.
pub fn heard_line(host: &HeardHost) -> String {
    let source = host.announced.name().map_or_else(
        || host.announced.protocols.first().map_or("", |protocol| protocol.name()).to_owned(),
        |name| format!("{}{SEPARATOR}{}", name.text, name.protocol.name()),
    );
    if source.is_empty() { host.address.to_string() } else { format!("{} {source}", host.address) }
}

/// A switch port in a line: the switch's name and the port.
pub fn switch_line(switch: &SwitchPort) -> String {
    [switch.name.as_deref(), switch.port.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
}

fn details(network: &super::Network, listening: &Listening, now: Instant) -> Vec<Detail> {
    let interface = &listening.request.interface;
    let duration = match listening.request.seconds {
        Some(seconds) => Value::Of { done: listening.elapsed(now).min(seconds), total: seconds, unit: Unit::Seconds },
        None => Value::Text(tr!(capture_until_stopped).to_owned()),
    };
    let mut details = vec![
        Detail::new(tr!(@ listen_interface_label), Value::Text(interface.clone())),
        Detail::new(tr!(@ listen_duration_label), duration),
        Detail::new(tr!(@ listen_heard_label), Value::Text(count(listening.heard.len(), Count::Host))),
    ];
    if let Some(switch) = network.hearing.switch(interface) {
        details.push(Detail::new(tr!(@ listen_switch_label), Value::Text(switch_line(&switch.port))));
    }
    let mut newest: Vec<&HeardHost> =
        listening.heard.iter().filter_map(|address| network.hearing.host(*address)).collect();
    newest.sort_by_key(|host| (std::cmp::Reverse(host.last), host.address));
    details.extend(
        newest
            .into_iter()
            .take(NEWEST_SHOWN)
            .map(|host| Detail::new(tr!(@ listen_host_label), Value::Text(heard_line(host)))),
    );
    details
}

/// Shows how far the listen got in its job, once `tcpdump` runs.
fn progress(state: &mut AppState, now: Instant) {
    let Some(listening) = state.network.listening.as_ref().filter(|listening| listening.started.is_some()) else {
        return;
    };
    let (id, gauge, details) = (listening.job, listening.gauge(now), details(&state.network, listening, now));
    jobs::advance(state, id, gauge, details);
}

/// H on the Neighbors page: the form, or stopping the listen that runs.
pub(crate) fn listen_key(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if let Some(listening) = &state.network.listening {
        let id = listening.job;
        return jobs::cancel(state, id, now);
    }
    match ListenForm::of(&state.network) {
        Some(form) => state.overlay = Some(Overlay::Listen(form)),
        None => state.notice = Some(tr!(listen_no_interface).to_owned()),
    }
    Vec::new()
}

pub(crate) fn on_form_key(state: &mut AppState, mut form: ListenForm, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let last = form.fields().len() - 1;
    match crate::keyboard::latin(key).code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Enter => {
            state.overlay = None;
            let Some(request) = form.request() else { return Vec::new() };
            return start(state, request, now);
        }
        KeyCode::Up => form.field = form.field.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => form.field = (form.field + 1).min(last),
        KeyCode::Home => form.field = 0,
        KeyCode::End => form.field = last,
        KeyCode::Left => form.change(-1),
        KeyCode::Right | KeyCode::Char(' ') => form.change(1),
        _ => {}
    }
    state.overlay = Some(Overlay::Listen(form));
    Vec::new()
}

/// Picks the form's row under a click, and changes it when it was already picked.
pub(crate) fn pick(state: &mut AppState, mut form: ListenForm, row: usize) {
    if form.field == row {
        form.change(1);
    }
    form.field = row.min(form.fields().len() - 1);
    state.overlay = Some(Overlay::Listen(form));
}

/// Starts listening as `request` asks, as a job shown in the network screen's panel.
pub(crate) fn start(state: &mut AppState, request: ListenRequest, now: Instant) -> Vec<Effect> {
    if state.network.listening.is_some() {
        return Vec::new();
    }
    let side = state.network.side.unwrap_or(state.active);
    let id = jobs::add(state, JobKind::Listen, request.interface.clone(), side, Link::Runner, now);
    state.network.listening = Some(Listening { job: id, request: request.clone(), started: None, heard: Vec::new() });
    vec![Effect::Listen { id, request }]
}

/// The answer to whether a refused listen should run as administrator: yes runs it again so.
pub(crate) fn as_administrator(state: &mut AppState, request: ListenRequest, now: Instant) -> Vec<Effect> {
    start(state, ListenRequest { administrator: true, ..request }, now)
}

pub(crate) fn on_event(state: &mut AppState, id: u64, event: ListenEvent, now: Instant) -> Vec<Effect> {
    let Some(listening) = state.network.listening.as_mut().filter(|listening| listening.job == id) else {
        return Vec::new();
    };
    match event {
        ListenEvent::Started => {
            listening.started = Some(now);
            progress(state, now);
            Vec::new()
        }
        ListenEvent::Heard { heard, at } => take(state, heard, at, now),
        ListenEvent::Finished(result) => finished(state, id, result, now),
    }
}

/// Takes in what was heard, puts it on the map while the screen shows, and keeps the hosts heard
/// on the networks Borg remembers.
fn take(state: &mut AppState, heard: Vec<Heard>, at: u64, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    let Some(listening) = network.listening.as_mut() else { return Vec::new() };
    let taken = network.hearing.take(&listening.request.interface, heard, &network.sample.interfaces, at);
    for address in &taken {
        if !listening.heard.contains(address) {
            listening.heard.push(*address);
        }
    }
    let save = !taken.is_empty() && network.note_hearing(now);
    if !taken.is_empty() && network.is_open() {
        network.merge();
    }
    progress(state, now);
    save.then_some(Effect::SaveNeighbors).into_iter().collect()
}

fn finished(state: &mut AppState, id: u64, result: Result<(), CaptureFailure>, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    let Some(listening) = network.listening.take_if(|listening| listening.job == id) else { return Vec::new() };
    let access = network.access;
    let save = network.flush(true, now);
    if network.is_open() {
        network.merge();
    }
    let request = listening.request;
    let interface = request.interface.clone();
    let hosts = count(listening.heard.len(), Count::Host);
    let reveal = |state: &mut AppState| {
        if let Some(job) = jobs::find_mut(state, id) {
            job.reveal = Some(Reveal::Neighbors);
        }
    };
    match result {
        Ok(()) => {
            reveal(state);
            jobs::end(state, id, How::Done, tr!(job_listen_done, hosts = hosts), now);
        }
        Err(CaptureFailure::Cancelled) => {
            reveal(state);
            jobs::end(state, id, How::Cancelled, tr!(job_listen_stopped, hosts = hosts), now);
        }
        Err(CaptureFailure::Missing(installer)) => {
            jobs::drop_job(state, id);
            crate::install::offer(state, Tool::Tcpdump, installer);
        }
        Err(CaptureFailure::Denied) if access.elevate && !request.administrator => {
            jobs::drop_job(state, id);
            state.overlay = Some(Overlay::Confirm(Confirm {
                title: tr!(listen_admin_title),
                message: tr!(listen_admin_question, interface = interface),
                purpose: ConfirmPurpose::Listen(Box::new(request)),
                accept: true,
            }));
        }
        Err(CaptureFailure::Denied) => {
            jobs::end(state, id, How::Failed, tr!(listen_denied, interface = interface), now);
        }
        Err(CaptureFailure::Failed(error)) => jobs::end(state, id, How::Failed, error, now),
        Err(CaptureFailure::NoRoom) => {
            jobs::end(state, id, How::Failed, tr!(job_listen_failed, interface = interface), now);
        }
    }
    save.then_some(Effect::SaveNeighbors).into_iter().collect()
}

/// Opening a finished listen: the Neighbors page with what it heard.
pub(crate) fn show_neighbors(state: &mut AppState) -> Vec<Effect> {
    let effects = super::resume(state);
    if state.network.is_open() {
        keys::show_page(&mut state.network, NetworkPage::Neighbors);
    }
    effects
}
