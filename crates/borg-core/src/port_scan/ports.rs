//! Which ports a scan tries: the common ones, every one, or a list such as `22,80,8000-8100`, and
//! the names IANA gives them.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const SERVICES: &str = include_str!("services.tsv");
const COLUMN: char = '\t';
const COMMON_MARK: &str = "1";
const ITEM_SEPARATORS: [char; 2] = [',', ' '];
const RANGE_SEPARATOR: char = '-';
const LIST_SEPARATOR: &str = ", ";
pub const FIRST_PORT: u16 = 1;
pub const LAST_PORT: u16 = u16::MAX;

/// A port IANA names for TCP, and whether the common choice tries it.
struct Service {
    port: u16,
    name: &'static str,
    common: bool,
}

fn services() -> &'static [Service] {
    static TABLE: OnceLock<Vec<Service>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<Service> = SERVICES
            .lines()
            .filter_map(|line| {
                let mut columns = line.split(COLUMN);
                let port = columns.next()?.parse().ok()?;
                let name = columns.next()?;
                let common = columns.next()? == COMMON_MARK;
                Some(Service { port, name, common })
            })
            .collect();
        table.sort_unstable_by_key(|service| service.port);
        table
    })
}

/// The name IANA registers for a TCP port.
pub fn port_name(port: u16) -> Option<&'static str> {
    let table = services();
    table.binary_search_by_key(&port, |service| service.port).ok().map(|index| table[index].name)
}

/// The well-known ports IANA names, and the registered ports of services often found on a
/// network, in order.
pub fn common_ports() -> Vec<u16> {
    services().iter().filter(|service| service.common).map(|service| service.port).collect()
}

/// The ports a scan tries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortChoice {
    Common,
    All,
    /// A list as typed, such as `22,80,8000-8100`.
    Custom(String),
}

impl PortChoice {
    pub fn ports(&self) -> Result<Vec<u16>, PortListError> {
        match self {
            Self::Common => Ok(common_ports()),
            Self::All => Ok((FIRST_PORT..=LAST_PORT).collect()),
            Self::Custom(text) => parse_ports(text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortListError {
    Empty,
    /// An item that is no port or range of ports.
    NotAPort(String),
    /// A range whose end comes before its start.
    Backwards(String),
}

/// Reads ports and ranges split by commas or spaces, such as `22,80,8000-8100`, into sorted
/// ports without repeats.
pub fn parse_ports(text: &str) -> Result<Vec<u16>, PortListError> {
    let mut ports = Vec::new();
    for item in text.split(ITEM_SEPARATORS).map(str::trim).filter(|item| !item.is_empty()) {
        let (first, last) = match item.split_once(RANGE_SEPARATOR) {
            Some((first, last)) => (port(first, item)?, port(last, item)?),
            None => (port(item, item)?, port(item, item)?),
        };
        if last < first {
            return Err(PortListError::Backwards(item.to_owned()));
        }
        ports.extend(first..=last);
    }
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() { Err(PortListError::Empty) } else { Ok(ports) }
}

fn port(text: &str, item: &str) -> Result<u16, PortListError> {
    text.trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port >= FIRST_PORT)
        .ok_or_else(|| PortListError::NotAPort(item.to_owned()))
}

/// Ports in a row, from `first` to `last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortRange {
    pub first: u16,
    pub last: u16,
}

impl PortRange {
    pub fn count(self) -> usize {
        usize::from(self.last - self.first) + 1
    }
}

/// Sorted ports as the runs they make.
pub fn ranges(ports: &[u16]) -> Vec<PortRange> {
    let mut ranges: Vec<PortRange> = Vec::new();
    for &port in ports {
        match ranges.last_mut() {
            Some(range) if range.last.checked_add(1) == Some(port) => range.last = port,
            _ => ranges.push(PortRange { first: port, last: port }),
        }
    }
    ranges
}

/// Ranges as a list a person reads, such as `22, 8000-8100`.
pub fn describe(ranges: &[PortRange]) -> String {
    let items = ranges.iter().map(|range| {
        if range.first == range.last {
            range.first.to_string()
        } else {
            format!("{}{RANGE_SEPARATOR}{}", range.first, range.last)
        }
    });
    items.collect::<Vec<_>>().join(LIST_SEPARATOR)
}
