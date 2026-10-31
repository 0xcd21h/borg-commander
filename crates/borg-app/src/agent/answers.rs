//! What the session tells an agent: the screen, processes, memory searches, containers and command output.

use super::{LIST_LIMIT, Location, OUTPUT_LIMIT, Reply, Target, hostname};
use borg_core::{AppState, Console, Row, Side, View};
use borg_i18n::tr;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

/// A folder on this machine, with the home folder as `~`.
pub fn home_relative(dir: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.as_os_str().len() > 1);
    match home.and_then(|home| dir.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Where a command runs, for the log and the user's question.
pub fn place(target: &Target) -> String {
    match target {
        Target::Host(dir) => home_relative(dir),
        Target::Container { name, dir: None, .. } => tr!(agent_container_place, name = name),
        Target::Container { name, dir: Some(dir), .. } => {
            tr!(agent_container_folder_place, dir = dir.display(), name = name)
        }
    }
}

/// The output of `command` typed into a shell, from what the terminal received between the
/// command starting and the shell announcing its end (OSC 133): escape sequences and carriage
/// returns removed, the echoed command line left out, and long output cut to its end.
pub fn shell_output(bytes: &[u8], command: &str) -> String {
    pub(super) const COMMAND_START: &[u8] = b"\x1b]133;C";
    pub(super) const COMMAND_END: &[u8] = b"\x1b]133;D";
    let find = |needle: &[u8]| bytes.windows(needle.len()).position(|window| window == needle);
    let end = find(COMMAND_END).unwrap_or(bytes.len());
    let text = match find(COMMAND_START) {
        Some(start) if start < end => String::from_utf8_lossy(&strip_escapes(&bytes[start..end])).replace('\r', ""),
        // A shell without a command-start mark (sh, older bash): the output starts on the line
        // after the echoed command, or after the first line if the echo cannot be found.
        _ => {
            let text = String::from_utf8_lossy(&strip_escapes(&bytes[..end])).replace('\r', "");
            let echo = text.rfind(command).map_or(0, |at| at + command.len());
            text[echo..].find('\n').map_or_else(String::new, |line| text[echo + line + 1..].to_owned())
        }
    };
    let text = if text.len() > OUTPUT_LIMIT {
        let cut = text.len() - OUTPUT_LIMIT;
        let from = (cut..text.len()).find(|&i| text.is_char_boundary(i)).unwrap_or(text.len());
        format!("[earlier output left out]\n{}", &text[from..])
    } else {
        text
    };
    if text.is_empty() || text.ends_with('\n') { text } else { format!("{text}\n") }
}

/// Removes terminal escape sequences: CSI (`ESC [ … final`), OSC (`ESC ] … BEL` or `ESC \\`)
/// and two-byte escapes.
pub(super) fn strip_escapes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'[') => {
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
                i += 1;
            }
            Some(b']') => {
                i += 2;
                while i < bytes.len() && bytes[i] != 0x07 && !(bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\')) {
                    i += 1;
                }
                i += if bytes.get(i) == Some(&0x07) { 1 } else { 2 };
            }
            _ => i += 2,
        }
    }
    out
}

