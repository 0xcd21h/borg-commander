//! A capture as text for an agent: where it was taken, the protocols Borg recognized, who talked
//! to whom and how much, and one packet's layers and bytes when asked for.

use super::TrafficView;
use borg_view::HexLayout;
use std::{cmp::Reverse, collections::HashMap, fmt::Write, time::Duration};
use unicode_width::UnicodeWidthStr;

/// Digits after the point of the capture's length, and of a packet's time from the start.
const SPAN_DIGITS: usize = 1;
const TIME_DIGITS: usize = 6;
/// What a column shows where the capture tells nothing.
const NOTHING: &str = "-";
const COLUMN_GAP: &str = "  ";
const PROCESS_COLUMNS: [&str; 6] = ["PID", "PROCESS", "PACKETS", "BYTES", "HOSTS", "CONNECTIONS"];
const HOST_COLUMNS: [&str; 6] = ["ADDRESS", "NAME", "PACKETS", "BYTES", "PROTOCOLS", "PROCESSES"];
const CONNECTION_COLUMNS: [&str; 11] =
    ["PROTOCOL", "PID", "PROCESS", "LOCAL", "REMOTE", "HOST", "ASKED", "PACKETS", "SENT", "RECEIVED", "FIRST"];

impl TrafficView {
    /// The capture as the filter keeps it: where it was taken and saved, the protocols of its
    /// packets, then its processes, hosts and connections, busiest first and at most `limit` of
    /// each. What the packets carry is left out.
    pub fn report(&self, limit: usize) -> String {
        let capture = &self.capture;
        let on = self.interface.as_ref().map_or_else(String::new, |interface| format!(" on {interface}"));
        let mut text = format!(
            "Capture{on} · packets: {} · bytes: {} · seconds: {:.SPAN_DIGITS$}\n",
            capture.packets.len(),
            capture.bytes(),
            capture.span().as_secs_f64()
        );
        if let Some(path) = &self.path {
            let _ = writeln!(text, "Saved to {}", path.display());
        }
        if capture.skipped > 0 {
            let _ = writeln!(text, "Packets left out past the limit: {}", capture.skipped);
        }
        if capture.cut {
            let _ = writeln!(text, "The file ends inside a packet");
        }
        if !self.filter.trim().is_empty() {
            let _ = writeln!(text, "Filter {} keeps {} of the packets", self.filter.trim(), self.packets.len());
        }
        let _ = writeln!(text, "Protocols by packet: {}", self.protocol_counts());
        self.processes_section(&mut text, limit);
        self.hosts_section(&mut text, limit);
        self.connections_section(&mut text, limit);
        text
    }

    /// One packet, counted from 1 as the summary's FIRST column counts: what it says, its time
    /// and size, its connection with the packets before and after it there, its layers, and its
    /// bytes as a hex dump. `None` when the capture has no such packet.
    pub fn packet_report(&self, number: usize) -> Option<String> {
        let index = number.checked_sub(1)?;
        let packet = self.capture.packets.get(index)?;
        let mut text = format!(
            "Packet {number} of {}: {} {}\nAt {} s · {} bytes on the wire · {} captured\n",
            self.capture.packets.len(),
            packet.layers.protocol(),
            packet.summary(),
            self.since_start(packet.stamp),
            packet.length,
            packet.data.len()
        );
        if let Some(flow) = packet.flow {
            let (conversation, ends) = (&self.capture.flows[flow], self.ends[flow]);
            let place = conversation.packets.iter().position(|&other| other == index).unwrap_or_default();
            let neighbour = |at: Option<usize>| {
                at.and_then(|at| conversation.packets.get(at))
                    .map_or_else(|| NOTHING.to_owned(), |p| (p + 1).to_string())
            };
            let _ = writeln!(
                text,
                "Connection: {} {} → {} · {} · packet {} of {} in it · previous {} · next {}",
                Self::protocol(conversation),
                ends.local,
                ends.remote,
                self.process(ends.pid),
                place + 1,
                conversation.packets.len(),
                neighbour(place.checked_sub(1)),
                neighbour(Some(place + 1))
            );
        }
        let mut layers: Vec<(&str, String)> = Vec::new();
        if let Some(interface) = &packet.interface {
            layers.push(("Interface", interface.clone()));
        }
        layers.extend(packet.layer_rows());
        let width = layers.iter().map(|(label, _)| label.width()).max().unwrap_or_default();
        for (label, value) in layers {
            let _ = writeln!(text, "{label:<width$}{COLUMN_GAP}{value}");
        }
        let _ = writeln!(text, "Bytes:");
        for line in HexLayout::full(packet.data.len() as u64).lines(&packet.data) {
            let _ = writeln!(text, "{line}");
        }
        Some(text)
    }

