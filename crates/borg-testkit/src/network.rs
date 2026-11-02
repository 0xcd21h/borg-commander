//! A made-up machine for the network screen's snapshots and screenshots: its sockets, the
//! processes behind them and its interfaces, sampled each second for half a minute.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use borg_core::{Effect, Interface, InterfaceKind, Msg, NetworkSample, Socket, TcpState, Transport};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::Stage;

/// Seconds of samples, enough to fill the charts' recent half.
pub const SECONDS: u64 = 30;

/// Opens the network screen with Alt-N and gives it `SECONDS` of samples a second apart and
/// the names of the hosts connections go to. Answers the time to render at.
pub fn open(stage: &mut Stage) -> Result<Instant> {
    let effects = borg_core::update(
        &mut stage.state,
        Msg::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT)),
        stage.started,
    );
    ensure!(effects.iter().any(|effect| matches!(effect, Effect::Network { .. })), "opening samples");
    let mut now = stage.started;
    for second in 0..SECONDS {
        let id = stage.state.network.pending.context("the screen waits for a sample")?;
        borg_core::update(&mut stage.state, Msg::Network { id, result: Ok(sample(second)) }, now);
        now += Duration::from_secs(1);
        borg_core::update(&mut stage.state, Msg::Tick, now);
    }
    let named = |address: &str, name: Option<&str>| (address.parse().unwrap(), name.map(str::to_owned));
    stage.send(Msg::HostNames(vec![
        named("203.0.113.80", Some("cdn.example.net")),
        named("203.0.113.81", Some("api.example.com")),
        named("203.0.113.82", Some("media.example.com")),
        named("198.51.100.40", Some("imap.example.org")),
        named("203.0.113.200", Some("backup.example.com")),
        named("198.51.100.24", None),
    ]));
    Ok(now)
}

/// Bytes a made-up stream has moved after `second` seconds, at `base` a second plus a slow
/// swing of up to `swing`.
fn moved_by(second: u64, base: u64, swing: u64) -> u64 {
    (0..=second).map(|i| base + (swing as f64 * (i as f64 / 4.0).sin().abs()) as u64).sum()
}

/// A made-up machine `second` seconds into a minute: an SSH server open to every network,
/// a database and a cache for itself, a web app on one address, a browser downloading and a
/// backup uploading.
pub fn sample(second: u64) -> NetworkSample {
    let socket = |transport, local: &str, remote: Option<&str>, state, pid, counts: Option<(u64, u64)>| Socket {
        transport,
        local: local.parse().unwrap(),
        remote: remote.map(|remote| remote.parse().unwrap()),
        state,
        pid,
        received: counts.map(|(received, _)| received),
        sent: counts.map(|(_, sent)| sent),
    };
    let listen =
        |local: &str, pid| socket(Transport::Tcp, local, None, Some(TcpState::Listen), Some(pid), Some((0, 0)));
    let connected = |local: &str, remote: &str, pid, counts| {
        socket(Transport::Tcp, local, Some(remote), Some(TcpState::Established), pid, Some(counts))
    };
    let download = moved_by(second, 380_000, 2_400_000);
    let upload = moved_by(second, 120_000, 900_000);
    let ssh = (moved_by(second, 900, 2_000), moved_by(second, 4_000, 12_000));
    let sockets = vec![
        listen("0.0.0.0:22", 301),
        listen("[::]:22", 301),
        connected("192.0.2.20:22", "198.51.100.24:51522", Some(301), ssh),
        listen("127.0.0.1:5432", 812),
        listen("[::1]:5432", 812),
        connected(
            "127.0.0.1:5432",
            "127.0.0.1:61001",
            Some(812),
            (moved_by(second, 300, 600), moved_by(second, 9_000, 30_000)),
        ),
        connected(
            "127.0.0.1:61001",
            "127.0.0.1:5432",
            Some(4100),
            (moved_by(second, 9_000, 30_000), moved_by(second, 300, 600)),
        ),
        listen("127.0.0.1:6379", 977),
        listen("192.0.2.20:3000", 4100),
        socket(
            Transport::Udp,
            "0.0.0.0:5353",
            None,
            None,
            Some(188),
            Some((moved_by(second, 200, 400), moved_by(second, 100, 200))),
        ),
        socket(Transport::Udp, "[::]:5353", None, None, Some(188), Some((moved_by(second, 200, 400), 0))),
        connected("192.0.2.20:50412", "203.0.113.80:443", Some(8239), (download, moved_by(second, 9_000, 20_000))),
        connected(
            "192.0.2.20:50413",
            "203.0.113.81:443",
            Some(8239),
            (moved_by(second, 3_000, 8_000), moved_by(second, 1_500, 2_000)),
        ),
        socket(
            Transport::Udp,
            "192.0.2.20:62556",
            Some("203.0.113.82:443"),
            None,
            Some(8239),
            Some((moved_by(second, 40_000, 90_000), moved_by(second, 2_000, 4_000))),
        ),
        connected(
            "192.0.2.20:50500",
            "198.51.100.40:993",
            Some(6120),
            (moved_by(second, 600, 1_200), moved_by(second, 200, 300)),
        ),
        connected("192.0.2.20:50600", "203.0.113.200:443", Some(7001), (moved_by(second, 3_000, 6_000), upload)),
        socket(
            Transport::Tcp,
            "192.0.2.20:50398",
            Some("203.0.113.80:443"),
            Some(TcpState::TimeWait),
            None,
            Some((0, 0)),
        ),
    ];
    let names = [
        (301, "sshd"),
        (812, "postgres"),
        (977, "redis-server"),
        (4100, "node"),
        (188, "mDNSResponder"),
        (8239, "browser"),
        (6120, "mail"),
        (7001, "backup-agent"),
    ]
    .into_iter()
    .map(|(pid, name)| (pid, name.to_owned()))
    .collect();
    let interface = |name: &str, kind, up, addresses: &[(&str, u8)], received, sent| Interface {
        name: name.into(),
        kind,
        up,
        addresses: addresses.iter().map(|(address, prefix)| (address.parse().unwrap(), *prefix)).collect(),
        mac: (kind != InterfaceKind::Loopback).then(|| "02:00:5e:10:00:20".to_owned()),
        mtu: Some(if kind == InterfaceKind::Loopback { 16_384 } else { 1_500 }),
        received,
        sent,
        packets_received: received / 1_200,
        packets_sent: sent / 1_200,
        errors: 0,
    };
    let interfaces = vec![
        interface("bridge0", InterfaceKind::Bridge, true, &[], 0, 0),
        interface(
            "en0",
            InterfaceKind::WiFi,
            true,
            &[("192.0.2.20", 24), ("2001:db8::20", 64)],
            41_000_000_000 + download,
            6_000_000_000 + upload,
        ),
        interface("en1", InterfaceKind::Ethernet, false, &[], 0, 0),
        interface(
            "lo0",
            InterfaceKind::Loopback,
            true,
            &[("127.0.0.1", 8), ("::1", 128)],
            2_000_000_000 + moved_by(second, 9_300, 30_600),
            2_000_000_000 + moved_by(second, 9_300, 30_600),
        ),
        interface(
            "utun3",
            InterfaceKind::Tunnel,
            true,
            &[("198.51.100.2", 32)],
            80_000_000 + moved_by(second, 1_000, 3_000),
            20_000_000,
        ),
    ];
    NetworkSample { sockets, interfaces, names }
}
