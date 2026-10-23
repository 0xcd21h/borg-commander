//! An AI agent connected through `borg mcp`: it sees what the user sees and can show things
//! in a panel, while everything that changes the machine (a command, a new file, an edit)
//! waits for the user's approval here, in a dialog that shows all of it.

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

use crate::{AppState, Console, Effect, Mount, Overlay, View};

/// How many entries the log of what agents did keeps.
const LOG_LIMIT: usize = 1000;
const ACCEPT_KEY: char = 'y';
const REJECT_KEY: char = 'n';
/// The widest the approval dialog and the log grow, so code stays readable.
const DIALOG_WIDTH: u16 = 100;
const MARGIN: u16 = 4;
/// Rows of the approval dialog besides its lines: borders, heading, place, buttons, gaps.
const APPROVAL_CHROME: u16 = 8;
/// Columns before a numbered diff line: the number, the change mark, and spaces.
const NUMBER_WIDTH: usize = 7;
/// How long an approval that appears while the user types ignores Yes, so a key meant for the
/// shell or an editor cannot approve what the user has not read.
pub const APPROVAL_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// How many agents are connected now.
    Connected { clients: usize },
    /// An agent asks to do something that changes the machine.
    Ask { id: u64, ask: Ask },
    /// An entry for the log of what agents did.
    Note(String),
    /// An approved command is about to run in a container's shell: the Docker view shows it.
    ShowShell { container: String, name: String },
    /// An agent asks to show a folder in a panel, with `focus` selected in it; `mount` (id and
    /// name) puts the panel in that container's filesystem.
    Reveal { dir: PathBuf, focus: Option<String>, mount: Option<(String, String)> },
    /// An approved command goes into the host's console: `line` is typed at its prompt.
    Type { line: String },
    /// Text an agent puts at the console's cursor for the user to review: nothing runs.
    Paste { text: String },
    /// An approved ask is done; `notice` says how it went.
    Finished { id: u64, notice: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskKind {
    Run,
    Create,
    Edit,
}

/// What an agent asks to do, as the approval dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub kind: AskKind,
    /// The command, or the file's path: what the dialog and later notices name.
    pub summary: String,
    /// Where it happens: a folder, or a container.
    pub place: String,
    /// The command's lines, or the change to the file.
    pub body: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Added,
    Removed,
    /// Unchanged lines a diff leaves out; the text says how many.
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub change: Change,
    /// The line's number in the file: the new file's, or the old one's for a removed line.
    pub number: Option<usize>,
    pub text: String,
}

impl DiffLine {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { change: Change::Same, number: None, text: text.into() }
    }
}

/// The approval dialog for one ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub id: u64,
    pub ask: Ask,
    /// The first row shown, after wrapping.
    pub scroll: usize,
    pub accept: bool,
    /// The dialog fills the screen.
    pub full: bool,
    /// Until when Yes is ignored: the user was typing when the dialog appeared.
    pub guarded_until: Option<Instant>,
}

/// How to connect Claude to this borg, as the Connect dialog shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectInfo {
    /// The name Claude knows this borg by, such as `borg-web1`.
    pub name: String,
    /// The borg program to start, by its full path.
    pub program: String,
    /// How SSH reaches this machine, guessed from the connection the user came in on; `None`
    /// when Borg runs on the user's own computer.
    pub ssh: Option<String>,
}

/// The Connect dialog: the target SSH reaches this machine as, and which command is picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    pub target: crate::input::LineInput,
    /// The command for keeping the connection in Claude Code, rather than starting now.
    pub keep: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Agent {
    pub clients: usize,
    /// The approved command running now, and what it is.
    pub running: Option<(u64, String)>,
    /// Asks waiting for the user's answer, oldest first.
    asks: Vec<(u64, Ask)>,
    /// What agents did, oldest first, while the user keeps the log; nothing is written to disk.
    pub log: Vec<String>,
    pub connect: ConnectInfo,
}

impl Agent {
    pub fn connected(&self) -> bool {
        self.clients > 0
    }
}

