//! The startup items screen: everything set to start on its own, newest change first, in one
//! pane, and the selected item's file, program, schedule and rule matches in the other. It
//! shares the network screen's look.

use crate::{
    activity_view::{FOOTER_ROWS, body, dim, emphasis, heading, row, text, wrap},
    format,
    look::Look,
    network_view::{FilterLine, filter_line},
    spinner::Spinner,
};
use borg_core::{AppState, ProgramPresence, RuleHit, Severity, StartupItem, StartupItems, rule_source};
use borg_i18n::{Count, count, tr};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use std::time::Instant;
use unicode_width::UnicodeWidthStr;

mod controls;

pub(crate) use controls::{keybar, panel_mouse, status};

const SUMMARY_ROW: u16 = 0;
const FILTER_ROW: u16 = 1;
const HEADER_ROW: u16 = 2;
/// What a value Borg could not find out shows as.
const UNKNOWN: &str = "—";
const SEPARATOR: &str = " · ";
const LINE_MARK: char = ':';
const RULE_JOIN: &str = ", ";
/// Panes at least this wide show who owns each file.
const OWNER_FROM: u16 = 96;
/// Panes at least this wide show whether each program is there.
const PROGRAM_FROM: u16 = 76;
/// Panes at least this wide show each item's place.
const PLACE_FROM: u16 = 60;
/// Panes at least this wide show the time of each change as well as its day.
const TIME_FROM: u16 = 76;
const MODIFIED_WIDTH: u16 = 16;
const DAY_WIDTH: u16 = 10;
const PLACE_WIDTH: u16 = 21;
const OWNER_WIDTH: u16 = 9;
const PROGRAM_WIDTH: u16 = 8;
const RULES_WIDTH: u16 = 22;
/// Cells between a fact's label and its value.
const LABEL_GAP: u16 = 2;
/// Rows what an item runs may take in the details before it is cut.
const RUNS_ROWS: usize = 6;

pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    panels: [Rect; 2],
    clock: &Zoned,
    now: Instant,
    buf: &mut Buffer,
) {
    let startup = &state.startup_items;
    let Some(side) = startup.side else { return };
    let spinner = Spinner::of(&state.busy, borg_core::Task::StartupItems, now);
    let list = panels[side.index()];
    let details = panels[side.other().index()];
    if !list.is_empty() {
        draw_list(state, look, list, clock, spinner, buf);
    }
    if !details.is_empty() {
        draw_details(startup, look, details, clock, buf);
    }
}

fn frame(startup: &StartupItems, look: &Look<'_>, rect: Rect, title: &str, focused: bool, buf: &mut Buffer) -> Rect {
    let theme = look.theme;
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let controls = crate::results_view::controls(rect, startup.expanded);
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
    Modified,
    Day,
    Place,
    Runs,
    Owner,
    Program,
    Rules,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Modified | Self::Day => tr!(startup_modified_column),
            Self::Place => tr!(startup_place_column),
            Self::Runs => tr!(startup_runs_column),
            Self::Owner => tr!(startup_owner_column),
            Self::Program => tr!(startup_program_column),
            Self::Rules => tr!(startup_rules_column),
        }
    }
}

