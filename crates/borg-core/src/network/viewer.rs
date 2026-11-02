//! The traffic viewer: a capture's processes, hosts, conversations and packets, a page each in
//! the network screen's list pane, and the selected row's details or a packet's bytes in the
//! other.

use super::{NetworkFilter, Socket, Transport, filter::Haystack, service};
use crate::{Capture, Flow, FlowKind, Packet};
use borg_i18n::tr;
use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

mod keys;
mod report;

pub(crate) use keys::{click, paste, route_key, show_page};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficPage {
    Processes,
    Hosts,
    Connections,
    Packets,
}

impl TrafficPage {
    pub const ALL: [Self; 4] = [Self::Processes, Self::Hosts, Self::Connections, Self::Packets];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The digit that opens the page.
    pub fn key(self) -> char {
        char::from(b'1' + self as u8)
    }
}

/// A process's share of the capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Talker {
    pub pid: Option<u32>,
    pub packets: usize,
    pub bytes: u64,
    /// Its conversations, by their place in the capture.
    pub flows: Vec<usize>,
    pub hosts: usize,
}

/// What went to and from one other machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    pub address: IpAddr,
    pub packets: usize,
    pub bytes: u64,
    pub flows: Vec<usize>,
    pub pids: Vec<Option<u32>>,
    pub protocols: Vec<&'static str>,
}

/// A conversation's end on this machine, the other end, and the process behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ends {
    pub pid: Option<u32>,
    pub local: SocketAddr,
    pub remote: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct TrafficView {
    pub capture: Capture,
    pub path: Option<PathBuf>,
    pub interface: Option<String>,
    pub page: TrafficPage,
    /// Each conversation's ends, by its place in the capture.
    pub ends: Vec<Ends>,
    names: HashMap<u32, String>,
    hosts: HashMap<IpAddr, String>,
    pub talkers: Vec<Talker>,
    pub host_rows: Vec<HostRow>,
    pub flows: Vec<usize>,
    pub packets: Vec<usize>,
    /// How many rows each page has before the filter.
    pub totals: [usize; 4],
    cursors: [usize; 4],
    tops: [usize; 4],
    pub filter: String,
    pub filtering: bool,
    /// How far the details are scrolled.
    pub details_top: usize,
    /// The pages Enter went from, with their filters and rows, for Esc to go back to.
    history: Vec<(TrafficPage, String, usize)>,
}

impl TrafficView {
    /// The viewer on `capture`, with each conversation's process found among `sockets`, and
    /// the names known for processes and addresses.
    pub fn new(
        capture: Capture,
        sockets: &[Socket],
        names: HashMap<u32, String>,
        resolved: &HashMap<IpAddr, Option<String>>,
        path: Option<PathBuf>,
    ) -> Self {
        let ends: Vec<Ends> = capture.flows.iter().map(|flow| ends(flow, sockets)).collect();
        let mut hosts: HashMap<IpAddr, String> =
            resolved.iter().filter_map(|(address, name)| Some((*address, name.clone()?))).collect();
        for (flow, end) in capture.flows.iter().zip(&ends) {
            let named = matches!(flow.app, Some("TLS" | "HTTP"));
            if let (true, Some(name), true) = (named, &flow.server_name, end.remote == flow.server) {
                hosts.insert(end.remote.ip(), name.clone());
            }
        }
        hosts.extend(capture.names.iter().map(|(address, name)| (*address, name.clone())));
        let mut view = Self {
            capture,
            path,
            interface: None,
            page: TrafficPage::Processes,
            ends,
            names,
            hosts,
            talkers: Vec::new(),
            host_rows: Vec::new(),
            flows: Vec::new(),
            packets: Vec::new(),
            totals: [0; 4],
            cursors: [0; 4],
            tops: [0; 4],
            filter: String::new(),
            filtering: false,
            details_top: 0,
            history: Vec::new(),
        };
        view.rebuild();
        view
    }

    /// Keeps the rows that match `text`, as the screen's filter does.
    pub fn set_filter(&mut self, text: &str) {
        text.clone_into(&mut self.filter);
        self.rebuild();
    }

