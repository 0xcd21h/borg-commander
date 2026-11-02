//! Traffic rates worked out from the byte counts of two samples a moment apart.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
};

use super::{NetworkSample, PortKey, Socket, SocketKey, Transport};
use crate::HISTORY;

/// Rates shown are the mean of this many samples, so a list ranked by them holds still.
pub const SMOOTHING: usize = 3;

/// Bytes in each direction, or bytes a second.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rate {
    pub down: u64,
    pub up: u64,
}

impl Rate {
    pub fn total(self) -> u64 {
        self.down.saturating_add(self.up)
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        Self { down: self.down.saturating_add(other.down), up: self.up.saturating_add(other.up) }
    }

    /// What moved since `before`. A count lower than before belongs to a socket that took the
    /// place of another, and all of it is new.
    fn since(self, before: Self) -> Self {
        Self {
            down: self.down.checked_sub(before.down).unwrap_or(self.down),
            up: self.up.checked_sub(before.up).unwrap_or(self.up),
        }
    }

    fn per_second(self, seconds: f64) -> Self {
        Self { down: (self.down as f64 / seconds) as u64, up: (self.up as f64 / seconds) as u64 }
    }
}

/// One thing's traffic: its rate each second for the last minute, and what it moved since the
/// screen opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meter {
    pub history: VecDeque<Rate>,
    pub moved: Rate,
}

impl Meter {
    fn push(&mut self, moved: Rate, seconds: f64) {
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(moved.per_second(seconds));
        self.moved = self.moved.plus(moved);
    }

    /// The rate over the last few samples.
    pub fn rate(&self) -> Rate {
        let recent: Vec<_> = self.history.iter().rev().take(SMOOTHING).collect();
        let count = recent.len().max(1) as u64;
        let sum = recent.into_iter().fold(Rate::default(), |sum, rate| sum.plus(*rate));
        Rate { down: sum.down / count, up: sum.up / count }
    }

    pub fn downloads(&self) -> Vec<u64> {
        self.history.iter().map(|rate| rate.down).collect()
    }

    pub fn uploads(&self) -> Vec<u64> {
        self.history.iter().map(|rate| rate.up).collect()
    }
}

/// The traffic of each socket, process and interface, from the samples taken so far.
#[derive(Debug, Clone, Default)]
pub struct Traffic {
    sockets_before: HashMap<SocketKey, Rate>,
    interfaces_before: HashMap<String, Rate>,
    pub sockets: HashMap<SocketKey, Meter>,
    /// Each listening port's own traffic and that of the connections that came in through it.
    pub ports: HashMap<PortKey, Meter>,
    /// Keyed by PID; sockets whose process Borg cannot see count under `None`.
    pub processes: HashMap<Option<u32>, Meter>,
    pub interfaces: HashMap<String, Meter>,
    sampled: bool,
}

impl Traffic {
    /// Takes in a sample taken `seconds` after the one before. The first sample only gives
    /// the counts the next one is measured against.
    pub(crate) fn take(&mut self, sample: &NetworkSample, seconds: f64) {
        let counted = self.sampled && seconds > 0.0;
        let mut counts: HashMap<SocketKey, Rate> = HashMap::new();
        for socket in &sample.sockets {
            if let (Some(down), Some(up)) = (socket.received, socket.sent) {
                let count = counts.entry(socket.key()).or_default();
                *count = count.plus(Rate { down, up });
            }
        }
        let moved: HashMap<SocketKey, Rate> = counts
            .iter()
            .map(|(key, count)| (*key, self.sockets_before.get(key).map_or(*count, |before| count.since(*before))))
            .collect();
        let mut processes: HashMap<Option<u32>, Rate> =
            sample.sockets.iter().map(|socket| (socket.pid, Rate::default())).collect();
        for (key, moved) in &moved {
            let process = processes.entry(key.pid).or_default();
            *process = process.plus(*moved);
        }
        let ports = port_traffic(sample, &moved);
        let interface_counts: HashMap<String, Rate> = sample
            .interfaces
            .iter()
            .map(|interface| (interface.name.clone(), Rate { down: interface.received, up: interface.sent }))
            .collect();
        let interfaces = interface_counts
            .iter()
            .map(|(name, count)| {
                (
                    name.clone(),
                    self.interfaces_before.get(name).map_or_else(Rate::default, |before| count.since(*before)),
                )
            })
            .collect();
        push_all(&mut self.sockets, moved, seconds, counted);
        push_all(&mut self.processes, processes, seconds, counted);
        push_all(&mut self.ports, ports, seconds, counted);
        push_all(&mut self.interfaces, interfaces, seconds, counted);
        self.sockets_before = counts;
        self.interfaces_before = interface_counts;
        self.sampled = true;
    }

    pub fn socket(&self, key: &SocketKey) -> Rate {
        self.sockets.get(key).map(Meter::rate).unwrap_or_default()
    }

    pub fn port(&self, port: PortKey) -> Rate {
        self.ports.get(&port).map(Meter::rate).unwrap_or_default()
    }

    pub fn process(&self, pid: Option<u32>) -> Rate {
        self.processes.get(&pid).map(Meter::rate).unwrap_or_default()
    }

    pub fn interface(&self, name: &str) -> Rate {
        self.interfaces.get(name).map(Meter::rate).unwrap_or_default()
    }
}

/// Drops the meters of what has gone and, when the sample counts, adds what each one moved.
fn push_all<K: Hash + Eq>(meters: &mut HashMap<K, Meter>, moved: HashMap<K, Rate>, seconds: f64, counted: bool) {
    meters.retain(|key, _| moved.contains_key(key));
    if counted {
        for (key, moved) in moved {
            meters.entry(key).or_default().push(moved, seconds);
        }
    }
}

/// What moved through each listening port: a UDP port's own traffic, and a TCP port's
/// connections that came in through it.
fn port_traffic(sample: &NetworkSample, moved: &HashMap<SocketKey, Rate>) -> HashMap<PortKey, Rate> {
    let mut listeners: Vec<&Socket> = sample.sockets.iter().filter(|socket| socket.listens()).collect();
    listeners.sort_by_key(|listener| (listener.port_key(), listener.local));
    let mut ports: HashMap<PortKey, Rate> =
        listeners.iter().map(|listener| (listener.port_key(), Rate::default())).collect();
    let mut seen = HashSet::new();
    for socket in sample.sockets.iter().filter(|socket| seen.insert(socket.key())) {
        let port = if socket.listens() {
            (socket.transport == Transport::Udp).then_some(socket.port_key())
        } else {
            listeners
                .iter()
                .find(|listener| socket.came_through(listener.port_key(), listener.local.ip()))
                .map(|listener| listener.port_key())
        };
        if let (Some(port), Some(moved)) = (port, moved.get(&socket.key())) {
            let total = ports.entry(port).or_default();
            *total = total.plus(*moved);
        }
    }
    ports
}
