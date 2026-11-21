//! Scans a subnet a few addresses at a time: an ICMP echo through an unprivileged datagram
//! socket where the system allows one, TCP connections to the scanned ports, and on the local
//! link an empty UDP datagram that makes the system ask for the host's MAC address. A discovery
//! sends the likely routers of many networks the same echo and two connections, then traces the
//! way to each one that answers with probes whose time to live grows one hop at a time.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use borg_core::{MAX_HOPS, Msg, Probe, SCAN_PORTS, Trace};
use tokio::{
    net::{TcpSocket, TcpStream, UdpSocket},
    sync::{Semaphore, mpsc::UnboundedSender},
    task::JoinSet,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const SUBNET_IN_FLIGHT: usize = 16;
const DISCOVERY_IN_FLIGHT: usize = 64;
/// The ports a discovery tries where ping gets no answer: the web interfaces routers have.
const DISCOVERY_PORTS: [u16; 2] = [80, 443];
/// The echo sequence numbers each address gets: one for its ping and one per traced hop.
const SEQUENCES_PER_ADDRESS: usize = 16;
const DISCARD_PORT: u16 = 9;
const ECHO_REQUEST: u8 = 8;
const ECHO_REPLY: u8 = 0;
const TIME_EXCEEDED: u8 = 11;
const IPV4_DESTINATION_AT: usize = 16;
const IPV4_ADDRESS_BYTES: usize = 4;
const ECHO_HEADER: usize = 8;
const CHECKSUM_AT: usize = 2;
const SEQUENCE_AT: usize = 6;
const ECHO_PAYLOAD: &[u8; 8] = b"borgscan";
const ECHO_LENGTH: usize = ECHO_HEADER + ECHO_PAYLOAD.len();
const IPV4_VERSION: u8 = 4;
const NIBBLE_BITS: u8 = 4;
const IPV4_WORDS: u8 = 0x0f;
const WORD_BYTES: usize = 4;
const REPLY_BUFFER: usize = 1500;

/// What a scan sends each address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// A subnet's hosts: ping and the scanned ports, and the nudge on the local link.
    Subnet { nudge: bool },
    /// The likely routers of other networks: ping and the discovery ports, then a trace.
    Discovery,
}

impl Plan {
    fn in_flight(self) -> usize {
        match self {
            Self::Subnet { .. } => SUBNET_IN_FLIGHT,
            Self::Discovery => DISCOVERY_IN_FLIGHT,
        }
    }

    fn ports(self) -> &'static [u16] {
        match self {
            Self::Subnet { .. } => &SCAN_PORTS,
            Self::Discovery => &DISCOVERY_PORTS,
        }
    }
}

/// The scan under way, and the flag that stops it from sending more probes.
pub(super) struct Scan {
    id: u64,
    stop: Arc<AtomicBool>,
}

impl Scan {
    pub(super) fn start(id: u64, addresses: Vec<Ipv4Addr>, plan: Plan, messages: UnboundedSender<Msg>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        tokio::spawn(run(id, addresses, plan, stop.clone(), messages));
        Self { id, stop }
    }

