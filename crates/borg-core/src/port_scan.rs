//! Scanning one host's TCP ports in the background. P on the network screen's Neighbors page asks
//! for the target, the ports and the speed, a confirmation names them, and the scan runs as a job
//! that outlives the screen. When it ends, a dialog says what it found once the panels or a
//! monitor screen show and you pause, and the host's details keep the result.

mod form;
mod memory;
mod ports;
mod watch;

#[cfg(test)]
mod tests;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

use crate::{AppState, Confirm, ConfirmPurpose, Effect, Mac, Overlay, View};

pub use form::{FormField, PortScanForm, PortsKind};
pub(crate) use form::{cycled, on_key as on_form_key, paste};
pub use memory::{HOST_HISTORY, MAX_TARGET, PORT_SCANS_FORMAT, PortScanMemory, SavedPortScans};
pub use ports::{
    FIRST_PORT, LAST_PORT, PortChoice, PortListError, PortRange, common_ports, describe, parse_ports, port_name, ranges,
};
pub use watch::{Canary, SILENT_RUN, Tally, Watch};

/// How long the keyboard and mouse rest before a finished scan's dialog opens.
const QUIET: Duration = crate::update_offer::QUIET;
const PAGE_ROWS: usize = 10;
/// Ctrl with this key saves a report.
pub(crate) const SAVE_KEY: char = 's';

/// How a port answered a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortAnswer {
    /// The connection was accepted.
    Open,
    /// The host refused it.
    Closed,
    /// Nothing came back in time.
    Filtered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanSpeed {
    Fast,
    Slow,
}

/// How a scan spaces its connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanPace {
    /// The most connections waiting at once.
    pub in_flight: usize,
    /// How long a connection waits for an answer before the port counts as filtered.
    pub timeout: Duration,
    /// The least time between two new connections.
    pub gap: Duration,
    /// The most time added at random to each gap.
    pub spread: Duration,
    /// How often the canary is asked again.
    pub canary_every: Duration,
}

const CANARY_EVERY: Duration = Duration::from_secs(5);
const FAST: ScanPace = ScanPace {
    in_flight: 64,
    timeout: Duration::from_secs(1),
    gap: Duration::ZERO,
    spread: Duration::ZERO,
    canary_every: CANARY_EVERY,
};
const SLOW: ScanPace = ScanPace {
    in_flight: 16,
    timeout: Duration::from_secs(3),
    gap: Duration::from_millis(100),
    spread: Duration::from_millis(200),
    canary_every: CANARY_EVERY,
};

impl ScanSpeed {
    pub const ALL: [Self; 2] = [Self::Fast, Self::Slow];

    pub fn pace(self) -> ScanPace {
        match self {
            Self::Fast => FAST,
            Self::Slow => SLOW,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => tr!(port_scan_fast),
            Self::Slow => tr!(port_scan_slow),
        }
    }
}

/// The network a scan ran from: the attached subnet the target is on, or else the one the default
/// gateway is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanNetwork {
    pub interface: String,
    pub subnet: Ipv4Addr,
    pub prefix: u8,
    /// This machine's address on the subnet.
    pub own: Ipv4Addr,
    pub gateway: Option<IpAddr>,
    pub gateway_mac: Option<Mac>,
}

impl ScanNetwork {
    pub fn cidr(&self) -> String {
        format!("{}/{}", self.subnet, self.prefix)
    }
}

/// A scan the confirmation asks about, and the job runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanRequest {
    /// The target as typed: a name or an address.
    pub target: String,
    pub address: IpAddr,
    /// The gateway of the attached network the address is on, when it is on one.
    pub gateway: Option<Mac>,
    pub network: Option<ScanNetwork>,
    pub choice: PortChoice,
    pub ports: Vec<u16>,
    pub speed: ScanSpeed,
}

impl PortScanRequest {
    /// The target as the dialogs name it: the name with its address, or the address.
    pub fn label(&self) -> String {
        target_label(&self.target, self.address)
    }
}

fn target_label(target: &str, address: IpAddr) -> String {
    if target == address.to_string() { target.to_owned() } else { format!("{target} ({address})") }
}

/// How a scan ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    Done,
    Cancelled,
    /// The canary went silent: the host or a firewall seems to limit the scan.
    RateLimited {
        port: u16,
    },
    Failed(String),
}

/// What the job sends back when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanResult {
    pub open: Vec<u16>,
    pub closed: usize,
    pub filtered: usize,
    pub not_scanned: Vec<PortRange>,
    pub ending: Ending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortScanEvent {
    /// How many ports have answered, and which are open.
    Progress { done: usize, open: Vec<u16> },
    /// The scan ended, at a time in seconds since the epoch.
    Finished { result: PortScanResult, at: u64 },
}

