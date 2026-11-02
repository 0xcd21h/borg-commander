//! Capturing traffic from the network screen: the dialog R opens on the selected row, the capture
//! while `tcpdump` runs, as you or as administrator, and the viewer it opens on what it caught.

use super::{
    InterfaceKind, NetworkPage, Socket, SocketKey, Transport,
    viewer::{TrafficPage, TrafficView},
};
use crate::{
    AppState, Ask, AskKind, Capture, Confirm, ConfirmPurpose, DiffLine, Effect, Installer, Overlay, Side, Tool,
};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Instant,
};

/// The shortest and the longest capture with a length.
pub const SHORTEST_CAPTURE: u64 = 5;
pub const LONGEST_CAPTURE: u64 = 60;
/// How long a capture can run: a few seconds, or until it is stopped.
pub const DURATIONS: [Option<u64>; 5] = [Some(SHORTEST_CAPTURE), Some(10), Some(30), Some(LONGEST_CAPTURE), None];
/// What a headers-only capture keeps of each packet: Ethernet, IPv6 and TCP with its options.
pub const HEADER_BYTES: u32 = 128;
const PCAP_EXTENSION: &str = "pcap";
const PCAPNG_EXTENSION: &str = "pcapng";

/// What capturing as administrator can do here, as the app finds it at startup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureAccess {
    /// Borg already runs as administrator.
    pub administrator: bool,
    /// Borg can ask the system for your password to capture as administrator.
    pub elevate: bool,
    /// A capture as administrator tags each packet with its process, as macOS's `pktap` does.
    pub tags: bool,
}

/// Why a capture saved nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureFailure {
    /// tcpdump is not installed, with how Borg installs it when it knows how.
    Missing(Option<Installer>),
    /// The system lets only an administrator capture on the interface.
    Denied,
    /// The system's password dialog was dismissed, or the capture stopped before it was answered.
    Cancelled,
    /// What went wrong, in words to show as they are.
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureScope {
    /// A listening port, with the connections that came in through it.
    Port {
        transport: Transport,
        port: u16,
    },
    Connection {
        transport: Transport,
        local: SocketAddr,
        remote: SocketAddr,
    },
    /// Everything on the interface, shown for one process.
    Process {
        pid: u32,
    },
    Everything,
}

impl CaptureScope {
    /// The `tcpdump` filter that keeps only this scope's packets.
    pub fn filter(&self) -> String {
        let protocol = |transport: Transport| transport.name().to_lowercase();
        match self {
            Self::Port { transport, port } => format!("{} port {port}", protocol(*transport)),
            Self::Connection { transport, local, remote } => {
                let way = |from: &SocketAddr, to: &SocketAddr| {
                    format!(
                        "(src host {} and src port {} and dst host {} and dst port {})",
                        from.ip(),
                        from.port(),
                        to.ip(),
                        to.port()
                    )
                };
                format!("{} and ({} or {})", protocol(*transport), way(local, remote), way(remote, local))
            }
            Self::Process { .. } | Self::Everything => String::new(),
        }
    }

    /// The scope in words, with a process and a connection's other end as `process` and
    /// `remote` name them.
    pub fn label(&self, process: impl Fn(u32) -> String, remote: impl Fn(SocketAddr) -> String) -> String {
        match *self {
            Self::Port { transport, port } => tr!(capture_scope_port, protocol = transport.name(), port = port),
            Self::Connection { transport, remote: other, .. } => {
                tr!(capture_scope_connection, protocol = transport.name(), remote = remote(other))
            }
            Self::Process { pid } => tr!(capture_scope_process, process = process(pid)),
            Self::Everything => tr!(capture_scope_everything).to_owned(),
        }
    }

    /// What the viewer's filter starts as, so that it shows the scope's traffic.
    pub fn view_filter(&self) -> String {
        match self {
            Self::Process { pid } => format!("pid:{pid}"),
            _ => String::new(),
        }
    }
}

