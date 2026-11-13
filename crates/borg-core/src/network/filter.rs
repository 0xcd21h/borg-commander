//! The network screen's filter. Every word must match a row: a plain word anywhere in it,
//! `field:value` in one field, and either one after `!` only where it does not match. A whole
//! address after `host:` means that address alone, so `host:192.168.1.1` leaves out `192.168.1.10`.

use std::net::IpAddr;

const NEGATION: char = '!';
const SEPARATOR: char = ':';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Port,
    Process,
    Pid,
    Host,
    State,
    Protocol,
    Interface,
}

const FIELDS: [(&str, Field); 9] = [
    ("port", Field::Port),
    ("proc", Field::Process),
    ("process", Field::Process),
    ("pid", Field::Pid),
    ("host", Field::Host),
    ("state", Field::State),
    ("proto", Field::Protocol),
    ("if", Field::Interface),
    ("interface", Field::Interface),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Term {
    negated: bool,
    field: Option<Field>,
    value: String,
}

impl Term {
    fn parse(word: &str) -> Option<Self> {
        let (negated, word) = match word.strip_prefix(NEGATION) {
            Some(rest) => (true, rest),
            None => (false, word),
        };
        let known = word.split_once(SEPARATOR).and_then(|(name, value)| {
            let field = FIELDS.iter().find(|(known, _)| known.eq_ignore_ascii_case(name))?.1;
            Some((Some(field), value))
        });
        let (field, value) = known.unwrap_or((None, word));
        (!value.is_empty()).then(|| Self { negated, field, value: value.to_lowercase() })
    }

    fn matches(&self, row: &Haystack) -> bool {
        let contains = |text: &str| text.to_lowercase().contains(&self.value);
        let found = match self.field {
            None => row.words().any(|word| contains(&word)),
            Some(Field::Port) => match self.value.parse::<u16>() {
                Ok(port) => row.ports.contains(&port),
                Err(_) => row.services.iter().any(|service| service.eq_ignore_ascii_case(&self.value)),
            },
            Some(Field::Process) => row.process.as_deref().is_some_and(contains),
            Some(Field::Pid) => row.pid.is_some_and(|pid| pid.to_string() == self.value),
            Some(Field::Host) => match self.value.parse::<IpAddr>() {
                Ok(address) => row.hosts.iter().any(|host| host.parse() == Ok(address)),
                Err(_) => row.hosts.iter().any(|host| contains(host)),
            },
            Some(Field::State) => row.state.is_some_and(contains),
            Some(Field::Protocol) => row.protocols.iter().any(|protocol| protocol.eq_ignore_ascii_case(&self.value)),
            Some(Field::Interface) => row.interface.as_deref().is_some_and(contains),
        };
        found != self.negated
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkFilter {
    terms: Vec<Term>,
}

impl NetworkFilter {
    pub fn parse(text: &str) -> Self {
        Self { terms: text.split_whitespace().filter_map(Term::parse).collect() }
    }

    pub(crate) fn matches(&self, row: &Haystack) -> bool {
        self.terms.iter().all(|term| term.matches(row))
    }
}

/// What a row offers the filter.
#[derive(Debug, Clone, Default)]
pub(crate) struct Haystack {
    pub(crate) process: Option<String>,
    pub(crate) pid: Option<u32>,
    pub(crate) ports: Vec<u16>,
    pub(crate) services: Vec<&'static str>,
    /// Addresses and the names they resolve to.
    pub(crate) hosts: Vec<String>,
    pub(crate) state: Option<&'static str>,
    pub(crate) protocols: Vec<&'static str>,
    pub(crate) interface: Option<String>,
    /// Words a plain search finds that no field holds, such as a label the row shows.
    pub(crate) labels: Vec<String>,
}

impl Haystack {
    fn words(&self) -> impl Iterator<Item = String> + '_ {
        self.process
            .iter()
            .cloned()
            .chain(self.pid.map(|pid| pid.to_string()))
            .chain(self.ports.iter().map(u16::to_string))
            .chain(self.services.iter().map(|s| (*s).to_owned()))
            .chain(self.hosts.iter().cloned())
            .chain(self.state.map(str::to_owned))
            .chain(self.protocols.iter().map(|protocol| (*protocol).to_owned()))
            .chain(self.interface.iter().cloned())
            .chain(self.labels.iter().cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hosts: &[&str]) -> Haystack {
        Haystack { hosts: hosts.iter().map(|host| (*host).to_owned()).collect(), ..Haystack::default() }
    }

    #[test]
    fn a_whole_address_after_host_keeps_that_address_alone() {
        let router = NetworkFilter::parse("host:192.168.1.1");
        assert!(router.matches(&row(&["192.168.1.1", "router.lan"])));
        assert!(!router.matches(&row(&["192.168.1.10"])));
        assert!(!router.matches(&row(&["192.168.1.19", "printer.lan"])));
        assert!(NetworkFilter::parse("host:2001:DB8::1").matches(&row(&["2001:db8::1"])));
        assert!(!NetworkFilter::parse("!host:192.168.1.1").matches(&row(&["192.168.1.1"])));
        assert!(NetworkFilter::parse("!host:192.168.1.1").matches(&row(&["192.168.1.10"])));
    }

    #[test]
    fn part_of_an_address_or_a_name_after_host_still_keeps_whatever_contains_it() {
        assert!(NetworkFilter::parse("host:192.168.1").matches(&row(&["192.168.1.10"])));
        assert!(NetworkFilter::parse("host:Example").matches(&row(&["203.0.113.9", "www.example.com"])));
        assert!(!NetworkFilter::parse("host:example").matches(&row(&["203.0.113.9"])));
    }
}