/// The containers, for an agent to choose one.
pub async fn containers() -> Reply {
    match borg_docker::list().await {
        Ok(containers) if containers.is_empty() => Reply::ok("No containers."),
        Ok(containers) => Reply::ok(
            containers
                .iter()
                .map(|c| {
                    let project = c.project.as_deref().map_or_else(String::new, |p| format!("  project {p}"));
                    format!("{}  {:?}  {}  {}{project}", c.name, c.state, c.status, c.image)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Err(error) => Reply::error(format!("{error:#}")),
    }
}

/// Finds a container by name, Compose service, or id prefix.
pub fn find_container<'a>(
    containers: &'a [borg_docker::Container],
    wanted: &str,
) -> Option<&'a borg_docker::Container> {
    containers.iter().find(|c| c.name == wanted).or_else(|| {
        let matches: Vec<_> =
            containers.iter().filter(|c| c.service.as_deref() == Some(wanted) || c.id.starts_with(wanted)).collect();
        (matches.len() == 1).then(|| matches[0])
    })
}

/// The console's terminal: the text it shows and what is typed at its prompt.
pub struct ConsoleText {
    pub lines: Vec<String>,
    pub typed: String,
}

/// How many of the console's last lines `screen` gives while the panels hide it.
pub(super) const CONSOLE_LINES: usize = 20;

/// What the user sees: where they are and what the panels hold, in words an agent can use
/// (panels as locations, which every tool takes as they are), then the screen itself as text.
pub fn screen(state: &AppState, frame: &[String], console: &ConsoleText) -> String {
    let console_shown = state.effective_view() == View::Shell;
    let looking = if state.viewer.is_some() {
        "a file in Borg's viewer"
    } else if console_shown {
        "Borg's console, which hides the panels (Ctrl-O switches)"
    } else if state.docker.is_open() {
        "the Docker view"
    } else if state.activity.side.is_some() {
        "the activity monitor"
    } else {
        "the panels"
    };
    let mut text = format!("Borg on {}. The user is looking at {looking}.\n", hostname());
    if matches!(state.overlay, Some(borg_core::Overlay::Approval(_))) {
        text.push_str("A dialog asks the user to approve a request.\n");
    }
    let _ = writeln!(text, "Active panel: {}", side_name(state.active));
    for side in Side::ALL {
        let panel = state.panel(side);
        let _ = writeln!(text, "{} panel: {}", side_name(side), panel_location(state, side));
        match panel.hovered() {
            Some(Row::Entry(entry)) => drop(writeln!(text, "  highlighted: {}", entry.name)),
            Some(Row::Parent) => drop(writeln!(text, "  highlighted: .. (the parent folder)")),
            Some(Row::Group { label, .. }) => drop(writeln!(text, "  highlighted: group {label}")),
            None => {}
        }
        let marked: Vec<&str> = panel.marked().map(|entry| entry.name.as_str()).take(LIST_LIMIT).collect();
        if !marked.is_empty() {
            let _ = writeln!(text, "  marked: {}", marked.join(", "));
        }
    }
    let shell = match &state.console {
        Console::Remote { machine, session } => format!("remote shell on {machine}:{session}"),
        Console::Host => format!("host shell in {}", state.shell.cwd.display()),
        Console::Container(_) => format!("the {} container's shell", state.console_container().unwrap_or_default()),
    };
    let doing = if state.shell.captured() {
        "running a full-screen program"
    } else if state.shell.busy() {
        "running a command"
    } else {
        "at its prompt"
    };
    let typed =
        if console.typed.is_empty() { String::new() } else { format!(", with {:?} typed there", console.typed) };
    let _ = writeln!(text, "Command line: {shell}, {doing}{typed}");
    let shells: Vec<&str> = state.docker.shells.iter().map(|shell| shell.name.as_str()).collect();
    if !shells.is_empty() {
        let _ = writeln!(text, "Container shells open in Borg: {}", shells.join(", "));
    }
    if state.docker.is_open() {
        let selected = state.docker.container().map_or_else(String::new, |c| format!(", on {} ({})", c.name, c.status));
        let _ = writeln!(text, "Docker view open{selected}");
    }
    if let Some(process) = state.activity.process().filter(|_| state.activity.side.is_some()) {
        let _ = writeln!(text, "Activity monitor open, on {} (PID {})", process.name, process.key.pid);
    }
    // Long runs of empty rows read as one, to keep the text short.
    let shown = |lines: &[String]| {
        let end = lines.iter().rposition(|line| !line.is_empty()).map_or(0, |last| last + 1);
        let mut out: Vec<String> = Vec::new();
        let mut empty = 0;
        for line in &lines[..end] {
            if line.is_empty() {
                empty += 1;
                continue;
            }
            match empty {
                0 => {}
                1 | 2 => out.extend(std::iter::repeat_n(String::new(), empty)),
                _ => out.push(format!("⋯ {empty} empty lines")),
            }
            empty = 0;
            out.push(line.clone());
        }
        out.join("\n")
    };
    if !frame.is_empty() {
        let _ = write!(text, "\nThe screen as the user sees it, as text:\n{}\n", shown(frame));
    }
    if !console_shown {
        let lines = shown(&console.lines);
        let last: Vec<&str> = lines.lines().collect();
        let last = &last[last.len().saturating_sub(CONSOLE_LINES)..];
        if !last.is_empty() {
            let _ = write!(text, "\nThe console behind the panels, its last lines:\n{}\n", last.join("\n"));
        }
    }
    text
}

pub(super) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// Where a panel is: a folder on this machine, or in a container's filesystem.
pub fn panel_location(state: &AppState, side: Side) -> Location {
    let panel = state.panel(side);
    match &panel.mount {
        Some(mount) => Location::Container { container: mount.name.clone(), path: panel.dir().to_path_buf() },
        None => Location::Host(panel.dir().to_path_buf()),
    }
}

/// A location an agent wrote, relative to the active panel unless it says otherwise.
pub fn locate(state: &AppState, text: &str) -> Location {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Location::parse(text, &panel_location(state, state.active), home.as_deref())
}

/// Where an agent's commands run on this machine without a folder of their own: the active
/// panel's folder, or the other panel's while the active one shows a container.
pub fn host_dir(state: &AppState) -> PathBuf {
    [state.active, state.active.other()]
        .into_iter()
        .map(|side| state.panel(side))
        .find(|panel| panel.mount.is_none())
        .map_or_else(|| state.shell.cwd.clone(), |panel| panel.dir().to_path_buf())
}

/// Text an agent types, with every control character left out but line breaks and tabs, so
/// nothing in it can end a paste or act as a key.
pub fn typable(text: &str) -> String {
    text.replace("\r\n", "\n").chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect()
}

/// The `shown` busiest processes, by memory or CPU, with the machine's totals.
pub fn processes(sample: &borg_core::SystemSample, by_cpu: bool, shown: usize) -> Reply {
    use borg_ui::size_text;
    let cores = sample.cpu.len().max(1);
    let cpu = sample.cpu.iter().map(|&c| u64::from(c)).sum::<u64>() as f64 / cores as f64 / 100.0;
    let mut processes: Vec<_> = sample.processes.iter().collect();
    if by_cpu {
        processes.sort_by_key(|p| std::cmp::Reverse(p.cpu.unwrap_or(0)));
    } else {
        processes.sort_by_key(|p| std::cmp::Reverse(p.resident.unwrap_or(0)));
    }
    let mut text = format!(
        "CPU {cpu:.1}% of {cores} cores · RAM {} of {} · {} processes\nPID      CPU%    RAM  NAME\n",
        size_text(sample.used),
        size_text(sample.total),
        sample.processes.len()
    );
    for p in processes.into_iter().take(shown) {
        let cpu = p.cpu.map_or_else(|| "—".into(), |c| format!("{:.1}", f64::from(c) / 100.0));
        let ram = p.resident.map_or_else(|| "—".into(), size_text);
        let path = p.executable.as_ref().map_or_else(String::new, |e| format!("  {}", e.display()));
        let _ = writeln!(text, "{:<8} {cpu:>5} {ram:>6}  {}{path}", p.key.pid, p.name);
    }
    Reply::ok(text)
}

/// A memory search as the agent reads it: what was searched and found, then a row per result.
pub fn memory_report(report: &borg_core::MemorySearchReport, name: &str, limit: usize) -> Reply {
    use borg_ui::size_text;
    let mut text = format!(
        "{name} (PID {}) · {} of {} searched · {} inaccessible{}\n",
        report.process.pid,
        size_text(report.scanned),
        size_text(report.total),
        size_text(report.skipped),
        if report.cancelled || report.next.is_some() { " · stopped before the end" } else { "" }
    );
    let shown = report.hits.len().min(limit);
    match &report.strings {
        Some(tally) => {
            let _ = write!(text, "{} different strings, {} copies", tally.distinct, tally.found);
            for (kind, count) in &tally.kinds {
                let _ = write!(text, " · {} {count}", indicator_name(*kind));
            }
            if tally.files > 0 {
                let _ = write!(text, "\nOnly in memory: {} · files compared: {}", tally.memory_only, tally.files);
            }
            let _ = writeln!(
                text,
                "\nShowing {shown}, best first. Columns: address, kind, copies, memory (no compared file holds it) or file, region, text"
            );
            for hit in &report.hits[..shown] {
                let found = hit.text.as_deref().unwrap_or_default().replace('\t', " ");
                let unit = if hit.encoding.starts_with("UTF-16") { 2 } else { 1 };
                let cut = if found.chars().count() < hit.length / unit { "…" } else { "" };
                let held = match hit.in_file {
                    Some(true) => "file",
                    Some(false) => "memory",
                    None => "-",
                };
                let kind = hit.indicator.map_or("-", indicator_name);
                let _ = writeln!(
                    text,
                    "{:#x}  {kind}  ×{}  {held}  {}  {found}{cut}",
                    hit.address,
                    hit.copies.max(1),
                    hit.region.kind()
                );
            }
        }
        None => {
            let _ =
                writeln!(text, "{} matches; showing {shown}. Columns: address, encoding, region", report.hits.len());
            for hit in &report.hits[..shown] {
                let _ = writeln!(text, "{:#x}  {}  {}", hit.address, hit.encoding, hit.region.name);
            }
        }
    }
    Reply::ok(text)
}

/// An indicator's kind as the agent reads it, the same in every language.
pub(super) fn indicator_name(kind: borg_core::Indicator) -> &'static str {
    use borg_core::Indicator;
    match kind {
        Indicator::Url => "url",
        Indicator::Ip => "ip",
        Indicator::Email => "email",
        Indicator::Path => "path",
        Indicator::Command => "command",
        Indicator::Secret => "secret",
        Indicator::Encoded => "base64",
    }
}