/// What a capture asks `tcpdump` for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub interface: String,
    /// How long it runs, or `None` until it is stopped.
    pub seconds: Option<u64>,
    /// Keep only the first `HEADER_BYTES` of each packet.
    pub headers_only: bool,
    pub filter: String,
    /// The folder the capture file is saved in.
    pub folder: PathBuf,
    /// Run `tcpdump` as administrator, after the system asks for your password.
    pub administrator: bool,
    /// Capture on macOS's `pktap`, which tags each packet with its process and saves pcapng.
    pub processes: bool,
}

impl CaptureRequest {
    /// The extension of the file the capture is saved in.
    pub fn extension(&self) -> &'static str {
        capture_extension(self.processes)
    }
}

/// The extension of a capture's file: pcapng when each packet carries its process.
pub fn capture_extension(processes: bool) -> &'static str {
    if processes { PCAPNG_EXTENSION } else { PCAP_EXTENSION }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureField {
    Scope,
    Interface,
    Duration,
    Content,
    Access,
}

/// Every field, **Run as** last since it shows only where Borg can capture as administrator.
const CAPTURE_FIELDS: [CaptureField; 5] =
    [CaptureField::Scope, CaptureField::Interface, CaptureField::Duration, CaptureField::Content, CaptureField::Access];

/// The dialog R opens: what to capture, where, for how long and how much of each packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureDialog {
    /// The selected row's scope first, then everything.
    pub scopes: Vec<CaptureScope>,
    pub scope: usize,
    pub interfaces: Vec<String>,
    pub interface: usize,
    pub duration: usize,
    pub headers_only: bool,
    /// Capture as administrator.
    pub administrator: bool,
    pub access: CaptureAccess,
    pub field: usize,
    folder: PathBuf,
}

impl CaptureDialog {
    /// The fields the dialog shows.
    pub fn fields(&self) -> &'static [CaptureField] {
        let shown = if self.access.elevate { CAPTURE_FIELDS.len() } else { CAPTURE_FIELDS.len() - 1 };
        &CAPTURE_FIELDS[..shown]
    }

    pub fn selected(&self) -> CaptureField {
        self.fields()[self.field]
    }

    /// Whether the capture tags each packet with its process.
    pub fn tags_processes(&self) -> bool {
        self.access.tags && (self.access.administrator || self.administrator)
    }

    /// The folder the capture is saved in.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn request(&self) -> Option<CaptureRequest> {
        Some(CaptureRequest {
            interface: self.interfaces.get(self.interface)?.clone(),
            seconds: DURATIONS[self.duration],
            headers_only: self.headers_only,
            filter: self.scopes.get(self.scope)?.filter(),
            folder: self.folder.clone(),
            administrator: self.administrator,
            processes: self.tags_processes(),
        })
    }

    /// Moves the selected field's value by `step`, round past either end.
    fn change(&mut self, step: isize) {
        let turn = |value: usize, count: usize| (value as isize + step).rem_euclid(count.max(1) as isize) as usize;
        match self.selected() {
            CaptureField::Scope => self.scope = turn(self.scope, self.scopes.len()),
            CaptureField::Interface => self.interface = turn(self.interface, self.interfaces.len()),
            CaptureField::Duration => self.duration = turn(self.duration, DURATIONS.len()),
            CaptureField::Content => self.headers_only = !self.headers_only,
            CaptureField::Access => self.administrator = !self.administrator,
        }
    }
}

/// Every socket seen while a capture ran, with the names of the processes that held them, to
/// tell each conversation's process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenSockets {
    sockets: HashMap<SocketKey, Socket>,
    names: HashMap<u32, String>,
}

impl SeenSockets {
    pub fn saw(&mut self, sockets: &[Socket], names: &HashMap<u32, String>) {
        for socket in sockets.iter().filter(|socket| socket.pid.is_some()) {
            self.sockets.insert(socket.key(), socket.clone());
        }
        self.names.extend(names.iter().map(|(pid, name)| (*pid, name.clone())));
    }

    /// The viewer on `capture`, each conversation with the process whose socket held it.
    pub fn view(
        &self,
        capture: Capture,
        hosts: &HashMap<IpAddr, Option<String>>,
        path: Option<PathBuf>,
    ) -> TrafficView {
        let sockets: Vec<Socket> = self.sockets.values().cloned().collect();
        TrafficView::new(capture, &sockets, self.names.clone(), hosts, path)
    }
}