    /// The name of the process with `pid`, or what the screen calls a process it cannot tell.
    pub fn name(&self, pid: Option<u32>) -> String {
        pid.and_then(|pid| self.names.get(&pid)).cloned().unwrap_or_else(|| tr!(network_unknown_process).into())
    }

    pub fn host(&self, address: IpAddr) -> Option<&str> {
        self.hosts.get(&address).map(String::as_str)
    }

    /// An endpoint by the name of its host when the capture or the screen knows it.
    pub fn endpoint(&self, address: SocketAddr) -> String {
        match self.host(address.ip()) {
            Some(host) => format!("{host}:{}", address.port()),
            None => address.to_string(),
        }
    }

    pub fn len(&self, page: TrafficPage) -> usize {
        match page {
            TrafficPage::Processes => self.talkers.len(),
            TrafficPage::Hosts => self.host_rows.len(),
            TrafficPage::Connections => self.flows.len(),
            TrafficPage::Packets => self.packets.len(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursors[self.page.index()]
    }

    pub fn list_start(&self, height: usize) -> usize {
        let cursor = self.cursor();
        self.tops[self.page.index()].min(cursor).max(cursor.saturating_sub(height.max(1) - 1))
    }

    pub(crate) fn keep_visible(&mut self, height: usize) {
        self.tops[self.page.index()] = self.list_start(height);
    }

    pub(crate) fn select(&mut self, index: usize) {
        let index = index.min(self.len(self.page).saturating_sub(1));
        if self.cursors[self.page.index()] != index {
            self.details_top = 0;
        }
        self.cursors[self.page.index()] = index;
    }

    pub(crate) fn move_by(&mut self, delta: isize) {
        let last = self.len(self.page).saturating_sub(1) as isize;
        self.select((self.cursor() as isize + delta).clamp(0, last.max(0)) as usize);
    }

    pub fn talker(&self) -> Option<&Talker> {
        self.talkers.get(self.cursors[TrafficPage::Processes.index()]).filter(|_| self.page == TrafficPage::Processes)
    }

    pub fn host_row(&self) -> Option<&HostRow> {
        self.host_rows.get(self.cursors[TrafficPage::Hosts.index()]).filter(|_| self.page == TrafficPage::Hosts)
    }

    /// The selected conversation, with its place in the capture.
    pub fn flow(&self) -> Option<(usize, &Flow)> {
        let index = *self.flows.get(self.cursors[TrafficPage::Connections.index()])?;
        (self.page == TrafficPage::Connections).then(|| (index, &self.capture.flows[index]))
    }

    /// The selected packet, with its place in the capture.
    pub fn packet(&self) -> Option<(usize, &Packet)> {
        let index = *self.packets.get(self.cursors[TrafficPage::Packets.index()])?;
        (self.page == TrafficPage::Packets).then(|| (index, &self.capture.packets[index]))
    }

    /// The packet's conversation's process, when the capture tells it.
    pub fn packet_pid(&self, packet: &Packet) -> Option<u32> {
        packet.flow.and_then(|flow| self.ends[flow].pid)
    }

    /// What a conversation sent from this machine and what it received, by its place in the
    /// capture.
    pub fn sent_received(&self, flow: usize) -> (u64, u64) {
        let conversation = &self.capture.flows[flow];
        if self.ends[flow].local == conversation.client {
            (conversation.to_server, conversation.to_client)
        } else {
            (conversation.to_client, conversation.to_server)
        }
    }

    /// What a conversation carries: the protocol Borg recognized in it, or its transport.
    pub fn protocol(flow: &Flow) -> &'static str {
        flow.app.unwrap_or(flow.kind.name())
    }

    /// Builds every page's rows from the capture, kept by the filter.
    pub(crate) fn rebuild(&mut self) {
        let filter = NetworkFilter::parse(&self.filter);
        let everything = self.filter.trim().is_empty();
        let kept: Vec<bool> =
            (0..self.capture.flows.len()).map(|flow| everything || filter.matches(&self.flow_haystack(flow))).collect();
        let mut talkers = self.all_talkers();
        let mut host_rows = self.all_hosts();
        self.totals = [talkers.len(), host_rows.len(), self.capture.flows.len(), self.capture.packets.len()];
        if !everything {
            talkers.retain(|talker| filter.matches(&self.talker_haystack(talker)));
            host_rows.retain(|row| filter.matches(&self.host_haystack(row)));
        }
        let mut flows: Vec<usize> = (0..self.capture.flows.len()).filter(|&flow| kept[flow]).collect();
        flows.sort_by_key(|&flow| (Reverse(self.capture.flows[flow].bytes()), flow));
        self.packets = (0..self.capture.packets.len())
            .filter(|&packet| match self.capture.packets[packet].flow {
                Some(flow) => kept[flow],
                None => everything || filter.matches(&self.loose_haystack(&self.capture.packets[packet])),
            })
            .collect();
        talkers.sort_by_key(|talker| (Reverse(talker.bytes), self.name(talker.pid).to_lowercase(), talker.pid));
        host_rows.sort_by_key(|row| (Reverse(row.bytes), row.address));
        (self.talkers, self.host_rows, self.flows) = (talkers, host_rows, flows);
        for page in TrafficPage::ALL {
            let index = page.index();
            self.cursors[index] = self.cursors[index].min(self.len(page).saturating_sub(1));
        }
    }

    fn all_talkers(&self) -> Vec<Talker> {
        let mut talkers: HashMap<Option<u32>, (Talker, BTreeSet<IpAddr>)> = HashMap::new();
        for (index, flow) in self.capture.flows.iter().enumerate() {
            let end = self.ends[index];
            let (talker, hosts) = talkers.entry(end.pid).or_insert_with(|| {
                (Talker { pid: end.pid, packets: 0, bytes: 0, flows: Vec::new(), hosts: 0 }, BTreeSet::new())
            });
            talker.packets += flow.packets.len();
            talker.bytes += flow.bytes();
            talker.flows.push(index);
            hosts.insert(end.remote.ip());
        }
        talkers.into_values().map(|(talker, hosts)| Talker { hosts: hosts.len(), ..talker }).collect()
    }

    fn all_hosts(&self) -> Vec<HostRow> {
        let mut rows: HashMap<IpAddr, HostRow> = HashMap::new();
        for (index, flow) in self.capture.flows.iter().enumerate() {
            let end = self.ends[index];
            let address = end.remote.ip();
            let row = rows.entry(address).or_insert_with(|| HostRow {
                address,
                packets: 0,
                bytes: 0,
                flows: Vec::new(),
                pids: Vec::new(),
                protocols: Vec::new(),
            });
            row.packets += flow.packets.len();
            row.bytes += flow.bytes();
            row.flows.push(index);
            if !row.pids.contains(&end.pid) {
                row.pids.push(end.pid);
            }
            let protocol = Self::protocol(flow);
            if !row.protocols.contains(&protocol) {
                row.protocols.push(protocol);
            }
        }
        rows.into_values().collect()
    }

    fn host_words(&self, address: IpAddr) -> Vec<String> {
        [address.to_string()].into_iter().chain(self.host(address).map(str::to_owned)).collect()
    }

    fn flow_haystack(&self, index: usize) -> Haystack {
        let (flow, end) = (&self.capture.flows[index], self.ends[index]);
        let ports = vec![end.remote.port(), end.local.port()];
        Haystack {
            process: Some(self.name(end.pid)),
            pid: end.pid,
            services: ports.iter().filter_map(|port| service(*port)).collect(),
            ports,
            hosts: self.host_words(end.remote.ip()).into_iter().chain([end.local.ip().to_string()]).collect(),
            protocols: [flow.kind.name()].into_iter().chain(flow.app).collect(),
            labels: flow.server_name.iter().cloned().collect(),
            ..Haystack::default()
        }
    }

    fn talker_haystack(&self, talker: &Talker) -> Haystack {
        let mut haystack = Haystack { process: Some(self.name(talker.pid)), pid: talker.pid, ..Haystack::default() };
        for &flow in &talker.flows {
            let found = self.flow_haystack(flow);
            haystack.ports.extend(found.ports);
            haystack.services.extend(found.services);
            haystack.hosts.extend(found.hosts);
            haystack.protocols.extend(found.protocols);
            haystack.labels.extend(found.labels);
        }
        haystack
    }

    fn host_haystack(&self, row: &HostRow) -> Haystack {
        let mut haystack = Haystack {
            hosts: self.host_words(row.address),
            protocols: row.protocols.clone(),
            labels: row.pids.iter().map(|pid| self.name(*pid)).collect(),
            ..Haystack::default()
        };
        for &flow in &row.flows {
            let end = self.ends[flow];
            haystack.ports.push(end.remote.port());
            haystack.services.extend(service(end.remote.port()));
            haystack.pid = haystack.pid.or(end.pid);
        }
        haystack
    }

    /// The haystack of a packet outside any conversation, such as ARP.
    fn loose_haystack(&self, packet: &Packet) -> Haystack {
        let hosts = packet.layers.ip.map_or_else(Vec::new, |ip| {
            self.host_words(ip.source).into_iter().chain(self.host_words(ip.destination)).collect()
        });
        Haystack { hosts, protocols: vec![packet.layers.protocol()], ..Haystack::default() }
    }

    /// Enter: from a process or host to its conversations, from a conversation to its packets.
    /// Esc comes back.
    pub(crate) fn drill(&mut self) -> bool {
        let next = match self.page {
            TrafficPage::Processes => self.talker().map(|talker| match talker.pid {
                Some(pid) => format!("pid:{pid}"),
                None => self.filter.clone(),
            }),
            TrafficPage::Hosts => self.host_row().map(|row| format!("host:{}", row.address)),
            TrafficPage::Connections => self
                .flow()
                .map(|(index, _)| self.ends[index])
                .map(|end| format!("host:{} port:{}", end.remote.ip(), end.local.port())),
            TrafficPage::Packets => None,
        };
        let Some(filter) = next else { return false };
        let target = match self.page {
            TrafficPage::Connections => TrafficPage::Packets,
            _ => TrafficPage::Connections,
        };
        self.history.push((self.page, self.filter.clone(), self.cursor()));
        self.page = target;
        self.details_top = 0;
        self.set_filter(&filter);
        self.select(0);
        true
    }

    /// Goes back to the page Enter came from, when it came from one.
    pub(crate) fn back(&mut self) -> bool {
        let Some((page, filter, cursor)) = self.history.pop() else { return false };
        self.page = page;
        self.details_top = 0;
        self.set_filter(&filter);
        self.select(cursor);
        true
    }

    /// How far the details can scroll: a packet's bytes, or a row's conversations.
    pub(crate) fn detail_rows(&self) -> usize {
        match self.page {
            TrafficPage::Processes => self.talker().map_or(0, |talker| talker.flows.len()),
            TrafficPage::Hosts => self.host_row().map_or(0, |row| row.flows.len()),
            TrafficPage::Connections => self.flow().map_or(0, |(_, flow)| flow.packets.len()),
            TrafficPage::Packets => self.packet().map_or(0, |(_, packet)| packet.data.len().div_ceil(crate::HEX_WIDTH)),
        }
    }
}

/// The ends of a conversation as this machine sees them, with the process whose socket holds
/// it. A conversation no socket holds counts its server as the other end.
fn ends(flow: &Flow, sockets: &[Socket]) -> Ends {
    let transport = match flow.kind {
        FlowKind::Tcp => Some(Transport::Tcp),
        FlowKind::Udp => Some(Transport::Udp),
        FlowKind::Icmp => None,
    };
    let canonical = |address: SocketAddr| SocketAddr::new(address.ip().to_canonical(), address.port());
    let ways = [(flow.client, flow.server), (flow.server, flow.client)];
    let held = |exact: bool| {
        ways.into_iter().find_map(|(local, remote)| {
            sockets
                .iter()
                .filter(|socket| Some(socket.transport) == transport)
                .find(|socket| {
                    let (bound, peer) = (canonical(socket.local), socket.remote.map(canonical));
                    if exact {
                        bound == canonical(local) && peer == Some(canonical(remote))
                    } else {
                        peer.is_none()
                            && bound.port() == local.port()
                            && (bound.ip().is_unspecified() || bound.ip() == local.ip().to_canonical())
                    }
                })
                .map(|socket| Ends { pid: socket.pid, local, remote })
        })
    };
    held(true).or_else(|| held(false)).unwrap_or(Ends { pid: None, local: flow.client, remote: flow.server })
}

#[cfg(test)]
mod tests;