pub(crate) fn receive(state: &mut AppState, event: AgentEvent) -> Vec<Effect> {
    match event {
        AgentEvent::Connected { clients } => {
            if clients > state.agent.clients {
                state.notice = Some(tr!(agent_connected).into());
            }
            state.agent.clients = clients;
        }
        AgentEvent::Ask { id, ask } => state.agent.asks.push((id, ask)),
        AgentEvent::Note(entry) => {
            if state.settings.preferences.agent_log {
                let log = &mut state.agent.log;
                log.push(entry);
                let overflow = log.len().saturating_sub(LOG_LIMIT);
                log.drain(..overflow);
            }
        }
        AgentEvent::ShowShell { container, name } => return crate::docker::show_shell(state, &container, &name),
        AgentEvent::Reveal { dir, focus, mount } => return reveal(state, dir, focus, mount),
        AgentEvent::Type { line } => return type_line(state, &line),
        AgentEvent::Paste { text } => return paste(state, &text),
        AgentEvent::Finished { id, notice } => {
            if state.agent.running.as_ref().is_some_and(|(running, _)| *running == id) {
                state.agent.running = None;
            }
            state.notice = Some(notice);
            return state.reload_all();
        }
    }
    Vec::new()
}

/// The host console takes a command now: it is the console, its shell reports its prompts,
/// and it sits at an empty prompt with nothing running.
pub fn host_console_ready(state: &AppState) -> bool {
    state.console == Console::Host
        && state.shell.integrated
        && crate::update::shell_ready(state)
        && !state.shell.captured()
}

/// Types an approved command at the host console's prompt, kept out of the shell's history,
/// and shows the console while it runs, as a command typed in the panels does.
fn type_line(state: &mut AppState, line: &str) -> Vec<Effect> {
    state.shell.return_view = state.view;
    state.shell.submit();
    state.view = View::Shell;
    vec![Effect::Write([crate::command::HISTORY_SKIP, line.as_bytes(), crate::command::SUBMIT].concat())]
}

/// Why an agent's text cannot go into the console, if it cannot. Text goes in as a paste
/// where the program there takes pastes (shells and editors insert a paste, never run it),
/// and as typed keys only as one line at a shell's prompt, where nothing runs without Enter.
pub fn typing_refusal(state: &AppState, text: &str) -> Option<&'static str> {
    let pastes = state.shell.observed.mode.contains(borg_term::TermMode::BRACKETED_PASTE);
    let at_prompt = state.shell.integrated && !state.shell.busy();
    if pastes || (at_prompt && !text.contains('\n')) {
        None
    } else if at_prompt {
        Some(tr!(agent_type_lines))
    } else {
        Some(tr!(agent_type_program))
    }
}

/// Puts an agent's text at the console's cursor without pressing Enter, and shows the
/// console when the text is more than the command line shows.
fn paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    if (text.contains('\n') || state.shell.busy()) && state.view == View::Panels {
        state.shell.return_view = View::Panels;
        state.view = View::Shell;
    }
    state.shell.mark_typed();
    state.notice = Some(tr!(agent_typed).into());
    vec![Effect::Write(borg_term::encode_paste(text, state.shell.observed.mode))]
}

/// Asks about the oldest waiting request once nothing else is on screen.
pub(crate) fn surface(state: &mut AppState, now: Instant) {
    if state.overlay.is_some() || state.agent.asks.is_empty() {
        return;
    }
    let (id, ask) = state.agent.asks.remove(0);
    let typing = now.saturating_duration_since(state.idle_since) < APPROVAL_GRACE;
    let guarded_until = typing.then(|| now + APPROVAL_GRACE);
    state.overlay = Some(Overlay::Approval(Approval { id, ask, scroll: 0, accept: false, full: false, guarded_until }));
}

