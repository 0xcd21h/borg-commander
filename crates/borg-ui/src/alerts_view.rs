//! The Alerts page: what the process watcher found, newest first, in one pane, and the selected
//! alert's reason, process and evidence in the other. It shares the startup items screen's look.

use crate::{
    activity_view::{FOOTER_ROWS, body, dim, emphasis, heading, row, severity_style, text, wrap},
    format,
    look::Look,
    network_view::{FilterLine, filter_line},
    spinner::Spinner,
};
use borg_core::{Alert, Alerts, AppState, Freezing, Response, Severity, Unsent, WatchSource, rule_source};
use borg_i18n::{Count, count, tr};
use borg_rules::RuleMatch;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use std::time::Instant;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod controls;

pub(crate) use controls::{keybar, panel_mouse, status};

const SUMMARY_ROW: u16 = 0;
const FILTER_ROW: u16 = 1;
const HEADER_ROW: u16 = 2;
/// What a value Borg could not find out shows as.
const UNKNOWN: &str = "—";
const SEPARATOR: &str = " · ";
/// Blank rows between the list and the note about a log Borg kept aside.
const KEPT_ASIDE_GAP: usize = 1;
/// Blank rows between the warning that alerts reach nobody and the details under it.
const UNSENT_GAP: usize = 1;
/// Panes at least this wide show who the process ran as.
const USER_FROM: u16 = 100;
/// Panes at least this wide show the parent that started it.
const PARENT_FROM: u16 = 86;
/// Panes at least this wide show where the process stands now.
const RESPONSE_FROM: u16 = 56;
/// Panes at least this wide show the time of day as well as the day.
const TIME_FROM: u16 = 56;
const TIME_WIDTH: u16 = 16;
const DAY_WIDTH: u16 = 10;
const RULE_WIDTH: u16 = 20;
const PARENT_WIDTH: u16 = 12;
const USER_WIDTH: u16 = 9;
const RESPONSE_WIDTH: u16 = 11;
/// Cells between a fact's label and its value.
const LABEL_GAP: u16 = 2;
/// Rows a command may take in the details before it is cut.
const COMMAND_ROWS: usize = 4;
/// Rows of process tree the details show.
const TREE_ROWS: usize = 6;
/// Connections the details list.
const CONNECTION_ROWS: usize = 4;
/// Cells each generation of the process tree is indented by.
const TREE_STEP: usize = 2;

pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    panels: [Rect; 2],
    clock: &Zoned,
    now: Instant,
    buf: &mut Buffer,
) {
    let alerts = &state.alerts;
    let Some(side) = alerts.side else { return };
    let spinner = Spinner::of(&state.busy, borg_core::Task::Alerts, now);
    let list = panels[side.index()];
    let details = panels[side.other().index()];
    let unsent = unsent(state);
    if !list.is_empty() {
        draw_list(state, look, list, clock, spinner, unsent.filter(|_| details.is_empty()), buf);
    }
    if !details.is_empty() {
        draw_details(alerts, look, details, clock, unsent, buf);
    }
}

/// The warning that alerts reach nobody, once the page has read how the watcher stands.
fn unsent(state: &AppState) -> Option<Unsent> {
    let alerts = &state.alerts;
    if !alerts.loaded || alerts.error.is_some() {
        return None;
    }
    let preferences = &state.settings.preferences;
    let turned_on = preferences.process_watch.then_some(state.settings.alert_delivery);
    alerts.log.unsent(turned_on, std::time::SystemTime::now())
}

/// The warning stands out more once Borg is about to act, or acts, with nobody told.
fn unsent_style(unsent: Unsent, look: &Look<'_>) -> Style {
    let severity = match unsent.freezing {
        Freezing::NotYet => Severity::Warning,
        Freezing::Soon | Freezing::Started => Severity::Alert,
    };
    severity_style(severity, look.theme)
}

fn frame(alerts: &Alerts, look: &Look<'_>, rect: Rect, title: &str, focused: bool, buf: &mut Buffer) -> Rect {
    let theme = look.theme;
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let controls = crate::results_view::controls(rect, alerts.expanded);
    let room = controls.first().map_or(rect.width.saturating_sub(2), |(r, _, _)| r.x.saturating_sub(rect.x + 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(if focused { styles.border_active } else { styles.border })
        .title(Line::styled(
            format::truncate_end(&format!(" {title} "), usize::from(room)),
            if focused { styles.title_active } else { styles.title },
        ));
    let inner = block.inner(rect);
    block.render(rect, buf);
    for (r, label, _) in controls {
        text(buf, r, label, styles.menu);
    }
    inner
}

/// What a list column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Time,
    Day,
    Rule,
    Program,
    Parent,
    User,
    Response,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Time | Self::Day => tr!(watch_time_column),
            Self::Rule => tr!(watch_rule_column),
            Self::Program => tr!(watch_program_column),
            Self::Parent => tr!(watch_parent_column),
            Self::User => tr!(watch_user_column),
            Self::Response => tr!(watch_response_column),
        }
    }
}