    pub(super) fn stop(&self, id: u64) {
        if id == self.id {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn abort(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

async fn run(id: u64, addresses: Vec<Ipv4Addr>, plan: Plan, stop: Arc<AtomicBool>, messages: UnboundedSender<Msg>) {
    let pings = icmp_socket().is_ok();
    let nudge = match plan {
        Plan::Subnet { nudge: true } => {
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await.ok().map(Arc::new)
        }
        Plan::Subnet { nudge: false } | Plan::Discovery => None,
    };
    let slots = Arc::new(Semaphore::new(plan.in_flight()));
    let mut probes = JoinSet::new();
    for (sequence, address) in (0..=u16::MAX).step_by(SEQUENCES_PER_ADDRESS).zip(addresses) {
        let Ok(slot) = slots.clone().acquire_owned().await else { break };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let (messages, nudge) = (messages.clone(), nudge.clone());
        probes.spawn(async move {
            if let Some(nudge) = nudge {
                let _ = nudge.send_to(&[], SocketAddrV4::new(address, DISCARD_PORT)).await;
            }
            let mut probe = probe(address, pings.then_some(sequence), plan.ports()).await;
            if plan == Plan::Discovery && probe.answered() {
                probe.trace = Some(trace(&probe, pings.then_some(sequence), plan.ports()).await);
            }
            drop(slot);
            let _ = messages.send(Msg::Probed { id, probe, at: epoch_seconds() });
        });
    }
    let mut result = Ok(());
    while let Some(joined) = probes.join_next().await {
        if let Err(error) = joined {
            result = Err(error.to_string());
        }
    }
    let _ = messages.send(Msg::ScanFinished { id, result });
}

pub(crate) fn epoch_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// One address's answers; `sequence` numbers its echo request, when Borg may ping.
async fn probe(address: Ipv4Addr, sequence: Option<u16>, ports: &[u16]) -> Probe {
    let ping = async {
        match sequence {
            Some(sequence) => ping(address, sequence, None).await.and_then(Heard::reply),
            None => None,
        }
    };
    let connects = futures::future::join_all(ports.iter().map(|port| connect(address, *port, None)));
    let (ping, answers) = tokio::join!(ping, connects);
    let open = ports.iter().zip(&answers).filter(|(_, answer)| **answer == Answer::Open).map(|(port, _)| *port);
    Probe { address, ping, open: open.collect(), refused: answers.contains(&Answer::Refused), trace: None }
}

/// How many hops away an address that answered is, from echoes or else connections whose time
/// to live grows one hop at a time, and the routers on the way that said they dropped the
/// shorter echoes.
async fn trace(probe: &Probe, sequence: Option<u16>, ports: &[u16]) -> Trace {
    let address = probe.address;
    let ttls = 1..=MAX_HOPS;
    let heard: Vec<Option<Heard>> = match (sequence, probe.ping) {
        (Some(sequence), Some(_)) => {
            let echoes = ttls.map(|ttl| ping(address, sequence + u16::from(ttl), Some(ttl)));
            futures::future::join_all(echoes).await
        }
        _ => {
            let Some(port) = answering_port(probe, ports).await else { return Trace::default() };
            let connects = ttls.map(|ttl| connect(address, port, Some(ttl)));
            let answers = futures::future::join_all(connects).await;
            answers.into_iter().map(|answer| (answer != Answer::Silent).then_some(Heard::Reached)).collect()
        }
    };
    path(&heard)
}

/// The port to trace through: one the probe found open, else one that refused.
async fn answering_port(probe: &Probe, ports: &[u16]) -> Option<u16> {
    if let Some(open) = probe.open.first() {
        return Some(*open);
    }
    for port in ports {
        if connect(probe.address, *port, None).await == Answer::Refused {
            return Some(*port);
        }
    }
    None
}

/// The trace that what each time to live heard tells: the first one that reached the address
/// is the hop count, and the routers before it are the path.
fn path(heard: &[Option<Heard>]) -> Trace {
    let reached = heard.iter().position(|heard| matches!(heard, Some(Heard::Reached)));
    let before = &heard[..reached.unwrap_or(heard.len())];
    let path = before
        .iter()
        .filter_map(|heard| match heard {
            Some(Heard::Router(router)) => Some(*router),
            Some(Heard::Reached | Heard::Replied(_)) | None => None,
        })
        .collect();
    Trace {
        hops: reached.and_then(|index| u8::try_from(index + 1).ok()),
        path: reached.map_or_else(Vec::new, |_| path),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Open,
    Refused,
    Silent,
}

/// A TCP connection to the port, limited to `ttl` hops when one is given.
async fn connect(address: Ipv4Addr, port: u16, ttl: Option<u8>) -> Answer {
    let target = std::net::SocketAddr::V4(SocketAddrV4::new(address, port));
    let connecting = async {
        match ttl {
            Some(ttl) => {
                let socket = TcpSocket::new_v4()?;
                rustix::net::sockopt::set_ip_ttl(&socket, u32::from(ttl))?;
                socket.connect(target).await
            }
            None => TcpStream::connect(target).await,
        }
    };
    match tokio::time::timeout(PROBE_TIMEOUT, connecting).await {
        Ok(Ok(_)) => Answer::Open,
        Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => Answer::Refused,
        _ => Answer::Silent,
    }
}

fn icmp_socket() -> io::Result<UdpSocket> {
    use rustix::net::{AddressFamily, SocketType, ipproto, socket};
    let socket = std::net::UdpSocket::from(socket(AddressFamily::INET, SocketType::DGRAM, Some(ipproto::ICMP))?);
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket)
}

/// What came back for an echo: its reply after a while, or a router on the way that dropped it
/// when its time to live ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    Replied(Duration),
    Reached,
    Router(Ipv4Addr),
}

impl Heard {
    fn reply(self) -> Option<Duration> {
        match self {
            Self::Replied(after) => Some(after),
            Self::Reached | Self::Router(_) => None,
        }
    }
}

/// An echo to `address`, limited to `ttl` hops when one is given. A limited echo that comes back
/// says it reached the address.
async fn ping(address: Ipv4Addr, sequence: u16, ttl: Option<u8>) -> Option<Heard> {
    let socket = icmp_socket().ok()?;
    if let Some(ttl) = ttl {
        rustix::net::sockopt::set_ip_ttl(&socket, u32::from(ttl)).ok()?;
    }
    let sent = Instant::now();
    socket.send_to(&echo_request(sequence), SocketAddrV4::new(address, 0)).await.ok()?;
    let mut buffer = [0_u8; REPLY_BUFFER];
    let reply = async {
        loop {
            let (length, from) = socket.recv_from(&mut buffer).await.ok()?;
            let IpAddr::V4(from) = from.ip() else { continue };
            match icmp_answer(&buffer[..length]) {
                Some(Icmp::Reply(heard)) if heard == sequence && from == address => {
                    return Some(if ttl.is_some() { Heard::Reached } else { Heard::Replied(sent.elapsed()) });
                }
                Some(Icmp::Dropped { sequence: heard, destination }) if heard == sequence && destination == address => {
                    return Some(Heard::Router(from));
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT, reply).await.ok().flatten()
}

fn echo_request(sequence: u16) -> [u8; ECHO_LENGTH] {
    let mut packet = [0_u8; ECHO_LENGTH];
    packet[0] = ECHO_REQUEST;
    packet[SEQUENCE_AT..ECHO_HEADER].copy_from_slice(&sequence.to_be_bytes());
    packet[ECHO_HEADER..].copy_from_slice(ECHO_PAYLOAD);
    let sum = checksum(&packet);
    packet[CHECKSUM_AT..CHECKSUM_AT + 2].copy_from_slice(&sum.to_be_bytes());
    packet
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = bytes
        .chunks(2)
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or_default()])))
        .sum();
    while sum > u32::from(u16::MAX) {
        sum = (sum & u32::from(u16::MAX)) + (sum >> u16::BITS);
    }
    !u16::try_from(sum).unwrap_or(u16::MAX)
}

/// An ICMP message Borg reads: an echo reply, or a router's word that it dropped an echo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Icmp {
    Reply(u16),
    Dropped { sequence: u16, destination: Ipv4Addr },
}

/// The length of the IPv4 header a packet starts with, if it starts with one.
fn ipv4_header(packet: &[u8]) -> usize {
    match packet.first() {
        Some(first) if first >> NIBBLE_BITS == IPV4_VERSION => usize::from(first & IPV4_WORDS) * WORD_BYTES,
        _ => 0,
    }
}

fn sequence(message: &[u8]) -> u16 {
    u16::from_be_bytes([message[SEQUENCE_AT], message[SEQUENCE_AT + 1]])
}

/// The ICMP message in a packet, with or without the IPv4 header in front, which macOS keeps and
/// Linux drops. A dropped echo comes back inside a time exceeded message with its own header.
fn icmp_answer(packet: &[u8]) -> Option<Icmp> {
    let message = &packet[ipv4_header(packet)..];
    let header = message.get(..ECHO_HEADER)?;
    match header[0] {
        ECHO_REPLY => Some(Icmp::Reply(sequence(header))),
        TIME_EXCEEDED => {
            let original = &message[ECHO_HEADER..];
            let inner = ipv4_header(original);
            let destination = original.get(IPV4_DESTINATION_AT..IPV4_DESTINATION_AT + IPV4_ADDRESS_BYTES)?;
            let echo = original.get(inner..inner + ECHO_HEADER)?;
            let destination = Ipv4Addr::from(<[u8; IPV4_ADDRESS_BYTES]>::try_from(destination).ok()?);
            (inner > 0 && echo[0] == ECHO_REQUEST).then(|| Icmp::Dropped { sequence: sequence(echo), destination })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