/// A finished scan as Borg remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortScanRecord {
    pub target: String,
    pub address: IpAddr,
    pub gateway: Option<Mac>,
    pub choice: PortChoice,
    pub speed: ScanSpeed,
    /// How many ports the scan was to try.
    pub count: usize,
    pub at: u64,
    pub open: Vec<u16>,
    pub closed: usize,
    pub filtered: usize,
    pub not_scanned: Vec<PortRange>,
    pub ending: Ending,
    pub network: Option<ScanNetwork>,
}

impl PortScanRecord {
    pub fn label(&self) -> String {
        target_label(&self.target, self.address)
    }

    pub fn skipped(&self) -> usize {
        self.not_scanned.iter().map(|range| range.count()).sum()
    }

    fn adds_up(&self) -> bool {
        let sorted = self.open.windows(2).all(|pair| pair[0] < pair[1]);
        let total = self.open.len() + self.closed + self.filtered + self.skipped();
        sorted && total == self.count && self.target.len() <= MAX_TARGET
    }
}

/// The running scan's state, for the job list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortProgress {
    pub target: String,
    pub speed: ScanSpeed,
    pub done: usize,
    pub total: usize,
    pub open: usize,
}

/// Port scans: what Borg remembers, and finished ones whose dialog waits for its moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortScans {
    pub memory: PortScanMemory,
    /// Whether the remembered scans may be saved: false after the file failed to load.
    pub writable: bool,
    running: Vec<(u64, PortScanRequest)>,
    finished: Vec<PortScanRecord>,
}

impl PortScans {
    pub fn new() -> Self {
        Self { writable: true, ..Self::default() }
    }

    /// The finished scans whose dialog has not opened yet.
    pub fn waiting(&self) -> usize {
        self.finished.len()
    }
}

/// The result dialog over a host's scans, newest first, showing `shown` scrolled to `top`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanView {
    pub scans: Vec<PortScanRecord>,
    pub shown: usize,
    pub top: usize,
}

impl PortScanView {
    /// The dialog over `record`, with the host's earlier scans Borg remembers behind it.
    pub fn of(record: PortScanRecord, memory: &PortScanMemory) -> Self {
        let earlier: Vec<PortScanRecord> =
            memory.history(record.address, record.gateway).filter(|scan| **scan != record).cloned().collect();
        Self { scans: std::iter::once(record).chain(earlier).collect(), shown: 0, top: 0 }
    }

    pub fn record(&self) -> &PortScanRecord {
        &self.scans[self.shown.min(self.scans.len().saturating_sub(1))]
    }
}

/// Whether an address belongs to a private, loopback or link-local range, or to the shared range
/// carrier NAT and overlay networks use.
pub fn is_private(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private() || address.is_loopback() || address.is_link_local() || is_shared(address)
        }
        IpAddr::V6(address) => address.is_loopback() || is_unique_local(address) || is_link_local_v6(address),
    }
}

const SHARED: (Ipv4Addr, u32) = (Ipv4Addr::new(100, 64, 0, 0), 10);
const UNIQUE_LOCAL: (u16, u16) = (0xfc00, 0xfe00);
const LINK_LOCAL_V6: (u16, u16) = (0xfe80, 0xffc0);
const IPV4_BITS: u32 = 32;

fn is_shared(address: Ipv4Addr) -> bool {
    let (network, prefix) = SHARED;
    let mask = u32::MAX << (IPV4_BITS - prefix);
    u32::from(address) & mask == u32::from(network)
}

fn is_unique_local(address: Ipv6Addr) -> bool {
    let (network, mask) = UNIQUE_LOCAL;
    address.segments()[0] & mask == network
}

fn is_link_local_v6(address: Ipv6Addr) -> bool {
    let (network, mask) = LINK_LOCAL_V6;
    address.segments()[0] & mask == network
}

/// The gateway of the attached network an address is on.
fn gateway_of(state: &AppState, address: IpAddr) -> Option<Mac> {
    let IpAddr::V4(address) = address else { return None };
    state.network.places.iter().find(|place| place.contains(address)).and_then(|place| place.gateway)
}

/// The network a scan of `address` runs from: the attached subnet it is on, or the default
/// gateway's.
fn network_of(state: &AppState, address: IpAddr) -> Option<ScanNetwork> {
    let network = &state.network;
    let gateway_interface = network.neighborhood.first_gateway().map(|gateway| gateway.interface.as_str());
    let place = match address {
        IpAddr::V4(address) => network.places.iter().find(|place| place.contains(address)),
        IpAddr::V6(_) => None,
    }
    .or_else(|| network.places.iter().find(|place| Some(place.interface.as_str()) == gateway_interface))?;
    Some(scan_network(network, place))
}