/// A capture while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub id: u64,
    pub request: CaptureRequest,
    pub started: Instant,
    pub bytes: u64,
    pub packets: usize,
    /// The system is asking for the administrator's password, and `tcpdump` has not started.
    pub waiting: bool,
    /// Stop was asked for, and the capture is being saved.
    pub stopping: bool,
    view_filter: String,
    /// Every socket the screen saw while it ran.
    seen: SeenSockets,
}

impl Recording {
    /// The seconds left, for a capture with a length.
    pub fn left(&self, now: Instant) -> Option<u64> {
        let elapsed = now.saturating_duration_since(self.started).as_secs();
        self.request.seconds.map(|seconds| seconds.saturating_sub(elapsed))
    }
}

/// R on the network screen: the dialog on the selected row, or stopping a capture that runs.
pub(crate) fn capture_key(state: &mut AppState) -> Vec<Effect> {
    if let Some(recording) = &mut state.network.recording {
        if recording.stopping {
            return Vec::new();
        }
        recording.stopping = true;
        return vec![Effect::StopCapture { id: recording.id }];
    }
    let Some(folder) = state.network.side.map(|side| state.panel(side).dir().to_path_buf()) else { return Vec::new() };
    let network = &state.network;
    let scope = selected_scope(network);
    let interfaces = capture_interfaces(network, scope.unwrap_or(CaptureScope::Everything));
    if interfaces.is_empty() {
        state.notice = Some(tr!(capture_no_interface).into());
        return Vec::new();
    }
    state.overlay = Some(Overlay::Capture(CaptureDialog {
        scopes: scope.into_iter().chain([CaptureScope::Everything]).collect(),
        scope: 0,
        interfaces,
        interface: 0,
        duration: 0,
        headers_only: false,
        administrator: false,
        access: network.access,
        field: 0,
        folder,
    }));
    Vec::new()
}

fn selected_scope(network: &super::Network) -> Option<CaptureScope> {
    match network.page {
        NetworkPage::Ports => network.port().map(|row| CaptureScope::Port { transport: row.transport, port: row.port }),
        NetworkPage::Processes => network.process()?.pid.map(|pid| CaptureScope::Process { pid }),
        NetworkPage::Connections => {
            let (socket, _) = network.connection()?;
            Some(CaptureScope::Connection { transport: socket.transport, local: socket.local, remote: socket.remote? })
        }
        NetworkPage::Interfaces => None,
    }
}

/// The interfaces to capture `scope` on, in the order the dialog offers them.
pub fn capture_interfaces(network: &super::Network, scope: CaptureScope) -> Vec<String> {
    interfaces(network, near(network, scope))
}

/// The address on this machine that a scope's traffic goes through, when it has one: a port's
/// first address, the selected row's when it is that port.
fn near(network: &super::Network, scope: CaptureScope) -> Option<IpAddr> {
    match scope {
        CaptureScope::Connection { local, .. } => Some(local.ip()),
        CaptureScope::Port { transport, port } => network
            .port()
            .into_iter()
            .chain(&network.ports)
            .find(|row| row.transport == transport && row.port == port)
            .and_then(|row| row.addresses.first().copied()),
        CaptureScope::Process { .. } | CaptureScope::Everything => None,
    }
}

/// The interfaces that are up, the one holding `near` first, then the busiest, with loopback
/// last unless `near` is on it.
fn interfaces(network: &super::Network, near: Option<IpAddr>) -> Vec<String> {
    let mut up: Vec<_> = network.sample.interfaces.iter().filter(|interface| interface.up).collect();
    let selected = (network.page == NetworkPage::Interfaces).then(|| network.interface().map(|(i, _)| i.name.clone()));
    let holds = |interface: &super::Interface| {
        near.is_some_and(|near| {
            interface.addresses.iter().any(|(address, _)| *address == near)
                || (near.is_loopback() && interface.kind == InterfaceKind::Loopback)
        })
    };
    up.sort_by_key(|interface| {
        (
            Some(Some(interface.name.clone())) != selected,
            !holds(interface),
            interface.kind == InterfaceKind::Loopback,
            std::cmp::Reverse(network.traffic.interface(&interface.name).total()),
            interface.addresses.is_empty(),
            interface.name.clone(),
        )
    });
    up.into_iter().map(|interface| interface.name.clone()).collect()
}

