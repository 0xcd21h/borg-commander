//! TLS key logs, the text `SSLKEYLOGFILE` makes browsers, curl and other programs write: a line
//! for each secret, naming it, the client random of the connection it belongs to, and the
//! secret, both in hex.

use std::collections::{HashMap, HashSet};

/// Bytes of a ClientHello's random, which names a connection in a key log.
pub const RANDOM: usize = 32;
const COMMENT: char = '#';
const MASTER: &str = "CLIENT_RANDOM";
const CLIENT_EARLY: &str = "CLIENT_EARLY_TRAFFIC_SECRET";
const CLIENT_HANDSHAKE: &str = "CLIENT_HANDSHAKE_TRAFFIC_SECRET";
const SERVER_HANDSHAKE: &str = "SERVER_HANDSHAKE_TRAFFIC_SECRET";
const CLIENT_TRAFFIC: &str = "CLIENT_TRAFFIC_SECRET_0";
const SERVER_TRAFFIC: &str = "SERVER_TRAFFIC_SECRET_0";

pub type Random = [u8; RANDOM];

/// One connection's secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    /// TLS 1.2's master secret.
    pub master: Option<Vec<u8>>,
    pub client_early: Option<Vec<u8>>,
    pub client_handshake: Option<Vec<u8>>,
    pub server_handshake: Option<Vec<u8>>,
    pub client_traffic: Option<Vec<u8>>,
    pub server_traffic: Option<Vec<u8>>,
}

impl Secrets {
    fn slot(&mut self, label: &str) -> Option<&mut Option<Vec<u8>>> {
        match label {
            MASTER => Some(&mut self.master),
            CLIENT_EARLY => Some(&mut self.client_early),
            CLIENT_HANDSHAKE => Some(&mut self.client_handshake),
            SERVER_HANDSHAKE => Some(&mut self.server_handshake),
            CLIENT_TRAFFIC => Some(&mut self.client_traffic),
            SERVER_TRAFFIC => Some(&mut self.server_traffic),
            _ => None,
        }
    }
}

/// The secrets a key log gives, by the client random of their connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyLog {
    connections: HashMap<Random, Secrets>,
}

impl KeyLog {
    /// The secrets in a key log's text. Lines Borg does not use, or cannot read, are left out.
    pub fn parse(text: &str) -> Self {
        let mut log = Self::default();
        log.add(text);
        log
    }

    pub fn add(&mut self, text: &str) {
        for (label, random, secret) in text.lines().filter_map(entry) {
            if let Some(slot) = self.connections.entry(random).or_default().slot(label) {
                *slot = Some(secret);
            }
        }
        self.connections.retain(|_, secrets| *secrets != Secrets::default());
    }

    pub fn get(&self, random: &Random) -> Option<&Secrets> {
        self.connections.get(random)
    }

    /// How many connections it has secrets for.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

/// The lines of a key log's text whose secrets belong to one of `randoms`' connections.
pub fn lines_for(text: &str, randoms: &HashSet<Random>) -> String {
    text.lines().filter(|line| entry(line).is_some_and(|(_, random, _)| randoms.contains(&random))).fold(
        String::new(),
        |mut kept, line| {
            kept.push_str(line);
            kept.push('\n');
            kept
        },
    )
}

/// A line's label, client random and secret, unless it is a comment or not a secret.
fn entry(line: &str) -> Option<(&str, Random, Vec<u8>)> {
    let line = line.trim();
    if line.starts_with(COMMENT) {
        return None;
    }
    let mut words = line.split_ascii_whitespace();
    let (label, random, secret) = (words.next()?, words.next()?, words.next()?);
    if words.next().is_some() || Secrets::default().slot(label).is_none() {
        return None;
    }
    Some((label, hex(random)?.try_into().ok()?, hex(secret).filter(|secret| !secret.is_empty())?))
}

fn hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANDOM_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RANDOM_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn reads_each_secret_by_its_connection() {
        let text = format!(
            "# written by a browser\nCLIENT_RANDOM {RANDOM_A} 0102\n\nSERVER_TRAFFIC_SECRET_0 {RANDOM_B} 0a0b\nCLIENT_TRAFFIC_SECRET_0 {RANDOM_B} 0c0d\n"
        );
        let log = KeyLog::parse(&text);
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(&[0xaa; RANDOM]).and_then(|secrets| secrets.master.clone()), Some(vec![1, 2]));
        let b = log.get(&[0xbb; RANDOM]).unwrap();
        assert_eq!((b.server_traffic.clone(), b.client_traffic.clone()), (Some(vec![10, 11]), Some(vec![12, 13])));
    }

    #[test]
    fn leaves_out_lines_it_cannot_use() {
        let text = format!(
            "EXPORTER_SECRET {RANDOM_A} 0102\nCLIENT_RANDOM {RANDOM_A}\nCLIENT_RANDOM abcd 0102\nCLIENT_RANDOM {RANDOM_A} 0g\nRSA 0102 0304"
        );
        assert!(KeyLog::parse(&text).is_empty());
    }

    #[test]
    fn keeps_the_lines_of_the_connections_asked_for() {
        let text = format!("# comment\nCLIENT_RANDOM {RANDOM_A} 01\nCLIENT_RANDOM {RANDOM_B} 02\n");
        let kept = lines_for(&text, &HashSet::from([[0xbb; RANDOM]]));
        assert_eq!(kept, format!("CLIENT_RANDOM {RANDOM_B} 02\n"));
    }
}