pub(crate) fn on_approval_key(
    state: &mut AppState,
    mut approval: Approval,
    key: &KeyEvent,
    now: Instant,
) -> Vec<Effect> {
    let decided = match key.code {
        KeyCode::Char(ACCEPT_KEY) => Some(true),
        KeyCode::Char(REJECT_KEY) | KeyCode::Esc => Some(false),
        KeyCode::Enter => Some(approval.accept),
        _ => None,
    };
    if decided == Some(true) && approval.guarded_until.is_some_and(|until| now < until) {
        state.overlay = Some(Overlay::Approval(approval));
        return Vec::new();
    }
    if let Some(approved) = decided {
        state.overlay = None;
        if approved {
            state.agent.running = Some((approval.id, approval.ask.summary.clone()));
        }
        return vec![Effect::AgentAnswer { id: approval.id, approved }];
    }
    let screen = Rect::new(0, 0, state.cols, state.rows);
    let (_, body) = approval_layout(&approval, screen);
    let page = usize::from(body.height.max(2) - 1);
    let last = approval_rows(&approval.ask.body, body.width).len().saturating_sub(usize::from(body.height));
    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => approval.accept = !approval.accept,
        KeyCode::Up => approval.scroll = approval.scroll.saturating_sub(1),
        KeyCode::Down => approval.scroll += 1,
        KeyCode::PageUp => approval.scroll = approval.scroll.saturating_sub(page),
        KeyCode::PageDown => approval.scroll += page,
        KeyCode::Home => approval.scroll = 0,
        KeyCode::End => approval.scroll = last,
        KeyCode::F(3) => approval.full = !approval.full,
        _ => {}
    }
    approval.scroll = approval.scroll.min(last);
    state.overlay = Some(Overlay::Approval(approval));
    Vec::new()
}

/// Scrolls the approval dialog or the log by `delta` rows, as the mouse wheel does.
pub(crate) fn scroll_dialog(state: &mut AppState, delta: isize) {
    let screen = Rect::new(0, 0, state.cols, state.rows);
    match state.overlay.as_mut() {
        Some(Overlay::Approval(approval)) => {
            let (_, body) = approval_layout(approval, screen);
            let last = approval_rows(&approval.ask.body, body.width).len().saturating_sub(usize::from(body.height));
            approval.scroll = approval.scroll.saturating_add_signed(delta).min(last);
        }
        Some(Overlay::AgentLog { scroll }) => {
            let body = log_layout(screen).1;
            let last = log_rows(&state.agent.log, body.width).len().saturating_sub(usize::from(body.height));
            *scroll = scroll.unwrap_or(last).saturating_add_signed(delta).min(last).into();
            if *scroll == Some(last) {
                *scroll = None;
            }
        }
        _ => {}
    }
}

