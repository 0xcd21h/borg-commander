//! What the network screen reads of the machine: every socket with its process, and every
//! interface with its counters.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Transport {
    Tcp,
    Udp,
}

impl Transport {
    /// The protocol's name, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TcpState {
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

const TCP_STATES: [(TcpState, &str); 11] = [
    (TcpState::Listen, "LISTEN"),
    (TcpState::SynSent, "SYN_SENT"),
    (TcpState::SynReceived, "SYN_RECEIVED"),
    (TcpState::Established, "ESTABLISHED"),
    (TcpState::FinWait1, "FIN_WAIT_1"),
    (TcpState::FinWait2, "FIN_WAIT_2"),
    (TcpState::CloseWait, "CLOSE_WAIT"),
    (TcpState::Closing, "CLOSING"),
    (TcpState::LastAck, "LAST_ACK"),
    (TcpState::TimeWait, "TIME_WAIT"),
    (TcpState::Closed, "CLOSED"),
];

impl TcpState {
    /// The state's name as `netstat` prints it, the same in every language.
    pub fn name(self) -> &'static str {
        TCP_STATES.iter().find(|(state, _)| *state == self).map_or("", |(_, name)| name)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        TCP_STATES.iter().find(|(_, known)| *known == name).map(|(state, _)| *state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socket {
    pub transport: Transport,
    pub local: SocketAddr,
    /// The other end, for a connection; a listening socket has none.
    pub remote: Option<SocketAddr>,
    /// TCP's state; UDP has none.
    pub state: Option<TcpState>,
    /// The process that holds the socket, when the system shows it to Borg.
    pub pid: Option<u32>,
    /// Bytes received over the socket's life, when the system counts them.
    pub received: Option<u64>,
    pub sent: Option<u64>,
}

impl Socket {
    /// Whether the socket waits for others: TCP listening, or UDP bound to a port with no peer.
    pub fn listens(&self) -> bool {
        match self.transport {
            Transport::Tcp => self.state == Some(TcpState::Listen),
            Transport::Udp => self.remote.is_none() && self.local.port() != 0,
        }
    }

    /// Whether the socket talks to one other end.
    pub fn connects(&self) -> bool {
        self.remote.is_some() && !self.listens()
    }

    /// Whether this TCP connection came in through a port listening on `address`.
    pub(crate) fn came_through(&self, port: PortKey, address: IpAddr) -> bool {
        self.transport == Transport::Tcp
            && port.transport == Transport::Tcp
            && self.connects()
            && self.local.port() == port.port
            && (address.is_unspecified() || address == self.local.ip())
    }

    /// The port this socket listens on, with its process.
    pub fn port_key(&self) -> PortKey {
        PortKey { transport: self.transport, port: self.local.port(), pid: self.pid }
    }

    pub fn key(&self) -> SocketKey {
        SocketKey { transport: self.transport, local: self.local, remote: self.remote, pid: self.pid }
    }
}

/// What tells a listening port apart: its protocol, number and process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortKey {
    pub transport: Transport,
    pub port: u16,
    pub pid: Option<u32>,
}

/// What tells a socket apart from one sample to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketKey {
    pub transport: Transport,
    pub local: SocketAddr,
    pub remote: Option<SocketAddr>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceKind {
    Loopback,
    WiFi,
    Ethernet,
    Tunnel,
    Bridge,
    Virtual,
    Other,
}

const LOOPBACK_NAMES: [&str; 2] = ["lo", "lo0"];
const NAME_PREFIXES: [(&str, InterfaceKind); 32] = [
    ("utun", InterfaceKind::Tunnel),
    ("tun", InterfaceKind::Tunnel),
    ("tap", InterfaceKind::Tunnel),
    ("wg", InterfaceKind::Tunnel),
    ("ppp", InterfaceKind::Tunnel),
    ("ipsec", InterfaceKind::Tunnel),
    ("gif", InterfaceKind::Tunnel),
    ("stf", InterfaceKind::Tunnel),
    ("tailscale", InterfaceKind::Tunnel),
    ("zt", InterfaceKind::Tunnel),
    ("bridge", InterfaceKind::Bridge),
    ("br", InterfaceKind::Bridge),
    ("docker", InterfaceKind::Bridge),
    ("virbr", InterfaceKind::Bridge),
    ("cni", InterfaceKind::Bridge),
    ("lxcbr", InterfaceKind::Bridge),
    ("veth", InterfaceKind::Virtual),
    ("vmnet", InterfaceKind::Virtual),
    ("vmenet", InterfaceKind::Virtual),
    ("vboxnet", InterfaceKind::Virtual),
    ("awdl", InterfaceKind::Virtual),
    ("llw", InterfaceKind::Virtual),
    ("anpi", InterfaceKind::Virtual),
    ("ap", InterfaceKind::Virtual),
    ("vxlan", InterfaceKind::Virtual),
    ("flannel", InterfaceKind::Virtual),
    ("cali", InterfaceKind::Virtual),
    ("dummy", InterfaceKind::Virtual),
    ("wl", InterfaceKind::WiFi),
    ("en", InterfaceKind::Ethernet),
    ("eth", InterfaceKind::Ethernet),
    ("em", InterfaceKind::Ethernet),
];

impl InterfaceKind {
    /// The kind an interface's name suggests, for interfaces the system says nothing more about.
    pub fn from_name(name: &str) -> Self {
        if LOOPBACK_NAMES.contains(&name) {
            return Self::Loopback;
        }
        NAME_PREFIXES.iter().find(|(prefix, _)| name.starts_with(prefix)).map_or(Self::Other, |(_, kind)| *kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub kind: InterfaceKind,
    /// Whether it can pass packets now.
    pub up: bool,
    /// Its addresses with their prefix lengths.
    pub addresses: Vec<(IpAddr, u8)>,
    pub mac: Option<String>,
    pub mtu: Option<u64>,
    /// Bytes and packets since the system started counting, and the errors in both directions.
    pub received: u64,
    pub sent: u64,
    pub packets_received: u64,
    pub packets_sent: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSample {
    pub sockets: Vec<Socket>,
    pub interfaces: Vec<Interface>,
    /// The name of each process that holds a socket.
    pub names: HashMap<u32, String>,
}

/// The services best known by their port, the same in every language.
const SERVICES: [(u16, &str); 34] = [
    (20, "ftp-data"),
    (21, "ftp"),
    (22, "ssh"),
    (23, "telnet"),
    (25, "smtp"),
    (53, "dns"),
    (67, "dhcp"),
    (68, "dhcp"),
    (80, "http"),
    (110, "pop3"),
    (123, "ntp"),
    (137, "netbios"),
    (143, "imap"),
    (161, "snmp"),
    (389, "ldap"),
    (443, "https"),
    (445, "smb"),
    (465, "smtps"),
    (587, "submission"),
    (631, "ipp"),
    (853, "dns-over-tls"),
    (993, "imaps"),
    (995, "pop3s"),
    (1883, "mqtt"),
    (3306, "mysql"),
    (3389, "rdp"),
    (5353, "mdns"),
    (5432, "postgresql"),
    (5672, "amqp"),
    (5900, "vnc"),
    (6379, "redis"),
    (8080, "http-alt"),
    (9200, "elasticsearch"),
    (27017, "mongodb"),
];

/// The service usually found on `port`.
pub fn service(port: u16) -> Option<&'static str> {
    SERVICES.iter().find(|(known, _)| *known == port).map(|(_, name)| *name)
}
