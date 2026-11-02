//! The sockets `netstat -anvbW` lists on macOS, each with its byte counts and the process that
//! holds it.

use borg_core::{Socket, TcpState, Transport};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

#[cfg(target_os = "macos")]
pub(super) const PROGRAM: &str = "/usr/sbin/netstat";
#[cfg(target_os = "macos")]
pub(super) const ARGUMENTS: [&str; 1] = ["-anvbW"];

/// Fields after `process:pid` on each line.
const TRAILING_FIELDS: usize = 8;
/// What `netstat` prints for an address or port it does not know.
const ANY: &str = "*";

/// Which addresses a socket line's `*` stands for.
#[derive(Clone, Copy)]
enum Family {
    V4,
    V6,
    Both,
}

const PROTOCOLS: [(&str, Transport, Family); 6] = [
    ("tcp4", Transport::Tcp, Family::V4),
    ("tcp6", Transport::Tcp, Family::V6),
    ("tcp46", Transport::Tcp, Family::Both),
    ("udp4", Transport::Udp, Family::V4),
    ("udp6", Transport::Udp, Family::V6),
    ("udp46", Transport::Udp, Family::Both),
];

/// Every TCP and UDP socket in `output`, and the name `netstat` gives each process.
pub(super) fn parse(output: &str) -> (Vec<Socket>, HashMap<u32, String>) {
    let mut names = HashMap::new();
    let sockets = output
        .lines()
        .filter_map(line)
        .map(|(socket, name)| {
            if let (Some(pid), Some(name)) = (socket.pid, name) {
                names.entry(pid).or_insert(name);
            }
            socket
        })
        .collect();
    (sockets, names)
}

fn line(line: &str) -> Option<(Socket, Option<String>)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (&protocol, rest) = fields.split_first()?;
    let (_, transport, family) = PROTOCOLS.iter().find(|(name, _, _)| *name == protocol)?;
    let [_, _, local, remote, rest @ ..] = rest else { return None };
    let (state, counts) = match transport {
        Transport::Tcp => {
            let (state, counts) = rest.split_first()?;
            (TcpState::from_name(state), counts)
        }
        Transport::Udp => (None, rest),
    };
    let [received, sent, _, _, owner @ ..] = counts else { return None };
    let owner = owner.get(..owner.len().checked_sub(TRAILING_FIELDS)?)?.join(" ");
    let (name, pid) = owner
        .rsplit_once(':')
        .map_or((None, None), |(name, pid)| (Some(name.to_owned()).filter(|name| !name.is_empty()), pid.parse().ok()));
    let socket = Socket {
        transport: *transport,
        local: endpoint(local, *family)?,
        remote: endpoint(remote, *family).filter(|remote| !(remote.ip().is_unspecified() && remote.port() == 0)),
        state,
        pid,
        received: received.parse().ok(),
        sent: sent.parse().ok(),
    };
    Some((socket, name))
}

/// An address as `netstat` prints it: the host, a dot and the port, with `*` for any.
fn endpoint(text: &str, family: Family) -> Option<SocketAddr> {
    let (host, port) = text.rsplit_once('.')?;
    let port = if port == ANY { 0 } else { port.parse().ok()? };
    let ip = match (host, family) {
        (ANY, Family::V4) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        (ANY, Family::V6 | Family::Both) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        (host, _) => host.split_once('%').map_or(host, |(address, _)| address).parse().ok()?,
    };
    Some(SocketAddr::new(ip, port))
}