/// The approval dialog's frame and the area its lines scroll in.
pub fn approval_layout(approval: &Approval, screen: Rect) -> (Rect, Rect) {
    let (width, most) = if approval.full {
        (screen.width, screen.height)
    } else {
        (screen.width.saturating_sub(MARGIN).min(DIALOG_WIDTH), screen.height.saturating_sub(MARGIN))
    };
    // Borders and a column of padding on each side.
    let body_width = width.saturating_sub(4);
    let rows = u16::try_from(approval_rows(&approval.ask.body, body_width).len()).unwrap_or(u16::MAX);
    let height = if approval.full { most } else { rows.saturating_add(APPROVAL_CHROME).min(most) };
    let frame = Rect {
        x: screen.x + (screen.width.saturating_sub(width)) / 2,
        y: screen.y + (screen.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    // Below the top border, the heading and a gap; above the place, buttons and gaps.
    let body = Rect {
        x: frame.x + 2,
        y: frame.y + 3,
        width: body_width,
        height: height.saturating_sub(APPROVAL_CHROME).max(1),
    };
    (frame, body)
}

/// The approval dialog's lines as shown `width` columns wide: each wrapped, a diff's with its
/// number and change mark first.
pub fn approval_rows(body: &[DiffLine], width: u16) -> Vec<(Change, String)> {
    let numbered = body.iter().any(|line| line.number.is_some() || line.change != Change::Same);
    let mut rows = Vec::new();
    for line in body {
        let prefix = if numbered {
            let mark = match line.change {
                Change::Same | Change::Gap => ' ',
                Change::Added => '+',
                Change::Removed => '-',
            };
            let number = line.number.map_or_else(String::new, |n| n.to_string());
            format!("{number:>4} {mark} ")
        } else {
            String::new()
        };
        let indent = if numbered { NUMBER_WIDTH } else { 0 };
        for (index, piece) in wrap(&line.text, usize::from(width).saturating_sub(indent).max(1)).into_iter().enumerate()
        {
            let lead = if index == 0 { prefix.clone() } else { " ".repeat(prefix.chars().count()) };
            rows.push((line.change, format!("{lead}{piece}")));
        }
    }
    rows
}

/// The log dialog's frame and the area its entries scroll in: most of the screen.
pub fn log_layout(screen: Rect) -> (Rect, Rect) {
    let width = screen.width.saturating_sub(MARGIN).min(DIALOG_WIDTH);
    let height = screen.height.saturating_sub(MARGIN);
    let frame = Rect {
        x: screen.x + (screen.width.saturating_sub(width)) / 2,
        y: screen.y + (screen.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let body =
        Rect { x: frame.x + 2, y: frame.y + 1, width: width.saturating_sub(4), height: height.saturating_sub(2) };
    (frame, body)
}

/// The log's entries as shown `width` columns wide, with a blank row between entries.
pub fn log_rows(log: &[String], width: u16) -> Vec<String> {
    let mut rows = Vec::new();
    for entry in log {
        if !rows.is_empty() {
            rows.push(String::new());
        }
        for line in entry.lines() {
            rows.extend(wrap(line, usize::from(width).max(1)));
        }
    }
    rows
}

/// Splits a line into pieces at most `width` columns wide, keeping tabs as spaces.
fn wrap(line: &str, width: usize) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let mut used = 0;
    for c in line.replace('\t', "    ").chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width && used > 0 {
            pieces.push(String::new());
            used = 0;
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(c);
        }
        used += w;
    }
    pieces
}

pub(crate) fn on_log_key(state: &mut AppState, scroll: Option<usize>, key: &KeyEvent) -> Vec<Effect> {
    let screen = Rect::new(0, 0, state.cols, state.rows);
    let body = log_layout(screen).1;
    let last = log_rows(&state.agent.log, body.width).len().saturating_sub(usize::from(body.height));
    let page = usize::from(body.height.max(2) - 1);
    let at = scroll.unwrap_or(last);
    let at = match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Up => at.saturating_sub(1),
        KeyCode::Down => at + 1,
        KeyCode::PageUp => at.saturating_sub(page),
        KeyCode::PageDown => at + page,
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => at,
    };
    let at = at.min(last);
    state.overlay = Some(Overlay::AgentLog { scroll: (at < last).then_some(at) });
    Vec::new()
}

/// Opens the log of what agents did, or says why there is none.
pub(crate) fn open_log(state: &mut AppState) -> Vec<Effect> {
    if !state.settings.preferences.agent_log {
        state.notice = Some(tr!(agent_log_off).into());
    } else if state.agent.log.is_empty() {
        state.notice = Some(tr!(agent_log_empty).into());
    } else {
        state.overlay = Some(Overlay::AgentLog { scroll: None });
    }
    Vec::new()
}

pub(crate) fn open_connect(state: &mut AppState) -> Vec<Effect> {
    let target = state.agent.connect.ssh.clone().unwrap_or_default();
    state.overlay = Some(Overlay::Connect(Connect { target: crate::input::LineInput::new(target), keep: false }));
    Vec::new()
}

pub(crate) fn on_connect_key(state: &mut AppState, mut connect: Connect, key: &KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => connect.keep = !connect.keep,
        KeyCode::Enter => {
            let [start, keep] = connect_commands(&state.agent.connect, connect.target.text().trim());
            state.overlay = None;
            state.notice = Some(tr!(agent_connect_copied).into());
            return vec![Effect::Copy(if connect.keep { keep } else { start })];
        }
        _ if state.agent.connect.ssh.is_some() => {
            connect.target.apply(key);
        }
        _ => {}
    }
    state.overlay = Some(Overlay::Connect(connect));
    Vec::new()
}

