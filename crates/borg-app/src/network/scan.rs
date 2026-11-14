//! Scans a subnet a few addresses at a time: an ICMP echo through an unprivileged datagram
//! socket where the system allows one, TCP connections to the scanned ports, and an empty UDP
//! datagram that makes the system ask for the host's MAC address.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use borg_core::{Msg, Probe, SCAN_PORTS};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::{Semaphore, mpsc::UnboundedSender},
    task::JoinSet,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const IN_FLIGHT: usize = 16;
const DISCARD_PORT: u16 = 9;
const ECHO_REQUEST: u8 = 8;
const ECHO_REPLY: u8 = 0;
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

/// The scan under way, and the flag that stops it from sending more probes.
pub(super) struct Scan {
    id: u64,
    stop: Arc<AtomicBool>,
}

impl Scan {
    pub(super) fn start(id: u64, addresses: Vec<Ipv4Addr>, messages: UnboundedSender<Msg>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        tokio::spawn(run(id, addresses, stop.clone(), messages));
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

async fn run(id: u64, addresses: Vec<Ipv4Addr>, stop: Arc<AtomicBool>, messages: UnboundedSender<Msg>) {
    let pings = icmp_socket().is_ok();
    let nudge = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await.ok().map(Arc::new);
    let slots = Arc::new(Semaphore::new(IN_FLIGHT));
    let mut probes = JoinSet::new();
    for (sequence, address) in (0..=u16::MAX).zip(addresses) {
        let Ok(slot) = slots.clone().acquire_owned().await else { break };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let (messages, nudge) = (messages.clone(), nudge.clone());
        probes.spawn(async move {
            if let Some(nudge) = nudge {
                let _ = nudge.send_to(&[], SocketAddrV4::new(address, DISCARD_PORT)).await;
            }
            let probe = probe(address, pings.then_some(sequence)).await;
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

fn epoch_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// One address's answers; `sequence` numbers its echo request, when Borg may ping.
async fn probe(address: Ipv4Addr, sequence: Option<u16>) -> Probe {
    let ping = async {
        match sequence {
            Some(sequence) => ping(address, sequence).await,
            None => None,
        }
    };
    let ports = futures::future::join_all(SCAN_PORTS.map(|port| connect(address, port)));
    let (ping, answers) = tokio::join!(ping, ports);
    let open = SCAN_PORTS.iter().zip(&answers).filter(|(_, answer)| **answer == Answer::Open).map(|(port, _)| *port);
    Probe { address, ping, open: open.collect(), refused: answers.contains(&Answer::Refused) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Open,
    Refused,
    Silent,
}

async fn connect(address: Ipv4Addr, port: u16) -> Answer {
    match tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect(SocketAddrV4::new(address, port))).await {
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

async fn ping(address: Ipv4Addr, sequence: u16) -> Option<Duration> {
    let socket = icmp_socket().ok()?;
    let sent = Instant::now();
    socket.send_to(&echo_request(sequence), SocketAddrV4::new(address, 0)).await.ok()?;
    let mut buffer = [0_u8; REPLY_BUFFER];
    let reply = async {
        loop {
            let (length, from) = socket.recv_from(&mut buffer).await.ok()?;
            if from.ip() == IpAddr::V4(address) && echo_reply(&buffer[..length]) == Some(sequence) {
                return Some(sent.elapsed());
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

/// The sequence number of an echo reply, with or without the IPv4 header in front, which
/// macOS keeps and Linux drops.
fn echo_reply(packet: &[u8]) -> Option<u16> {
    let start = match packet.first() {
        Some(first) if first >> NIBBLE_BITS == IPV4_VERSION => usize::from(first & IPV4_WORDS) * WORD_BYTES,
        _ => 0,
    };
    let message = packet.get(start..start + ECHO_HEADER)?;
    (message[0] == ECHO_REPLY).then(|| u16::from_be_bytes([message[SEQUENCE_AT], message[SEQUENCE_AT + 1]]))
}

#[cfg(test)]
mod tests;
