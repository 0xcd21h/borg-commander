//! The `hosts` tool: every paired host as the hosts page shows it, asked once the way the page
//! asks, with the target IDs the other tools take.

use std::{path::Path, time::Instant};

use borg_core::{ConcernLevel, ConcernReport, FiguresReport, HostHeard, HostReport, Hosts, ReachReport, host_reports};
use serde_json::json;

use super::Reply;
use crate::host_watch::{Seen, ask_once, paired_hosts};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const DAY: u64 = 24 * 60 * 60;
const HOUR: u64 = 60 * 60;

/// Asks every paired host how it is doing, keeping those `filter` keeps, as the page's filter
/// does.
pub(super) async fn hosts(state: &Path, filter: &str) -> Reply {
    let servers = crate::link::servers(state);
    if servers.is_empty() {
        return Reply::ok("No hosts are paired with this computer. Pair one with borg pair.");
    }
    let mut seen = Seen::load(state);
    seen.keep(&servers);
    let mut hosts = Hosts::default();
    hosts.filter = filter.trim().to_owned();
    hosts.list(paired_hosts(&servers, &seen));
    hosts.take_names(&crate::remote_names::load(state).unwrap_or_default());
    let now = Instant::now();
    for (identity, heard) in ask_once(state, servers).await {
        if let HostHeard::Answered(reply) = &heard {
            seen.mark(&identity, reply.seen);
        }
        hosts.answer(&identity, heard, now);
    }
    let reports = host_reports(&hosts, Instant::now());
    Reply { ok: true, text: summary(&reports, hosts.cards.len()), structured: Some(json!({ "hosts": reports })) }
}

/// The reports as text: a line on all of them, then a few lines for each host.
pub(super) fn summary(reports: &[HostReport], paired: usize) -> String {
    let count = |reach: ReachReport| reports.iter().filter(|report| report.reach == reach).count();
    let mut head = format!("{} of {paired} paired hosts", reports.len());
    let states = [
        (ReachReport::Answering, "answering"),
        (ReachReport::Silent, "not answering in time"),
        (ReachReport::Failed, "failed"),
        (ReachReport::Offline, "offline"),
    ];
    let parts: Vec<String> = states
        .iter()
        .filter_map(|(reach, word)| Some(count(*reach)).filter(|n| *n > 0).map(|n| format!("{n} {word}")))
        .collect();
    if !parts.is_empty() {
        head.push_str(": ");
        head.push_str(&parts.join(", "));
    }
    head.push('.');
    let mut lines = vec![head];
    for report in reports {
        lines.extend(host_lines(report));
    }
    lines.join("\n")
}

fn host_lines(report: &HostReport) -> Vec<String> {
    let mut first = format!("{} ({}): ", report.name, report.target);
    match report.reach {
        ReachReport::Answering => {
            first.push_str("answering");
            if let Some(ms) = report.round_trip_ms {
                first.push_str(&format!(" in {ms} ms"));
            }
        }
        ReachReport::Waiting => first.push_str("no answer yet"),
        ReachReport::Silent => first.push_str("not answering in time"),
        ReachReport::Offline => first.push_str("offline"),
        ReachReport::Failed => {
            first.push_str("failed: ");
            first.push_str(report.error.as_deref().unwrap_or_default());
        }
    }
    if let Some(borg) = &report.borg {
        first.push_str(&format!(", Borg {borg}"));
        if report.older_borg {
            first.push_str(" (older than this computer's)");
        }
    }
    if let Some(system) = &report.system {
        first.push_str(&format!(", {system}"));
    }
    if report.reach != ReachReport::Answering
        && let Some(at) = &report.last_answered
    {
        first.push_str(&format!(", last answered {at}"));
    }
    let mut lines = vec![first];
    match &report.figures {
        Some(figures) => lines.extend(figure_lines(figures)),
        None if report.reach == ReachReport::Answering => {
            lines.push("  sends no figures: its Borg is too old; updating it adds them".into());
        }
        None => {}
    }
    if !report.concerns.is_empty() {
        let concerns: Vec<String> = report.concerns.iter().map(concern_text).collect();
        lines.push(format!("  concerns: {}", concerns.join(", ")));
    }
    if !report.sessions.is_empty() {
        let sessions: Vec<String> =
            report.sessions.iter().map(|session| format!("{} ({})", session.name, session.target)).collect();
        lines.push(format!("  sessions: {}", sessions.join(", ")));
    }
    lines
}

fn figure_lines(figures: &FiguresReport) -> Vec<String> {
    let mut machine = Vec::new();
    if let Some(cpu) = figures.cpu_percent {
        machine.push(format!("CPU {}", percent(cpu)));
    }
    if let Some(load) = &figures.load {
        let mut text = format!("load {:.2} {:.2} {:.2} on {} cores", load.one, load.five, load.fifteen, figures.cores);
        if let Some(share) = load.percent_of_cores {
            text.push_str(&format!(" ({} of the cores)", percent(share)));
        }
        machine.push(text);
    }
    if let Some(memory) = &figures.memory {
        machine.push(format!("RAM {} of {}", percent(memory.percent), size(memory.total_bytes)));
        if memory.swap_total_bytes > 0 {
            machine.push(format!("swap {} of {}", size(memory.swap_used_bytes), size(memory.swap_total_bytes)));
        }
    }
    if let Some(disk) = &figures.disk {
        machine.push(format!(
            "disk {} {} used, {} free",
            disk.mount,
            percent(disk.percent_used),
            size(disk.available_bytes)
        ));
    }
    if let Some(uptime) = figures.uptime_seconds {
        machine.push(format!("up {}", span(uptime)));
    }
    let mut lines = Vec::new();
    if !machine.is_empty() {
        lines.push(format!("  {}", machine.join(", ")));
    }
    let mut rest = Vec::new();
    if let Some(watcher) = &figures.watcher {
        if watcher.on {
            let mut text = format!(
                "watcher on, unread: {}, {}",
                counted(watcher.unread_alerts, "alert"),
                counted(watcher.unread_warnings, "warning")
            );
            if watcher.frozen > 0 {
                text.push_str(&format!(", {} frozen", counted(watcher.frozen, "process")));
            }
            if let Some(until) = &watcher.learning_until {
                text.push_str(&format!(", learning until {until}"));
            }
            rest.push(text);
        } else {
            rest.push("watcher off".into());
        }
    }
    if let Some(containers) = &figures.containers {
        let mut text = format!("containers {} of {} running", containers.running, containers.total);
        if containers.troubled > 0 {
            text.push_str(&format!(", {} unhealthy or restarting", containers.troubled));
        }
        rest.push(text);
    }
    if !rest.is_empty() {
        lines.push(format!("  {}", rest.join(", ")));
    }
    lines
}