/// The commands that connect Claude Code to this borg: one that starts Claude connected now,
/// and one that keeps the connection for every session. Over SSH they reach `target`.
pub fn connect_commands(info: &ConnectInfo, target: &str) -> [String; 2] {
    let (command, args) = match &info.ssh {
        Some(_) => {
            // SSH hands the remote command to the login shell, so the program is quoted there;
            // BatchMode fails at once where a password would be asked.
            let remote = format!("{} mcp", shell_quote(&info.program));
            ("ssh".to_owned(), vec!["-o".to_owned(), "BatchMode=yes".to_owned(), target.to_owned(), remote])
        }
        None => (info.program.clone(), vec!["mcp".to_owned()]),
    };
    let json_args: Vec<String> = args.iter().map(|arg| json_string(arg)).collect();
    let config = format!(
        r#"{{"mcpServers":{{{}:{{"command":{},"args":[{}]}}}}}}"#,
        json_string(&info.name),
        json_string(&command),
        json_args.join(",")
    );
    let shell_args: Vec<String> = args.iter().map(|arg| shell_quote(arg)).collect();
    [
        format!("claude --mcp-config {}", shell_quote(&config)),
        format!("claude mcp add {} -- {} {}", info.name, shell_quote(&command), shell_args.join(" ")),
    ]
}

/// `text` as one shell word: unchanged when it is plain, otherwise in single quotes.
pub fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '=' | '@' | ':' | ','));
    if plain { text.to_owned() } else { format!("'{}'", text.replace('\'', r"'\''")) }
}

fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Shows a folder in the inactive panel, so the user's own panel stays where it is: on this
/// machine, or in a container's filesystem.
fn reveal(state: &mut AppState, dir: PathBuf, focus: Option<String>, mount: Option<(String, String)>) -> Vec<Effect> {
    let side = state.active.other();
    state.aside = None;
    let panel = state.panel(side);
    let host = panel.mount.as_ref().map_or_else(|| panel.dir().to_path_buf(), |mount| mount.host.clone());
    state.panel_mut(side).mount = mount.map(|(id, name)| Mount { id, name, host });
    vec![state.navigate(side, dir, focus)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_commands_start_claude_connected_or_keep_the_server() {
        let info = ConnectInfo {
            name: "borg-web1".into(),
            program: "/home/deploy/.local/bin/borg".into(),
            ssh: Some("deploy@10.0.3.17".into()),
        };
        let [start, keep] = connect_commands(&info, "prod");
        assert_eq!(
            start,
            r#"claude --mcp-config '{"mcpServers":{"borg-web1":{"command":"ssh","args":["-o","BatchMode=yes","prod","/home/deploy/.local/bin/borg mcp"]}}}'"#
        );
        assert_eq!(keep, "claude mcp add borg-web1 -- ssh -o BatchMode=yes prod '/home/deploy/.local/bin/borg mcp'");

        let local = ConnectInfo { name: "borg".into(), program: "/opt/my tools/borg".into(), ssh: None };
        let [start, keep] = connect_commands(&local, "");
        assert_eq!(
            start,
            r#"claude --mcp-config '{"mcpServers":{"borg":{"command":"/opt/my tools/borg","args":["mcp"]}}}'"#
        );
        assert_eq!(keep, "claude mcp add borg -- '/opt/my tools/borg' mcp");
    }

    #[test]
    fn diff_rows_carry_numbers_and_marks_and_wrap_under_their_text() {
        let body = vec![
            DiffLine { change: Change::Same, number: Some(9), text: "server {".into() },
            DiffLine { change: Change::Removed, number: Some(10), text: "listen 80;".into() },
            DiffLine { change: Change::Added, number: Some(10), text: "listen 8080 default_server;".into() },
        ];
        let rows = approval_rows(&body, 20);
        assert_eq!(rows[0], (Change::Same, "   9   server {".into()));
        assert_eq!(rows[1], (Change::Removed, "  10 - listen 80;".into()));
        assert_eq!(rows[2], (Change::Added, "  10 + listen 8080 d".into()));
        assert_eq!(rows[3], (Change::Added, "       efault_server".into()));
        assert_eq!(rows[4], (Change::Added, "       ;".into()));
        let command = approval_rows(&[DiffLine::plain("echo hi")], 20);
        assert_eq!(command, [(Change::Same, "echo hi".into())], "a command has no numbers");
    }
}
