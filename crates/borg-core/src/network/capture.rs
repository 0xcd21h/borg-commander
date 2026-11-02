//! Capturing traffic from the network screen: the dialog R opens on the selected row, the capture
//! while `tcpdump` runs, and the viewer it opens on what it caught.

use super::{
    InterfaceKind, NetworkPage, Socket, SocketKey, Transport,
    viewer::{TrafficPage, TrafficView},
};
use crate::{AppState, Capture, Effect, Overlay, Side};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Instant,
};

/// How long a capture can run: a few seconds, or until it is stopped.
pub const DURATIONS: [Option<u64>; 5] = [Some(5), Some(10), Some(30), Some(60), None];
/// What a headers-only capture keeps of each packet: Ethernet, IPv6 and TCP with its options.
pub const HEADER_BYTES: u32 = 128;

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

    /// What the viewer's filter starts as, so that it shows the scope's traffic.
    fn view_filter(&self) -> String {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureField {
    Scope,
    Interface,
    Duration,
    Content,
}

pub const CAPTURE_FIELDS: [CaptureField; 4] =
    [CaptureField::Scope, CaptureField::Interface, CaptureField::Duration, CaptureField::Content];

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
    pub field: usize,
    folder: PathBuf,
}

impl CaptureDialog {
    pub fn selected(&self) -> CaptureField {
        CAPTURE_FIELDS[self.field]
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
        }
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
    /// Stop was asked for, and the capture is being saved.
    pub stopping: bool,
    view_filter: String,
    /// Every socket the screen saw while it ran, to tell each conversation's process.
    sockets: HashMap<SocketKey, Socket>,
    names: HashMap<u32, String>,
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
    let near = scope.and_then(|scope| match scope {
        CaptureScope::Connection { local, .. } => Some(local.ip()),
        CaptureScope::Port { .. } => network.port().and_then(|row| row.addresses.first().copied()),
        _ => None,
    });
    let interfaces = interfaces(network, near);
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
    let last = CAPTURE_FIELDS.len() - 1;
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
    dialog.field = row.min(CAPTURE_FIELDS.len() - 1);
    state.overlay = Some(Overlay::Capture(dialog));
}

fn start(state: &mut AppState, dialog: &CaptureDialog, now: Instant) -> Vec<Effect> {
    let Some(request) = dialog.request() else { return Vec::new() };
    let network = &mut state.network;
    network.next_id += 1;
    let id = network.next_id;
    let view_filter = dialog.scopes.get(dialog.scope).map(CaptureScope::view_filter).unwrap_or_default();
    let mut recording = Recording {
        id,
        request: request.clone(),
        started: now,
        bytes: 0,
        packets: 0,
        stopping: false,
        view_filter,
        sockets: HashMap::new(),
        names: HashMap::new(),
    };
    recording.saw(&network.sample.sockets, &network.sample.names);
    network.recording = Some(recording);
    vec![Effect::Capture { id, request }]
}

impl Recording {
    fn saw(&mut self, sockets: &[Socket], names: &HashMap<u32, String>) {
        for socket in sockets.iter().filter(|socket| socket.pid.is_some()) {
            self.sockets.insert(socket.key(), socket.clone());
        }
        self.names.extend(names.iter().map(|(pid, name)| (*pid, name.clone())));
    }
}

/// Keeps the sockets of each sample the screen takes while a capture runs.
pub(crate) fn sampled(network: &mut super::Network) {
    let (sockets, names) = (&network.sample.sockets, &network.sample.names);
    if let Some(recording) = &mut network.recording {
        recording.saw(sockets, names);
    }
}

pub(crate) fn progress(state: &mut AppState, id: u64, bytes: u64, packets: usize) {
    if let Some(recording) = state.network.recording.as_mut().filter(|recording| recording.id == id) {
        recording.bytes = bytes;
        recording.packets = packets;
    }
}

/// A capture ended: the viewer opens on it while the screen shows, a notice says where it was
/// saved otherwise, and the panels showing its folder list the new file.
pub(crate) fn done(state: &mut AppState, id: u64, result: Result<(PathBuf, Capture), String>) -> Vec<Effect> {
    let Some(recording) = state.network.recording.take_if(|recording| recording.id == id) else { return Vec::new() };
    let saved_in = result.as_ref().ok().and_then(|(path, _)| path.parent()).map(Path::to_path_buf);
    match result {
        Err(error) => state.notice = Some(error),
        Ok((path, capture)) if !state.network.is_open() => {
            let packets = borg_i18n::count(capture.packets.len(), borg_i18n::Count::Packet);
            state.notice = Some(tr!(capture_saved, packets = packets, path = path.display()));
        }
        Ok((path, capture)) => {
            let sockets: Vec<Socket> = recording.sockets.into_values().collect();
            let hosts = state.network.hosts.clone();
            let mut view = TrafficView::new(capture, &sockets, recording.names, &hosts, Some(path));
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