/// The columns that fit a pane `width` cells wide over `items`, each with its width; `None`
/// takes what is left. Every column is at least as wide as its title. The place and rule
/// columns are as wide as their widest value, up to a limit, and the rule column shows only
/// when an item matched a rule.
fn columns(width: u16, items: &[StartupItem]) -> Vec<(Field, Option<u16>)> {
    let fixed = |field: Field, cells: u16| (field, Some(cells.max(field.title().width() as u16)));
    let fitted = |field: Field, values: Vec<usize>, limit: u16| {
        fixed(field, (values.into_iter().max().unwrap_or_default() as u16).min(limit))
    };
    let places = items.iter().map(|item| item.place.label().width()).collect();
    let rules: Vec<usize> =
        items.iter().map(|item| item.rule_names().join(RULE_JOIN).width()).filter(|&w| w > 0).collect();
    let modified =
        if width >= TIME_FROM { fixed(Field::Modified, MODIFIED_WIDTH) } else { fixed(Field::Day, DAY_WIDTH) };
    [
        Some(modified),
        (width >= PLACE_FROM).then(|| fitted(Field::Place, places, PLACE_WIDTH)),
        Some((Field::Runs, None)),
        (width >= OWNER_FROM).then(|| fixed(Field::Owner, OWNER_WIDTH)),
        (width >= PROGRAM_FROM).then(|| fixed(Field::Program, PROGRAM_WIDTH)),
        (!rules.is_empty()).then(|| fitted(Field::Rules, rules, RULES_WIDTH)),
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

fn draw_list(state: &AppState, look: &Look<'_>, rect: Rect, clock: &Zoned, spinner: Option<Spinner>, buf: &mut Buffer) {
    let startup = &state.startup_items;
    let theme = look.theme;
    let styles = &theme.styles;
    let inner = frame(startup, look, rect, tr!(startup_title), true, buf);
    if inner.is_empty() {
        return;
    }
    if startup.loaded {
        let items = count(startup.report.items.len(), Count::Item);
        let mut summary = tr!(startup_summary, items = items, matched = startup.report.matched());
        if startup.pending.is_some() {
            summary = format!("{summary}{SEPARATOR}{}", tr!(startup_reading));
        }
        text(buf, row(inner, SUMMARY_ROW), summary, emphasis(theme));
    }
    let line = FilterLine {
        filter: &startup.filter,
        filtering: startup.filtering,
        shown: startup.len(),
        total: startup.report.items.len(),
        placeholder: tr!(startup_filter_placeholder),
    };
    filter_line(&line, look, row(inner, FILTER_ROW), buf);
    let header = row(inner, HEADER_ROW);
    let columns = columns(header.width, &startup.report.items);
    if startup.loaded {
        for ((field, _), cell) in columns.iter().zip(cells(&columns, header)) {
            text(buf, cell, field.title(), styles.menu);
        }
    }
    let body = body(rect);
    if let Some(error) = &startup.error {
        for (offset, line) in wrap(error, usize::from(body.width)).into_iter().enumerate() {
            text(buf, row(body, offset as u16), line, styles.notice);
        }
    } else if !startup.loaded {
        if let Some(spinner) = spinner {
            spinner.draw_centered(tr!(startup_loading), look, dim(theme), body, buf);
        }
    } else if startup.is_empty() {
        let note = if startup.filter.is_empty() { tr!(startup_none) } else { tr!(network_no_matches) };
        text(buf, row(body, 0), note, dim(theme));
    }
    let start = startup.list_start(usize::from(body.height));
    let end = startup.len().min(start + usize::from(body.height));
    for (offset, index) in (start..end).enumerate() {
        let Some(item) = startup.item(index) else { continue };
        let line = row(body, offset as u16);
        let selected = index == startup.cursor();
        let base = if selected { styles.cursor } else { styles.overlay };
        buf.set_style(line, base);
        for ((field, _), cell) in columns.iter().zip(cells(&columns, line)) {
            let (value, style) = value(item, *field, look, clock, base, selected);
            Line::from(Span::styled(format::truncate_end(&value, usize::from(cell.width)), style)).render(cell, buf);
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    if let Some(note) = footer_note(state) {
        text(buf, row(footer, 0), note, dim(theme));
    }
}

/// What the footer says: a notice, the places Borg could not read, or the rule files it left out.
fn footer_note(state: &AppState) -> Option<String> {
    let report = &state.startup_items.report;
    if let Some(notice) = state.notice.clone() {
        return Some(notice);
    }
    if !report.unread.is_empty() {
        let paths = report.unread.iter().map(|(path, _)| path.display().to_string()).collect::<Vec<_>>().join(", ");
        return Some(tr!(startup_unread, paths = paths));
    }
    (!report.problems.is_empty()).then(|| tr!(rule_files_left_out, count = report.problems.len()))
}

/// The style of a rule's level: alerts stand out, notices recede.
fn severity_style(severity: Severity, look: &Look<'_>) -> Style {
    let theme = look.theme;
    match severity {
        Severity::Alert => theme.styles.notice.add_modifier(Modifier::BOLD),
        Severity::Warning => emphasis(theme),
        Severity::Notice => dim(theme),
    }
}

/// What an item's row shows in `field`, and its style.
fn value(
    item: &StartupItem,
    field: Field,
    look: &Look<'_>,
    clock: &Zoned,
    base: Style,
    selected: bool,
) -> (String, Style) {
    let theme = look.theme;
    let quiet = if selected { base } else { dim(theme) };
    let plain = if item.found.disabled { quiet } else { base };
    match field {
        Field::Modified | Field::Day => {
            let stamp = if field == Field::Day {
                format::day(item.modified, clock)
            } else {
                format::stamp(item.modified, clock)
            };
            if stamp.is_empty() { (UNKNOWN.into(), quiet) } else { (stamp, plain) }
        }
        Field::Place => (item.place.label().to_owned(), quiet),
        Field::Runs => (runs(item).to_owned(), plain),
        Field::Owner => (item.owner.clone().unwrap_or_else(|| UNKNOWN.into()), quiet),
        Field::Program => match item.presence {
            ProgramPresence::Present => (item.presence.label().to_owned(), quiet),
            ProgramPresence::Missing => {
                let style = if selected { base } else { theme.styles.notice };
                (item.presence.label().to_owned(), style.add_modifier(Modifier::BOLD))
            }
            ProgramPresence::Unknown => (UNKNOWN.into(), quiet),
        },
        Field::Rules => {
            let style = match item.worst() {
                _ if selected => base,
                Some(level) => severity_style(level.into(), look),
                None => quiet,
            };
            (item.rule_names().join(RULE_JOIN), style)
        }
    }
}

/// What an item runs, or its name when its file says nothing runs.
fn runs(item: &StartupItem) -> &str {
    if item.found.runs.is_empty() { &item.found.name } else { &item.found.runs }
}

/// The item's file, and its line when the file holds one item per line.
fn file_line(item: &StartupItem) -> String {
    match item.found.line {
        Some(line) => format!("{}{LINE_MARK}{line}", item.file.display()),
        None => item.file.display().to_string(),
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
}

fn draw_details(startup: &StartupItems, look: &Look<'_>, rect: Rect, clock: &Zoned, buf: &mut Buffer) {
    let theme = look.theme;
    let Some(item) = startup.selected() else {
        frame(startup, look, rect, tr!(startup_title), false, buf);
        return;
    };
    let inner = frame(startup, look, rect, &item.found.name, false, buf);
    if inner.is_empty() {
        return;
    }
    text(buf, row(inner, 0), item.place.label(), emphasis(theme));
    text(buf, row(inner, 1), format::truncate_start(&file_line(item), usize::from(inner.width)), theme.styles.overlay);
    let body = Rect { y: inner.y + 3.min(inner.height), height: inner.height.saturating_sub(3), ..inner };
    let labels = [
        tr!(startup_runs_label),
        tr!(startup_program_label),
        tr!(startup_when_label),
        tr!(startup_user_label),
        tr!(startup_owner_label),
        tr!(startup_modified_label),
    ];
    let label = labels.iter().map(|label| label.width() as u16).max().unwrap_or(0) + LABEL_GAP;
    let mut facts = Facts { area: body, y: 0, label, look };
    facts.add(buf, labels[0], &item.found.runs, RUNS_ROWS);
    facts.add(buf, labels[1], &program(item), 2);
    facts.add(buf, labels[2], item.found.when.as_deref().unwrap_or_default(), 2);
    facts.add(buf, labels[3], item.found.user.as_deref().unwrap_or_default(), 1);
    facts.add(buf, labels[4], item.owner.as_deref().unwrap_or_default(), 1);
    facts.add(buf, labels[5], &format::exact_stamp(item.modified, clock), 1);
    let mut y = facts.y + 1;
    let notes = [item.found.disabled.then(|| tr!(startup_disabled)), item.read_only.then(|| tr!(startup_read_only))];
    let noted = y;
    for note in notes.into_iter().flatten() {
        for line in wrap(note, usize::from(body.width)) {
            text(buf, row(body, y), line, dim(theme));
            y += 1;
        }
    }
    if y > noted {
        y += 1;
    }
    text(buf, row(body, y), tr!(startup_rules_label), heading(theme));
    y += 1;
    if item.hits.is_empty() {
        text(buf, row(body, y), tr!(startup_no_hits), dim(theme));
        return;
    }
    for hit in &item.hits {
        y = draw_hit(hit, look, body, y, buf);
    }
}

/// The program an item runs, whether it is there, and where a bare name was found.
fn program(item: &StartupItem) -> String {
    let Some(program) = &item.found.program else { return String::new() };
    let mut parts = vec![program.clone()];
    if let Some(found) = &item.resolved {
        parts.push(tr!(startup_found_at, path = found.display()));
    } else if item.presence != ProgramPresence::Unknown {
        parts.push(item.presence.label().to_owned());
    }
    parts.join(SEPARATOR)
}

/// One rule match: its name, level and where it matched, what it looks for, what else could
/// explain it, and whose rule it is. Returns the row after it.
fn draw_hit(hit: &RuleHit, look: &Look<'_>, area: Rect, mut y: u16, buf: &mut Buffer) -> u16 {
    let theme = look.theme;
    let severity = Severity::from(hit.rule.level);
    let title = if hit.in_program {
        tr!(startup_hit_in_program, rule = hit.rule.rule, level = severity.label())
    } else {
        tr!(startup_hit_in_file, rule = hit.rule.rule, level = severity.label())
    };
    let source = rule_source(&hit.rule);
    let lines = [Some(title.as_str()), hit.rule.about(), hit.rule.caveat(), Some(source.as_str())];
    let styles = [severity_style(severity, look), theme.styles.overlay, dim(theme), dim(theme)];
    for (value, style) in lines.into_iter().zip(styles) {
        for line in value.map(|value| wrap(value, usize::from(area.width))).unwrap_or_default() {
            text(buf, row(area, y), line, style);
            y += 1;
        }
    }
    y + 1
}
