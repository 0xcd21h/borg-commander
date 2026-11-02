//! The sockets Linux lists in `/proc/net`, the processes that hold them from each process's open
//! files, and TCP's byte counts from `ss` where it is installed.

use borg_core::{Socket, TcpState, Transport};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

#[cfg(target_os = "linux")]
const TABLES: [(&str, Transport); 4] = [
    ("/proc/net/tcp", Transport::Tcp),
    ("/proc/net/tcp6", Transport::Tcp),
    ("/proc/net/udp", Transport::Udp),
    ("/proc/net/udp6", Transport::Udp),
];
#[cfg(target_os = "linux")]
const PROCESSES: &str = "/proc";
#[cfg(target_os = "linux")]
const SS: &str = "ss";
#[cfg(target_os = "linux")]
const SS_ARGUMENTS: [&str; 1] = ["-tinHe"];

/// The kernel's codes for TCP's states.
const TCP_STATES: [(&str, TcpState); 11] = [
    ("01", TcpState::Established),
    ("02", TcpState::SynSent),
    ("03", TcpState::SynReceived),
    ("04", TcpState::FinWait1),
    ("05", TcpState::FinWait2),
    ("06", TcpState::TimeWait),
    ("07", TcpState::Closed),
    ("08", TcpState::CloseWait),
    ("09", TcpState::LastAck),
    ("0A", TcpState::Listen),
    ("0B", TcpState::Closing),
];
const SOCKET_LINK: (&str, &str) = ("socket:[", "]");
const INODE_FIELD: &str = "ino:";
const RECEIVED_FIELD: &str = "bytes_received:";
const SENT_FIELD: &str = "bytes_acked:";
const HEX: u32 = 16;
const IPV4_DIGITS: usize = 8;
const IPV6_DIGITS: usize = 32;
const WORD_DIGITS: usize = 8;

/// A socket as the kernel lists it, before its process and byte counts are known.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Listed {
    pub transport: Transport,
    pub local: SocketAddr,
    pub remote: Option<SocketAddr>,
    pub state: Option<TcpState>,
    pub inode: u64,
}

#[cfg(target_os = "linux")]
pub(super) fn read() -> anyhow::Result<Vec<Socket>> {
    let mut listed = Vec::new();
    for (path, transport) in TABLES {
        match std::fs::read_to_string(path) {
            Ok(table) => listed.extend(parse_table(&table, transport)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(anyhow::Error::from(error).context(path)),
        }
    }
    let owners = owners();
    let counts = std::process::Command::new(SS)
        .args(SS_ARGUMENTS)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_counts(&String::from_utf8_lossy(&output.stdout)));
    Ok(assemble(listed, &owners, counts.as_ref()))
}

/// Each socket inode the processes Borg may look into hold, and which process holds it.
#[cfg(target_os = "linux")]
fn owners() -> HashMap<u64, u32> {
    let Ok(processes) = std::fs::read_dir(PROCESSES) else { return HashMap::new() };
    processes
        .flatten()
        .filter_map(|entry| Some((entry.file_name().to_str()?.parse::<u32>().ok()?, entry.path())))
        .flat_map(|(pid, path)| {
            std::fs::read_dir(path.join("fd"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|fd| std::fs::read_link(fd.path()).ok())
                .filter_map(|target| socket_inode(target.to_str()?))
                .map(move |inode| (inode, pid))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The sockets with their processes and, for TCP when `ss` gave them, their byte counts.
pub(super) fn assemble(
    listed: Vec<Listed>,
    owners: &HashMap<u64, u32>,
    counts: Option<&HashMap<u64, (u64, u64)>>,
) -> Vec<Socket> {
    listed
        .into_iter()
        .map(|listed| {
            let counted = counts
                .filter(|_| listed.transport == Transport::Tcp)
                .map(|counts| counts.get(&listed.inode).copied().unwrap_or_default());
            Socket {
                transport: listed.transport,
                local: listed.local,
                remote: listed.remote,
                state: listed.state,
                pid: owners.get(&listed.inode).copied(),
                received: counted.map(|(received, _)| received),
                sent: counted.map(|(_, sent)| sent),
            }
        })
        .collect()
}

/// The inode of a socket an open file links to, as `socket:[12345]`.
pub(super) fn socket_inode(target: &str) -> Option<u64> {
    target.strip_prefix(SOCKET_LINK.0)?.strip_suffix(SOCKET_LINK.1)?.parse().ok()
}

/// The rows of one of `/proc/net`'s tables, after its title line.
pub(super) fn parse_table(table: &str, transport: Transport) -> Vec<Listed> {
    table.lines().skip(1).filter_map(|line| row(line, transport)).collect()
}

fn row(line: &str, transport: Transport) -> Option<Listed> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [_, local, remote, state, _, _, _, _, _, inode, ..] = fields.as_slice() else { return None };
    let remote = endpoint(remote)?;
    Some(Listed {
        transport,
        local: endpoint(local)?,
        remote: Some(remote).filter(|remote| !(remote.ip().is_unspecified() && remote.port() == 0)),
        state: match transport {
            Transport::Tcp => TCP_STATES.iter().find(|(code, _)| code == state).map(|(_, state)| *state),
            Transport::Udp => None,
        },
        inode: inode.parse().ok()?,
    })
}

/// An address as the kernel prints it: the address's bytes in hex, word by word in the
/// machine's order, a colon and the port in hex.
fn endpoint(text: &str) -> Option<SocketAddr> {
    let (address, port) = text.split_once(':')?;
    let port = u16::from_str_radix(port, HEX).ok()?;
    let ip = match address.len() {
        IPV4_DIGITS => IpAddr::V4(Ipv4Addr::from(word(address)?)),
        IPV6_DIGITS => {
            let mut bytes = [0; 16];
            for (index, chunk) in bytes.chunks_mut(WORD_DIGITS / 2).enumerate() {
                chunk.copy_from_slice(&word(address.get(index * WORD_DIGITS..(index + 1) * WORD_DIGITS)?)?);
            }
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

fn word(hex: &str) -> Option<[u8; 4]> {
    u32::from_str_radix(hex, HEX).ok().map(u32::to_ne_bytes)
}

/// The bytes each TCP socket received and sent, by inode, from `ss -tinHe`: a line for each
/// socket with its inode, and an indented line under it with its counters.
pub(super) fn parse_counts(output: &str) -> HashMap<u64, (u64, u64)> {
    let mut counts = HashMap::new();
    let mut inode = None;
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            inode = line.split_whitespace().find_map(|field| field.strip_prefix(INODE_FIELD)?.parse().ok());
            continue;
        }
        let Some(inode) = inode.take() else { continue };
        let field = |name: &str| {
            line.split_whitespace().find_map(|field| field.strip_prefix(name)?.parse::<u64>().ok()).unwrap_or(0)
        };
        counts.insert(inode, (field(RECEIVED_FIELD), field(SENT_FIELD)));
    }
    counts
}