fn concern_text(concern: &ConcernReport) -> String {
    let figure = serde_json::to_value(concern.figure).ok().and_then(|value| value.as_str().map(str::to_owned));
    let level = match concern.level {
        ConcernLevel::Alert => "alert",
        ConcernLevel::Warning => "warning",
        ConcernLevel::Notice => "notice",
    };
    format!("{} {level}", figure.unwrap_or_default())
}

fn percent(value: f64) -> String {
    format!("{}%", value.round())
}

fn size(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB)
}

fn span(seconds: u64) -> String {
    match seconds {
        seconds if seconds >= DAY => counted(whole(seconds / DAY), "day"),
        seconds if seconds >= HOUR => counted(whole(seconds / HOUR), "hour"),
        seconds => counted(whole(seconds / 60), "minute"),
    }
}

fn whole(count: u64) -> usize {
    usize::try_from(count).unwrap_or(usize::MAX)
}

fn counted(count: usize, thing: &str) -> String {
    match (count, thing) {
        (1, _) => format!("1 {thing}"),
        (_, "process") => format!("{count} processes"),
        _ => format!("{count} {thing}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_core::{
        ContainerFigures, DiskFigures, HostFigures, HostReply, LoadAverage, MemoryFigures, PairedHost, RemoteChoice,
        RemoteHostInfo, WatchFigures,
    };
    use std::time::{Duration, SystemTime};

    const GIB_BYTES: u64 = 1024 * 1024 * 1024;

    fn paired(name: &str) -> PairedHost {
        PairedHost { identity: name.into(), machine: name.into(), relay: "wss://relay.example".into(), last_seen: None }
    }

    #[test]
    fn the_text_names_each_host_its_target_its_figures_and_what_worries() {
        let now = Instant::now();
        let mut hosts = Hosts::default();
        hosts.list(vec![paired("web-01"), paired("db"), paired("nas")]);
        let figures = HostFigures {
            cores: 4,
            cpu: Some(1_250),
            load: Some(LoadAverage { one: 42, five: 38, fifteen: 30 }),
            uptime: Some(16 * DAY + 5),
            memory: Some(MemoryFigures {
                used: 6 * GIB_BYTES,
                total: 8 * GIB_BYTES,
                swap_used: 0,
                swap_total: 2 * GIB_BYTES,
            }),
            disk: Some(DiskFigures { mount: "/".into(), total: 100 * GIB_BYTES, available: 3 * GIB_BYTES }),
            watch: Some(WatchFigures { on: true, unread_alerts: 1, ..Default::default() }),
            containers: Some(ContainerFigures { running: 5, total: 6, troubled: 1 }),
        };
        let session = RemoteChoice {
            identity: "web-01".into(),
            machine: "web-01".into(),
            session: Some("s1".into()),
            name: "main".into(),
            ..Default::default()
        };
        let info = RemoteHostInfo {
            version: Some(env!("CARGO_PKG_VERSION").into()),
            platform: Some("linux / x86_64".into()),
            latency_ms: Some(38),
            ..Default::default()
        };
        let seen = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let answered = HostReply { info, sessions: vec![session], figures: Some(figures), seen };
        hosts.answer("web-01", HostHeard::Answered(Box::new(answered)), now);
        hosts.answer("db", HostHeard::Offline, now);
        let older = HostReply {
            info: RemoteHostInfo { version: Some("0.1.0".into()), ..Default::default() },
            sessions: Vec::new(),
            figures: None,
            seen,
        };
        hosts.answer("nas", HostHeard::Answered(Box::new(older)), now);

        let text = summary(&host_reports(&hosts, now), hosts.cards.len());
        let expected = format!(
            "3 of 3 paired hosts: 2 answering, 1 offline.
nas (remote:nas): answering, Borg 0.1.0 (older than this computer's)
  sends no figures: its Borg is too old; updating it adds them
web-01 (remote:web-01): answering in 38 ms, Borg {}, linux / x86_64
  CPU 13%, load 0.42 0.38 0.30 on 4 cores (11% of the cores), RAM 75% of 8.0 GiB, swap 0.0 GiB of 2.0 GiB, disk / 97% used, 3.0 GiB free, up 16 days
  watcher on, unread: 1 alert, 0 warnings, containers 5 of 6 running, 1 unhealthy or restarting
  concerns: disk alert, watcher alert, containers warning
  sessions: main (remote:web-01/s1)
db (remote:db): offline",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(text, expected);
    }
}