pub(crate) fn on_dialog_key(
    state: &mut AppState,
    mut dialog: CaptureDialog,
    key: &KeyEvent,
    now: Instant,
) -> Vec<Effect> {
    let last = dialog.fields().len() - 1;
    match crate::keyboard::latin(key).code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Enter => {
            state.overlay = None;
            return start(state, &dialog, now);
        }
        KeyCode::Up => dialog.field = dialog.field.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => dialog.field = (dialog.field + 1).min(last),
        KeyCode::Home => dialog.field = 0,
        KeyCode::End => dialog.field = last,
        KeyCode::Left => dialog.change(-1),
        KeyCode::Right | KeyCode::Char(' ') => dialog.change(1),
        _ => {}
    }
    state.overlay = Some(Overlay::Capture(dialog));
    Vec::new()
}

/// Picks the dialog's row under a click, and changes it when it was already picked.
pub(crate) fn pick(state: &mut AppState, mut dialog: CaptureDialog, row: usize) {
    if dialog.field == row {
        dialog.change(1);
    }
    dialog.field = row.min(dialog.fields().len() - 1);
    state.overlay = Some(Overlay::Capture(dialog));
}

fn start(state: &mut AppState, dialog: &CaptureDialog, now: Instant) -> Vec<Effect> {
    let Some(request) = dialog.request() else { return Vec::new() };
    let view_filter = dialog.scopes.get(dialog.scope).map(CaptureScope::view_filter).unwrap_or_default();
    record(state, request, view_filter, now)
}

/// The answer to whether a refused capture should run as administrator: yes runs it again so.
pub(crate) fn elevate(state: &mut AppState, request: CaptureRequest, view_filter: String, now: Instant) -> Vec<Effect> {
    let access = state.network.access;
    let request = CaptureRequest { administrator: true, processes: access.tags, ..request };
    record(state, request, view_filter, now)
}

fn record(state: &mut AppState, request: CaptureRequest, view_filter: String, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    network.next_id += 1;
    let id = network.next_id;
    let mut recording = Recording {
        id,
        request: request.clone(),
        started: now,
        bytes: 0,
        packets: 0,
        waiting: request.administrator,
        stopping: false,
        view_filter,
        seen: SeenSockets::default(),
    };
    recording.seen.saw(&network.sample.sockets, &network.sample.names);
    network.recording = Some(recording);
    vec![Effect::Capture { id, request }]
}

/// Keeps the sockets of each sample the screen takes while a capture runs.
pub(crate) fn sampled(network: &mut super::Network) {
    let (sockets, names) = (&network.sample.sockets, &network.sample.names);
    if let Some(recording) = &mut network.recording {
        recording.seen.saw(sockets, names);
    }
}

/// The capture's bytes and packets so far; the first report of a capture as administrator
/// also says the password was given, so its seconds count from then.
pub(crate) fn progress(state: &mut AppState, id: u64, bytes: u64, packets: usize, now: Instant) {
    if let Some(recording) = state.network.recording.as_mut().filter(|recording| recording.id == id) {
        if recording.waiting {
            recording.waiting = false;
            recording.started = now;
        }
        recording.bytes = bytes;
        recording.packets = packets;
    }
}