/// The columns that fit a pane `width` cells wide over `alerts`, each with its width; `None`
/// takes what is left. Every column is at least as wide as its title, and the rule column is
/// as wide as its widest value, up to a limit.
fn columns(width: u16, alerts: &[Alert]) -> Vec<(Field, Option<u16>)> {
    let fixed = |field: Field, cells: u16| (field, Some(cells.max(field.title().width() as u16)));
    let rules = alerts.iter().map(|alert| alert.rule().label().width()).max().unwrap_or_default() as u16;
    let time = if width >= TIME_FROM { fixed(Field::Time, TIME_WIDTH) } else { fixed(Field::Day, DAY_WIDTH) };
    [
        Some(time),
        Some(fixed(Field::Rule, rules.min(RULE_WIDTH))),
        Some((Field::Program, None)),
        (width >= PARENT_FROM).then(|| fixed(Field::Parent, PARENT_WIDTH)),
        (width >= USER_FROM).then(|| fixed(Field::User, USER_WIDTH)),
        (width >= RESPONSE_FROM).then(|| fixed(Field::Response, RESPONSE_WIDTH)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Cells of these widths side by side, one cell apart, with the open one taking what is left.
fn cells(columns: &[(Field, Option<u16>)], area: Rect) -> Vec<Rect> {
    let fixed: u16 =
        columns.iter().filter_map(|(_, width)| *width).sum::<u16>() + columns.len().saturating_sub(1) as u16;
    let open = area.width.saturating_sub(fixed);
    let mut x = area.x;
    columns
        .iter()
        .map(|(_, width)| {
            let width = width.unwrap_or(open);
            let cell = Rect { x, width: width.min(area.right().saturating_sub(x)), ..area };
            x = x.saturating_add(width + 1);
            cell
        })
        .collect()
}

fn draw_list(
    state: &AppState,
    look: &Look<'_>,
    rect: Rect,
    clock: &Zoned,
    spinner: Option<Spinner>,
    unsent: Option<Unsent>,
    buf: &mut Buffer,
) {
    let alerts = &state.alerts;
    let theme = look.theme;
    let styles = &theme.styles;
    let inner = frame(alerts, look, rect, tr!(watch_title), true, buf);
    if inner.is_empty() {
        return;
    }
    if alerts.loaded {
        text(buf, row(inner, SUMMARY_ROW), summary(alerts), emphasis(theme));
    }
    let line = FilterLine {
        filter: &alerts.filter,
        filtering: alerts.filtering,
        shown: alerts.len(),
        total: alerts.log.alerts.len(),
        placeholder: tr!(watch_filter_placeholder),
    };
    filter_line(&line, look, row(inner, FILTER_ROW), buf);
    let header = row(inner, HEADER_ROW);
    let columns = columns(header.width, &alerts.log.alerts);
    if alerts.loaded && !alerts.log.alerts.is_empty() {
        for ((field, _), cell) in columns.iter().zip(cells(&columns, header)) {
            text(buf, cell, field.title(), styles.menu);
        }
    }
    let body = body(rect);
    let mut taken = 0;
    if let Some(error) = &alerts.error {
        for (offset, line) in wrap(error, usize::from(body.width)).into_iter().enumerate() {
            text(buf, row(body, offset as u16), line, styles.notice);
        }
    } else if !alerts.loaded {
        if let Some(spinner) = spinner {
            spinner.draw_centered(tr!(watch_loading), look, dim(theme), body, buf);
        }
    } else if alerts.is_empty() {
        let lines = wrap(&nothing_here(state), usize::from(body.width));
        taken = lines.len();
        for (offset, line) in lines.into_iter().enumerate() {
            text(buf, row(body, offset as u16), line, dim(theme));
        }
    }
    let start = alerts.list_start(usize::from(body.height));
    let end = alerts.len().min(start + usize::from(body.height));
    for (offset, index) in (start..end).enumerate() {
        let Some(alert) = alerts.alert(index) else { continue };
        let line = row(body, offset as u16);
        let selected = index == alerts.cursor();
        let base = if selected { styles.cursor } else { styles.overlay };
        buf.set_style(line, base);
        for ((field, _), cell) in columns.iter().zip(cells(&columns, line)) {
            let (value, style) = value(alert, *field, look, clock, base, selected);
            Line::from(Span::styled(format::truncate_end(&value, usize::from(cell.width)), style)).render(cell, buf);
        }
    }
    taken = taken.max(end - start);
    if alerts.loaded
        && alerts.error.is_none()
        && let Some(aside) = &alerts.log.kept_aside
    {
        let note = tr!(watch_log_kept_aside, path = aside.display());
        let lines = broken(&note, usize::from(body.width));
        let first = taken + KEPT_ASIDE_GAP;
        if first + lines.len() <= usize::from(body.height) {
            for (offset, line) in lines.into_iter().enumerate() {
                text(buf, row(body, (first + offset) as u16), line, dim(theme));
            }
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    match unsent.filter(|_| state.notice.is_none()) {
        Some(unsent) => text(buf, row(footer, 0), unsent.line(), unsent_style(unsent, look)),
        None => {
            if let Some(note) = footer_note(state, clock) {
                text(buf, row(footer, 0), note, dim(theme));
            }
        }
    }
}

/// The text in lines no wider than `width`, a word too long for a line of its own, such as a
/// path, broken where the line runs out.
fn broken(value: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in wrap(value, width) {
        if line.width() <= width {
            lines.push(line);
            continue;
        }
        let mut piece = String::new();
        for character in line.chars() {
            if !piece.is_empty() && piece.width() + character.width().unwrap_or_default() > width {
                lines.push(std::mem::take(&mut piece));
            }
            piece.push(character);
        }
        lines.push(piece);
    }
    lines
}

/// How many alerts there are, how many nobody has read, and what was left out.
fn summary(alerts: &Alerts) -> String {
    let tally = count(alerts.log.alerts.len(), Count::Alert);
    let mut summary = tr!(watch_summary, alerts = tally, unread = alerts.log.unread());
    let suppressed = alerts.log.watch.suppressed;
    if suppressed > 0 {
        summary = format!("{summary}{SEPARATOR}{}", tr!(watch_suppressed, count = suppressed));
    }
    summary
}

/// Why the list is empty: the watcher is off, nothing is watching yet, the filter hides
/// everything, or nothing has happened.
fn nothing_here(state: &AppState) -> String {
    let alerts = &state.alerts;
    if !alerts.filter.is_empty() {
        return tr!(network_no_matches).into();
    }
    if !state.settings.preferences.process_watch {
        return tr!(watch_off).into();
    }
    if alerts.log.watch.source == WatchSource::Off {
        return tr!(watch_not_running).into();
    }
    tr!(watch_none).into()
}

/// What the footer says: a notice, or how the watcher stands.
fn footer_note(state: &AppState, clock: &Zoned) -> Option<String> {
    if let Some(notice) = state.notice.clone() {
        return Some(notice);
    }
    let watch = &state.alerts.log.watch;
    if watch.source == WatchSource::Off {
        return None;
    }
    if watch.learning(std::time::SystemTime::now()) {
        let until = format::stamp(Some(watch.learned_at), clock);
        return Some(tr!(watch_learning_until, until = until));
    }
    if let Some(until) = state.alerts.log.freezing_waits(std::time::SystemTime::now()) {
        return Some(tr!(watch_review_until, until = format::stamp(Some(until), clock)));
    }
    let mut note = tr!(watch_watching, source = watch.source.label());
    if watch.seen > 0 {
        note = format!("{note}{SEPARATOR}{}", tr!(watch_seen, count = watch.seen));
    }
    Some(note)
}

/// What an alert's row shows in `field`, and its style.
fn value(alert: &Alert, field: Field, look: &Look<'_>, clock: &Zoned, base: Style, selected: bool) -> (String, Style) {
    let theme = look.theme;
    let quiet = if selected { base } else { dim(theme) };
    let severity = if selected { base } else { severity_style(Severity::from(alert.level()), theme) };
    match field {
        Field::Time | Field::Day => {
            let at = Some(alert.at);
            let stamp = if field == Field::Day { format::day(at, clock) } else { format::stamp(at, clock) };
            (stamp, if alert.seen { quiet } else { base })
        }
        Field::Rule => (alert.rule().label().to_owned(), severity),
        Field::Program => (alert.program().to_owned(), base),
        Field::Parent => (alert.parent().unwrap_or(UNKNOWN).to_owned(), quiet),
        Field::User => (alert.process.user.clone(), quiet),
        Field::Response => {
            let style = if alert.frozen() && !selected { severity_style(Severity::Alert, theme) } else { quiet };
            (alert.state.label().to_owned(), style)
        }
    }
}

/// Labelled values one under another, long ones wrapped under their label.
struct Facts<'a, 'b> {
    area: Rect,
    y: u16,
    label: u16,
    look: &'b Look<'a>,
}

impl Facts<'_, '_> {
    fn add(&mut self, buf: &mut Buffer, label: &str, value: &str, rows: usize) {
        if value.is_empty() {
            return;
        }
        let width = self.area.width.saturating_sub(self.label);
        let mut lines = wrap(value, usize::from(width));
        if lines.len() > rows {
            lines.truncate(rows);
            if let Some(last) = lines.last_mut() {
                *last = format::truncate_end(&format!("{last} …"), usize::from(width));
            }
        }
        for (index, line) in lines.into_iter().enumerate() {
            if self.y >= self.area.height {
                return;
            }
            let at = row(self.area, self.y);
            if index == 0 {
                text(buf, Rect { width: self.label.min(at.width), ..at }, label, dim(self.look.theme));
            }
            let rest = Rect { x: at.x + self.label.min(at.width), width, ..at };
            text(buf, rest, format::truncate_end(&line, usize::from(rest.width)), self.look.theme.styles.overlay);
            self.y += 1;
        }
    }

    /// A heading with a blank row above it.
    fn heading(&mut self, buf: &mut Buffer, label: &str) {
        if self.y >= self.area.height {
            return;
        }
        self.y += 1;
        text(buf, row(self.area, self.y), label, heading(self.look.theme));
        self.y += 1;
    }

    /// A heading, then lines of its own under it.
    fn section(&mut self, buf: &mut Buffer, label: &str, lines: &[String], rows: usize) {
        self.heading(buf, label);
        for line in lines.iter().take(rows) {
            if self.y >= self.area.height {
                return;
            }
            let at = row(self.area, self.y);
            text(buf, at, format::truncate_end(line, usize::from(at.width)), self.look.theme.styles.overlay);
            self.y += 1;
        }
    }
}

/// Draws the warning at the top of `area`, and returns what is left under it and a blank row.
fn draw_unsent(unsent: Unsent, look: &Look<'_>, area: Rect, buf: &mut Buffer) -> Rect {
    let lines = wrap(&unsent.line(), usize::from(area.width));
    let style = unsent_style(unsent, look);
    let taken = (lines.len() + UNSENT_GAP).min(usize::from(area.height)) as u16;
    for (offset, line) in lines.into_iter().enumerate() {
        text(buf, row(area, offset as u16), line, style);
    }
    Rect { y: area.y + taken, height: area.height - taken, ..area }
}

fn draw_details(alerts: &Alerts, look: &Look<'_>, rect: Rect, clock: &Zoned, unsent: Option<Unsent>, buf: &mut Buffer) {
    let theme = look.theme;
    let title = alerts.selected().map_or(tr!(watch_title), |alert| alert.rule().label());
    let inner = frame(alerts, look, rect, title, false, buf);
    let inner = match unsent {
        Some(unsent) => draw_unsent(unsent, look, inner, buf),
        None => inner,
    };
    let Some(alert) = alerts.selected() else { return };
    if inner.is_empty() {
        return;
    }
    let severity = Severity::from(alert.level());
    let heading_line = format!("{}{SEPARATOR}{}", severity.label(), format::exact_stamp(Some(alert.at), clock));
    text(buf, row(inner, 0), heading_line, severity_style(severity, theme));
    let mut y = 1;
    for line in wrap(&alert.finding.reason(), usize::from(inner.width)) {
        text(buf, row(inner, y), line, theme.styles.overlay);
        y += 1;
    }
    if let Response::Spared(protection) = &alert.response {
        for line in wrap(&protection.reason(), usize::from(inner.width)) {
            text(buf, row(inner, y), line, dim(theme));
            y += 1;
        }
    }
    let body = Rect { y: inner.y + (y + 1).min(inner.height), height: inner.height.saturating_sub(y + 1), ..inner };
    let labels = [
        tr!(watch_command_label),
        tr!(watch_program_label),
        tr!(watch_parent_label),
        tr!(watch_user_label),
        tr!(watch_terminal_label),
        tr!(watch_streams_label),
        tr!(watch_errors_label),
        tr!(watch_state_label),
    ];
    let label = labels.iter().map(|label| label.width() as u16).max().unwrap_or(0) + LABEL_GAP;
    let mut facts = Facts { area: body, y: 0, label, look };
    facts.add(buf, labels[0], &alert.process.command_line(), COMMAND_ROWS);
    facts.add(buf, labels[1], &program(alert), 2);
    facts.add(buf, labels[2], &parent(alert), 1);
    facts.add(buf, labels[3], &alert.process.user, 1);
    facts.add(buf, labels[4], alert.process.terminal.as_deref().unwrap_or_default(), 1);
    facts.add(buf, labels[5], &streams(alert), 1);
    facts.add(buf, labels[6], alert.process.errors.label(), 1);
    facts.add(buf, labels[7], &state_and_response(alert), 1);
    facts.section(buf, tr!(watch_tree_label), &tree(alert), TREE_ROWS);
    if !alert.evidence.connections.is_empty() {
        facts.section(buf, tr!(watch_connections_label), &alert.evidence.connections, CONNECTION_ROWS);
    }
    if let Some(hash) = &alert.evidence.program_hash {
        facts.section(buf, tr!(watch_hash_label), std::slice::from_ref(hash), 1);
    }
    facts.heading(buf, tr!(watch_rules_label));
    if alert.evidence.hits.is_empty() {
        let note = alert
            .evidence
            .missing
            .clone()
            .map_or_else(|| tr!(watch_no_hits).into(), |reason| tr!(watch_evidence_missing, reason = reason));
        for line in wrap(&note, usize::from(body.width)) {
            if facts.y >= body.height {
                return;
            }
            text(buf, row(body, facts.y), line, dim(theme));
            facts.y += 1;
        }
        return;
    }
    for hit in &alert.evidence.hits {
        facts.y = draw_hit(hit, look, body, facts.y, buf);
    }
}

/// The program the process ran, and whether its file is still there.
fn program(alert: &Alert) -> String {
    let Some(program) = &alert.process.program else { return UNKNOWN.into() };
    let mut line = program.display().to_string();
    if alert.process.program_deleted {
        line = format!("{line}{SEPARATOR}{}", tr!(watch_state_gone));
    }
    line
}

/// The parent's program and process number.
fn parent(alert: &Alert) -> String {
    alert.process.parent().map_or_else(String::new, |parent| format!("{} ({})", parent.program_name(), parent.pid))
}

fn streams(alert: &Alert) -> String {
    format!("{}{SEPARATOR}{}", alert.process.input.label(), alert.process.output.label())
}

/// What the process is now, what Borg did when the rule fired, and which process it was.
fn state_and_response(alert: &Alert) -> String {
    let now = alert.state.label();
    let pid = tr!(watch_pid, pid = alert.process.key.pid);
    if alert.response.label() == now {
        return format!("{now}{SEPARATOR}{pid}");
    }
    format!("{now}{SEPARATOR}{}{SEPARATOR}{pid}", alert.response.label())
}

/// The process and its parents, oldest first, each indented under the one that started it.
fn tree(alert: &Alert) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (depth, ancestor) in alert.process.ancestors.iter().rev().enumerate() {
        let command =
            if ancestor.command.is_empty() { ancestor.program_name().to_owned() } else { ancestor.command.clone() };
        lines.push(format!("{:indent$}{} ({})", "", command, ancestor.pid, indent = depth * TREE_STEP));
    }
    let depth = alert.process.ancestors.len() * TREE_STEP;
    lines.push(format!("{:indent$}{} ({})", "", alert.process.command_line(), alert.process.key.pid, indent = depth));
    lines
}

/// One rule match: its name, level and what it looks for, what else could explain it, and whose
/// rule it is. Returns the row after it.
fn draw_hit(hit: &RuleMatch, look: &Look<'_>, area: Rect, mut y: u16, buf: &mut Buffer) -> u16 {
    let theme = look.theme;
    let severity = Severity::from(hit.level);
    let title = format!("{}{SEPARATOR}{}", hit.rule, severity.label());
    let source = rule_source(hit);
    let lines = [Some(title.as_str()), hit.about(), hit.caveat(), Some(source.as_str())];
    let styles = [severity_style(severity, theme), theme.styles.overlay, dim(theme), dim(theme)];
    for (value, style) in lines.into_iter().zip(styles) {
        for line in value.map(|value| wrap(value, usize::from(area.width))).unwrap_or_default() {
            if y >= area.height {
                return y;
            }
            text(buf, row(area, y), line, style);
            y += 1;
        }
    }
    y + 1
}