/// An attached network with the address of the gateway on its interface.
pub(crate) fn scan_network(network: &crate::Network, place: &crate::LocalNetwork) -> ScanNetwork {
    let gateways = || network.neighborhood.gateways.iter().filter(|gateway| gateway.interface == place.interface);
    let gateway = gateways().find(|gateway| gateway.address.is_ipv4()).or_else(|| gateways().next());
    ScanNetwork {
        interface: place.interface.clone(),
        subnet: place.network,
        prefix: place.prefix,
        own: place.own,
        gateway: gateway.map(|gateway| gateway.address),
        gateway_mac: place.gateway,
    }
}

/// Whether an address is on one of the attached networks Borg knows.
fn attached(state: &AppState, address: IpAddr) -> bool {
    let IpAddr::V4(address) = address else { return false };
    state.network.places.iter().any(|place| place.contains(address))
}

/// P: asks what to scan, starting from the selected neighbor's address.
pub(crate) fn open_form(state: &mut AppState, target: Option<IpAddr>) -> Vec<Effect> {
    let target = target.map(|address| address.to_string()).unwrap_or_default();
    state.overlay = Some(Overlay::PortScan(Box::new(PortScanForm::new(target))));
    Vec::new()
}

/// The form asks to scan: a typed address goes to the confirmation, and a name is looked up
/// first.
pub(crate) fn submit(state: &mut AppState, mut form: PortScanForm) -> Vec<Effect> {
    let (target, ports) = match form.checked() {
        Ok(checked) => checked,
        Err(error) => {
            form.error = Some(error);
            state.overlay = Some(Overlay::PortScan(Box::new(form)));
            return Vec::new();
        }
    };
    if let Ok(address) = target.parse::<IpAddr>() {
        return confirm(state, &form, target, address, ports);
    }
    form.error = None;
    form.resolving = Some(target.clone());
    state.overlay = Some(Overlay::PortScan(Box::new(form)));
    vec![Effect::ResolveScanTarget(target)]
}

/// The name the form waits for was looked up.
pub(crate) fn resolved(state: &mut AppState, name: String, result: Result<IpAddr, String>) -> Vec<Effect> {
    let Some(Overlay::PortScan(form)) = &mut state.overlay else { return Vec::new() };
    if form.resolving.as_deref() != Some(name.as_str()) {
        return Vec::new();
    }
    form.resolving = None;
    match result {
        Ok(address) => {
            let form = (**form).clone();
            match form.checked() {
                Ok((target, ports)) if target == name => confirm(state, &form, target, address, ports),
                _ => Vec::new(),
            }
        }
        Err(error) => {
            form.error = Some(tr!(port_scan_unresolved, name = name, error = error));
            Vec::new()
        }
    }
}

fn confirm(state: &mut AppState, form: &PortScanForm, target: String, address: IpAddr, ports: Vec<u16>) -> Vec<Effect> {
    let request = PortScanRequest {
        target,
        address,
        gateway: gateway_of(state, address),
        network: network_of(state, address),
        choice: form.choice(),
        ports,
        speed: form.speed,
    };
    let local = is_private(address) || attached(state, address);
    let question = tr!(
        port_scan_question,
        ports = borg_i18n::count(request.ports.len(), borg_i18n::Count::Port),
        target = request.label(),
        mode = request.speed.label()
    );
    let message = if local { question } else { format!("{question} {}", tr!(port_scan_outside, address = address)) };
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(port_scan_title),
        message,
        purpose: ConfirmPurpose::ScanPorts(Box::new(request)),
        accept: local,
    }));
    Vec::new()
}

/// The confirmation said yes: the scan starts as a job.
pub(crate) fn start(state: &mut AppState, request: PortScanRequest, now: Instant) -> Vec<Effect> {
    let progress =
        PortProgress { target: request.label(), speed: request.speed, done: 0, total: request.ports.len(), open: 0 };
    let id = crate::jobs::begin_port_scan(state, progress, now);
    state.notice = Some(tr!(port_scan_started, target = request.label()));
    state.port_scans.running.push((id, request.clone()));
    vec![Effect::ScanPorts { id, request: Box::new(request) }]
}

pub(crate) fn on_event(state: &mut AppState, id: u64, event: PortScanEvent) -> Vec<Effect> {
    match event {
        PortScanEvent::Progress { done, open } => {
            crate::jobs::port_scan_progress(state, id, done, open.len());
            Vec::new()
        }
        PortScanEvent::Finished { result, at } => finished(state, id, result, at),
    }
}