/// A capture ended: the viewer opens on it while the screen shows, a notice says where it was
/// saved otherwise, and the panels showing its folder list the new file. Without tcpdump, Borg
/// offers to install it, and a capture the system refused asks whether to run it as
/// administrator, where Borg can ask for that.
pub(crate) fn done(state: &mut AppState, id: u64, result: Result<(PathBuf, Capture), CaptureFailure>) -> Vec<Effect> {
    let Some(recording) = state.network.recording.take_if(|recording| recording.id == id) else { return Vec::new() };
    let saved_in = result.as_ref().ok().and_then(|(path, _)| path.parent()).map(Path::to_path_buf);
    let access = state.network.access;
    match result {
        Err(CaptureFailure::Missing(installer)) => crate::install::offer(state, Tool::Tcpdump, installer),
        Err(CaptureFailure::Denied) if access.elevate && !recording.request.administrator => {
            state.overlay = Some(Overlay::Confirm(Confirm {
                title: tr!(capture_admin_title),
                message: tr!(capture_admin_question, interface = recording.request.interface),
                purpose: ConfirmPurpose::Capture {
                    request: Box::new(recording.request),
                    view_filter: recording.view_filter,
                },
                accept: true,
            }));
        }
        Err(CaptureFailure::Denied) => {
            state.notice = Some(tr!(capture_denied, interface = recording.request.interface));
        }
        Err(CaptureFailure::Cancelled) => {}
        Err(CaptureFailure::Failed(error)) => state.notice = Some(error),
        Ok((path, capture)) if !state.network.is_open() => {
            let packets = borg_i18n::count(capture.packets.len(), borg_i18n::Count::Packet);
            state.notice = Some(tr!(capture_saved, packets = packets, path = path.display()));
        }
        Ok((path, capture)) => {
            let mut view = recording.seen.view(capture, &state.network.hosts, Some(path));
            view.interface = Some(recording.request.interface);
            view.set_filter(&recording.view_filter);
            state.network.viewer = Some(Box::new(view));
            state.network.filtering = false;
            state.network.details_focused = false;
        }
    }
    let Some(folder) = saved_in else { return Vec::new() };
    let showing: Vec<Side> = Side::ALL.into_iter().filter(|&side| state.panel(side).destination() == folder).collect();
    showing.into_iter().map(|side| state.reload(side)).collect()
}

/// Opens the viewer on a capture file read from a panel.
pub(crate) fn opened(state: &mut AppState, path: PathBuf, result: Result<Capture, String>) -> Vec<Effect> {
    let capture = match result {
        Ok(capture) => capture,
        Err(error) => {
            state.notice = Some(error);
            return Vec::new();
        }
    };
    let effects = if state.network.is_open() { Vec::new() } else { super::open(state) };
    if !state.network.is_open() {
        return effects;
    }
    let network = &mut state.network;
    let mut view = TrafficView::new(capture, &[], HashMap::new(), &network.hosts, Some(path));
    view.page = TrafficPage::Hosts;
    network.viewer = Some(Box::new(view));
    network.filtering = false;
    network.details_focused = false;
    effects
}

/// What the approval dialog shows when an agent asks to capture: the traffic, with the name
/// of a process scope's `process`, the interface, how long, how much of each packet, and where
/// the capture is saved. `place` says who asked.
pub fn capture_ask(scope: CaptureScope, process: Option<&str>, request: &CaptureRequest, place: String) -> Ask {
    let field = |label: &str, value: String| DiffLine::plain(tr!(agent_capture_field, label = label, value = value));
    let traffic = scope.label(
        |pid| tr!(process_control_subject, name = process.unwrap_or(tr!(network_unknown_process)), pid = pid),
        |remote| remote.to_string(),
    );
    let duration = request
        .seconds
        .map_or_else(|| tr!(capture_until_stopped).to_owned(), |seconds| tr!(capture_seconds, seconds = seconds));
    let content = if request.headers_only {
        tr!(capture_headers_only, bytes = HEADER_BYTES)
    } else {
        tr!(capture_whole_packets).to_owned()
    };
    let body = vec![
        field(tr!(capture_scope_field), traffic),
        field(tr!(network_interface_column), request.interface.clone()),
        field(tr!(capture_duration_field), duration.clone()),
        field(tr!(capture_content_field), content),
        DiffLine::plain(tr!(capture_saves_to, extension = request.extension(), folder = request.folder.display())),
        DiffLine::plain(""),
        DiffLine::plain(tr!(agent_capture_warning)),
    ];
    let summary = tr!(agent_capture_summary, interface = request.interface, duration = duration);
    Ask { kind: AskKind::Capture, summary, place, body }
}
