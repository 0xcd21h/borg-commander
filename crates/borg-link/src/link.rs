//! Where a relay is, and the one-time link a server shows so a computer can pair with it.

use std::fmt;

use crate::key::{KEY_LEN, PublicKey, hex, unhex};

pub const SECRET_LEN: usize = 16;
const SCHEME: &str = "borg://";
/// A link to a relay without TLS, for one on the same machine or behind a proxy that adds it.
const PLAIN_SCHEME: &str = "borg+ws://";

/// A relay's address: its host and port, and whether it speaks TLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    pub host: String,
    pub tls: bool,
}

impl Relay {
    /// Reads `wss://host[:port]`, `ws://host:port`, or a bare host, which means TLS.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_end_matches('/');
        let (host, tls) = match text.split_once("://") {
            Some(("wss", host)) => (host, true),
            Some(("ws", host)) => (host, false),
            Some(_) => return None,
            None => (text, true),
        };
        let valid = !host.is_empty()
            && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
        valid.then(|| Self { host: host.to_owned(), tls })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}://{}{path}", if self.tls { "wss" } else { "ws" }, self.host)
    }
}

impl fmt::Display for Relay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", if self.tls { "wss" } else { "ws" }, self.host)
    }
}

/// What a computer needs to pair with a server: the relay, the server's key, and a secret
/// that works once. The key lets the computer recognise the server through any relay; the
/// secret shows the server that the computer got the link from its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLink {
    pub relay: Relay,
    pub server: PublicKey,
    pub secret: [u8; SECRET_LEN],
}

impl PairingLink {
    /// Reads a link, leaving out any spaces and line breaks: a link has none, but copying
    /// one that a terminal wrapped over two rows can add them.
    pub fn parse(text: &str) -> Option<Self> {
        let text: String = text.split_whitespace().collect();
        let text = text.as_str();
        let (rest, tls) = match text.strip_prefix(SCHEME) {
            Some(rest) => (rest, true),
            None => (text.strip_prefix(PLAIN_SCHEME)?, false),
        };
        let (host, rest) = rest.split_once('/')?;
        let (key, secret) = rest.split_once('#')?;
        let relay = Relay::parse(&format!("{}://{host}", if tls { "wss" } else { "ws" }))?;
        Some(Self { relay, server: unhex::<KEY_LEN>(key)?, secret: unhex::<SECRET_LEN>(secret)? })
    }
}

impl fmt::Display for PairingLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.relay.tls { SCHEME } else { PLAIN_SCHEME };
        write!(f, "{scheme}{}/{}#{}", self.relay.host, hex(&self.server), hex(&self.secret))
    }
}

/// A fresh secret for a pairing link.
pub fn secret() -> [u8; SECRET_LEN] {
    let mut secret = [0; SECRET_LEN];
    getrandom::fill(&mut secret).expect("the system gives random bytes");
    secret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relays_and_pairing_links_read_back() {
        assert_eq!(Relay::parse("relay.example.com"), Some(Relay { host: "relay.example.com".into(), tls: true }));
        assert_eq!(
            Relay::parse("ws://127.0.0.1:9000/").map(|r| r.url("/v1/listen/x")),
            Some("ws://127.0.0.1:9000/v1/listen/x".into())
        );
        assert_eq!(Relay::parse("http://relay.example.com"), None);
        assert_eq!(Relay::parse("wss://relay example"), None);

        let link = PairingLink {
            relay: Relay::parse("relay.example.com").unwrap(),
            server: [7; KEY_LEN],
            secret: [9; SECRET_LEN],
        };
        let text = link.to_string();
        assert!(text.starts_with("borg://relay.example.com/0707"), "{text}");
        assert_eq!(PairingLink::parse(&text), Some(link.clone()));
        // Copied from a dialog that wrapped it over two rows.
        let (first, second) = text.split_at(40);
        assert_eq!(PairingLink::parse(&format!("{first} {second}")), Some(link.clone()));
        assert_eq!(PairingLink::parse(&format!("{first}\n  {second}")), Some(link.clone()));
        let plain = PairingLink { relay: Relay::parse("ws://127.0.0.1:9000").unwrap(), ..link };
        assert!(plain.to_string().starts_with("borg+ws://127.0.0.1:9000/"));
        assert_eq!(PairingLink::parse(&plain.to_string()), Some(plain));
        assert_eq!(PairingLink::parse("borg://relay.example.com/07#09"), None);
    }
}