fn finished(state: &mut AppState, id: u64, result: PortScanResult, at: u64) -> Vec<Effect> {
    crate::jobs::end_port_scan(state, id);
    let scans = &mut state.port_scans;
    let Some(index) = scans.running.iter().position(|(running, _)| *running == id) else { return Vec::new() };
    let (_, request) = scans.running.remove(index);
    let record = PortScanRecord {
        target: request.target,
        address: request.address,
        gateway: request.gateway,
        choice: request.choice,
        speed: request.speed,
        count: request.ports.len(),
        at,
        open: result.open,
        closed: result.closed,
        filtered: result.filtered,
        not_scanned: result.not_scanned,
        ending: result.ending,
        network: request.network,
    };
    match &record.ending {
        Ending::Done | Ending::RateLimited { .. } => scans.finished.push(record.clone()),
        Ending::Cancelled => {
            state.notice = Some(tr!(port_scan_cancelled, target = record.label(), open = record.open.len()))
        }
        Ending::Failed(error) => {
            state.notice = Some(tr!(port_scan_failed, target = record.label(), error = error));
            return Vec::new();
        }
    }
    let scans = &mut state.port_scans;
    scans.memory.note(record);
    if scans.writable { vec![Effect::SavePortScans] } else { Vec::new() }
}

/// Nothing shows that the dialog would cover or take keys from: no editor, viewer, dialog or
/// shell.
fn can_open(state: &AppState) -> bool {
    state.view == View::Panels
        && state.overlay.is_none()
        && state.editor.is_none()
        && state.viewer.is_none()
        && state.approval.is_none()
        && state.intro.is_none()
        && !state.screensaver
}

/// Opens the next finished scan's dialog once it can open and the keyboard and mouse have
/// rested.
pub(crate) fn surface(state: &mut AppState, now: Instant) {
    if state.port_scans.finished.is_empty() || !can_open(state) || now < state.idle_since + QUIET {
        return;
    }
    let record = state.port_scans.finished.remove(0);
    state.overlay = Some(Overlay::PortScanDone(Box::new(record)));
}

/// When a waiting dialog opens if nothing happens before.
pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    (!state.port_scans.finished.is_empty() && can_open(state)).then_some(state.idle_since + QUIET)
}

/// The finished scan's dialog: Enter shows the ports, Esc closes it.
pub(crate) fn on_done_key(state: &mut AppState, record: PortScanRecord, key: &KeyEvent) -> Vec<Effect> {
    match crate::keyboard::latin(key).code {
        KeyCode::Enter => {
            let view = PortScanView::of(record, &state.port_scans.memory);
            state.overlay = Some(Overlay::PortScanResult(Box::new(view)));
        }
        KeyCode::Esc | KeyCode::F(10) => state.overlay = None,
        _ => {}
    }
    Vec::new()
}

/// The result dialog scrolls through the open ports, steps to the host's earlier and later scans,
/// and saves the shown scan as a report.
pub(crate) fn on_result_key(state: &mut AppState, mut view: PortScanView, key: &KeyEvent) -> Vec<Effect> {
    let key = crate::keyboard::latin(key);
    if key.code == KeyCode::Char(SAVE_KEY) && key.modifiers == KeyModifiers::CONTROL {
        let source = crate::report::ReportSource::PortScan(Box::new(view.record().clone()));
        return crate::report::open(state, source, Some(Overlay::PortScanResult(Box::new(view))));
    }
    let last = view.record().open.len().saturating_sub(1);
    let older = (view.shown + 1).min(view.scans.len().saturating_sub(1));
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Left if older != view.shown => (view.shown, view.top) = (older, 0),
        KeyCode::Right if view.shown > 0 => (view.shown, view.top) = (view.shown - 1, 0),
        KeyCode::Up => view.top = view.top.saturating_sub(1),
        KeyCode::Down => view.top = (view.top + 1).min(last),
        KeyCode::PageUp => view.top = view.top.saturating_sub(PAGE_ROWS),
        KeyCode::PageDown => view.top = (view.top + PAGE_ROWS).min(last),
        KeyCode::Home => view.top = 0,
        KeyCode::End => view.top = last,
        _ => {}
    }
    state.overlay = Some(Overlay::PortScanResult(Box::new(view)));
    Vec::new()
}

/// O on the Neighbors page: the selected host's scans, or a notice that it has none yet.
pub(crate) fn show_scans(state: &mut AppState) -> Vec<Effect> {
    let Some((neighbor, _)) = state.network.neighbor() else { return Vec::new() };
    let address = neighbor.address;
    match scan_of(state, address).cloned() {
        Some(record) => {
            let view = PortScanView::of(record, &state.port_scans.memory);
            state.overlay = Some(Overlay::PortScanResult(Box::new(view)));
        }
        None => state.notice = Some(tr!(port_scan_none_yet, address = address)),
    }
    Vec::new()
}

/// The scan of a neighbor, found behind the gateway of the network it is on.
pub fn scan_of(state: &AppState, address: IpAddr) -> Option<&PortScanRecord> {
    state.port_scans.memory.find(address, gateway_of(state, address))
}