    fn since_start(&self, stamp: Duration) -> String {
        format!("{:.TIME_DIGITS$}", self.elapsed(stamp).as_secs_f64())
    }

    /// A process by name and PID, or what the screen calls one it cannot tell.
    fn process(&self, pid: Option<u32>) -> String {
        match pid {
            Some(pid) => format!("{} (PID {pid})", self.name(Some(pid))),
            None => self.name(None),
        }
    }

    /// How many of the kept packets each protocol has, most first.
    fn protocol_counts(&self) -> String {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &packet in &self.packets {
            *counts.entry(self.capture.packets[packet].layers.protocol()).or_default() += 1;
        }
        let mut counts: Vec<(&str, usize)> = counts.into_iter().collect();
        counts.sort_by_key(|&(protocol, count)| (Reverse(count), protocol));
        if counts.is_empty() {
            return NOTHING.to_owned();
        }
        counts.iter().map(|(protocol, count)| format!("{protocol} {count}")).collect::<Vec<_>>().join(", ")
    }

    fn processes_section(&self, text: &mut String, limit: usize) {
        let rows = self.talkers.iter().take(limit).map(|talker| {
            vec![
                pid(talker.pid),
                self.name(talker.pid),
                talker.packets.to_string(),
                talker.bytes.to_string(),
                talker.hosts.to_string(),
                talker.flows.len().to_string(),
            ]
        });
        section(text, &format!("Processes: {}", self.talkers.len()), &PROCESS_COLUMNS, rows.collect());
        more(text, self.talkers.len(), limit);
    }

    fn hosts_section(&self, text: &mut String, limit: usize) {
        let rows = self.host_rows.iter().take(limit).map(|row| {
            let processes: Vec<String> = row.pids.iter().map(|pid| self.process(*pid)).collect();
            vec![
                row.address.to_string(),
                self.host(row.address).unwrap_or(NOTHING).to_owned(),
                row.packets.to_string(),
                row.bytes.to_string(),
                row.protocols.join(", "),
                processes.join(", "),
            ]
        });
        section(text, &format!("Hosts: {}", self.host_rows.len()), &HOST_COLUMNS, rows.collect());
        more(text, self.host_rows.len(), limit);
    }

    fn connections_section(&self, text: &mut String, limit: usize) {
        let rows = self.flows.iter().take(limit).map(|&flow| {
            let (conversation, ends) = (&self.capture.flows[flow], self.ends[flow]);
            let (sent, received) = self.sent_received(flow);
            let first =
                conversation.packets.first().map_or_else(|| NOTHING.to_owned(), |first| (first + 1).to_string());
            vec![
                Self::protocol(conversation).to_owned(),
                pid(ends.pid),
                self.name(ends.pid),
                ends.local.to_string(),
                ends.remote.to_string(),
                self.host(ends.remote.ip()).unwrap_or(NOTHING).to_owned(),
                conversation.server_name.clone().unwrap_or_else(|| NOTHING.to_owned()),
                conversation.packets.len().to_string(),
                sent.to_string(),
                received.to_string(),
                first,
            ]
        });
        let heading = format!("Connections: {}, busiest first", self.flows.len());
        section(text, &heading, &CONNECTION_COLUMNS, rows.collect());
        more(text, self.flows.len(), limit);
    }
}

fn pid(pid: Option<u32>) -> String {
    pid.map_or_else(|| NOTHING.to_owned(), |pid| pid.to_string())
}

/// A blank line, a heading, then the rows under their column names, each column as wide as its
/// widest cell.
fn section(text: &mut String, heading: &str, columns: &[&str], rows: Vec<Vec<String>>) {
    let _ = writeln!(text, "\n{heading}");
    if rows.is_empty() {
        return;
    }
    let header: Vec<String> = columns.iter().map(|column| (*column).to_owned()).collect();
    let lines: Vec<Vec<String>> = std::iter::once(header).chain(rows).collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|column| lines.iter().map(|line| line[column].width()).max().unwrap_or_default())
        .collect();
    for line in &lines {
        let padded: Vec<String> = line
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell}{}", " ".repeat(width - cell.width())))
            .collect();
        let _ = writeln!(text, "{}", padded.join(COLUMN_GAP).trim_end());
    }
}

fn more(text: &mut String, total: usize, limit: usize) {
    if total > limit {
        let _ = writeln!(text, "… {} more", total - limit);
    }
}
