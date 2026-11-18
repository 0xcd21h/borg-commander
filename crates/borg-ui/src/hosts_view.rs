//! The Hosts page: a card for each paired host with its load, memory, fullest disk, process
//! watcher and containers, or a row for each in a table, and the selected host's facts in the
//! other pane.

use std::time::{Duration, Instant, SystemTime};

use borg_core::{
    AppState, CARD_GAP, CARD_HEIGHT, ContainerFigures, DiskFigures, HOSTS_EVERY, HostCard, HostFigures, Hosts, Reach,
    Severity, WHOLE, WatchFigures,
};
use borg_i18n::{Count, count, tr};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    Theme,
    activity_view::{FOOTER_ROWS, Fill, dim, emphasis, inner, meter, row, severity_style, text, wrap},
    format,
    look::Look,
    network_view::{FilterLine, filter_line},
    spinner::Spinner,
};

mod controls;
mod details;
mod rows;

pub(crate) use controls::{keybar, panel_mouse, status};

const SUMMARY_ROW: u16 = 0;
const FILTER_ROW: u16 = 1;
/// Rows above the cards: the summary and the filter.
const CARDS_TOP: u16 = 2;
const SEPARATOR: &str = " · ";
/// What a figure the host did not send shows as.
const UNKNOWN: &str = "—";
/// Marks before a host's name: it answers, it went quiet or failed, Borg waits for its first
/// answer, the relay says it is offline.
const ANSWERING_MARK: &str = "●";
const QUIET_MARK: &str = "◐";
const WAITING_MARK: &str = "◌";
const OFFLINE_MARK: &str = "○";
/// After the version of a host that runs an older Borg.
const OLDER_MARK: &str = "↑";
/// Cells a bar's percentage takes, the text after it, and the least its meter takes.
const PERCENT_CELLS: u16 = 4;
const EXTRA_CELLS: u16 = 12;
const METER_LEAST: u16 = 4;
/// Cells between a card's border and its text, on each side.
const CARD_PADDING: u16 = 1;
/// Cells a card's title keeps for its mark, the spaces and the corners.
const TITLE_CHROME: usize = 8;
const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
/// Learning time left counts in days from this many days.
const DAYS_FROM: u64 = 2;
/// Hundredths of a percent in a percent.
const HUNDREDTHS: u32 = 100;

pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    panels: [Rect; 2],
    clock: &Zoned,
    now: Instant,
    buf: &mut Buffer,
) {
    let hosts = &state.hosts;
    let Some(side) = hosts.side else { return };
    let list = panels[side.index()];
    let details = panels[side.other().index()];
    if !list.is_empty() {
        draw_list(state, look, list, clock, now, buf);
    }
    if !details.is_empty() {
        details::draw(hosts, look, details, clock, now, buf);
    }
}

fn draw_list(state: &AppState, look: &Look<'_>, rect: Rect, clock: &Zoned, now: Instant, buf: &mut Buffer) {
    let hosts = &state.hosts;
    let theme = look.theme;
    let inner = crate::results_view::pane_frame(hosts.expanded, look, rect, tr!(hosts_title), true, buf);
    if inner.is_empty() {
        return;
    }
    if hosts.listed {
        text(buf, row(inner, SUMMARY_ROW), summary(hosts), emphasis(theme));
    }
    let line = FilterLine {
        filter: &hosts.filter,
        filtering: hosts.filtering,
        shown: hosts.rows.len(),
        total: hosts.cards.len(),
        placeholder: tr!(hosts_filter_placeholder),
        esc_clears: false,
    };
    filter_line(&line, look, row(inner, FILTER_ROW), buf);
    let area = cards_body(rect);
    if let Some(error) = &hosts.error {
        wrapped(buf, area, error, theme.styles.notice);
    } else if !hosts.listed {
        if let Some(spinner) = Spinner::of(&state.busy, borg_core::Task::Hosts, now) {
            spinner.draw_centered(tr!(hosts_loading), look, dim(theme), area, buf);
        }
    } else if hosts.rows.is_empty() {
        let note = if hosts.cards.is_empty() { tr!(remote_no_hosts) } else { tr!(network_no_matches) };
        wrapped(buf, area, note, dim(theme));
    } else {
        match hosts.columns() {
            Some(columns) => draw_cards(hosts, look, area, columns, now, buf),
            None => rows::draw(hosts, look, rect, clock, now, buf),
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    if let Some(notice) = &state.notice {
        text(buf, footer, notice.as_str(), dim(theme));
    } else if hosts.columns().is_some()
        && let Some(line) = offline_line(hosts, clock)
    {
        text(buf, footer, line, dim(theme));
    }
}

/// How many hosts answer, of how many, and how often Borg asks.
fn summary(hosts: &Hosts) -> String {
    tr!(hosts_summary, answering = hosts.online(), hosts = hosts.cards.len(), every = format::span(HOSTS_EVERY))
}

/// The hosts the relay says are offline, with when each last answered.
fn offline_line(hosts: &Hosts, clock: &Zoned) -> Option<String> {
    let names: Vec<String> = hosts
        .offline()
        .map(|card| match card.last_seen {
            Some(seen) => tr!(hosts_seen_at, name = card.name(), since = format::modified(Some(seen), clock)),
            None => card.name().to_owned(),
        })
        .collect();
    (!names.is_empty()).then(|| tr!(hosts_offline_line, hosts = names.join(SEPARATOR)))
}

/// Where the cards go: under the summary and the filter, above the footer.
pub(crate) fn cards_body(rect: Rect) -> Rect {
    let inner = inner(rect);
    Rect {
        y: inner.y.saturating_add(CARDS_TOP).min(inner.bottom()),
        height: inner.height.saturating_sub(CARDS_TOP + FOOTER_ROWS),
        ..inner
    }
}

/// Rows of cards that fit `area`.
pub(crate) fn card_lines(area: Rect) -> usize {
    usize::from(area.height / CARD_HEIGHT)
}

/// Where the card at `row` sits in `area`, `columns` across, with the row of cards `first` at the
/// top; none when it is out of sight.
pub(crate) fn card_rect(area: Rect, columns: usize, first: usize, row: usize) -> Option<Rect> {
    let across = u16::try_from(columns.max(1)).ok()?;
    let width = area.width.saturating_sub(CARD_GAP * (across - 1)) / across;
    let line = u16::try_from((row / columns.max(1)).checked_sub(first)?).ok()?;
    let y = area.y.checked_add(line.checked_mul(CARD_HEIGHT)?)?;
    if y.saturating_add(CARD_HEIGHT) > area.bottom() {
        return None;
    }
    let column = u16::try_from(row % columns.max(1)).ok()?;
    Some(Rect { x: area.x + column * (width + CARD_GAP), y, width, height: CARD_HEIGHT })
}

fn draw_cards(hosts: &Hosts, look: &Look<'_>, area: Rect, columns: usize, now: Instant, buf: &mut Buffer) {
    let first = hosts.list_start(card_lines(area));
    for row in first * columns..hosts.reachable() {
        let (Some(rect), Some(card)) = (card_rect(area, columns, first, row), hosts.card(row)) else { break };
        draw_card(card, row == hosts.cursor(), look, rect, now, buf);
    }
}

fn draw_card(card: &HostCard, selected: bool, look: &Look<'_>, area: Rect, now: Instant, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let concern = card.figures.as_ref().and_then(HostFigures::worst);
    let border = concern.map_or_else(|| dim(theme), |severity| severity_style(severity, theme));
    let (mark, mark_style) = mark(card, theme);
    let name_style = if selected { styles.cursor } else { emphasis(theme) };
    let version = version_text(card);
    let room = usize::from(area.width).saturating_sub(version.width() + TITLE_CHROME);
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(mark, mark_style),
        Span::raw(" "),
        Span::styled(format::truncate_end(card.name(), room), name_style),
        Span::raw(" "),
    ]);
    let mut block = Block::bordered()
        .border_type(if selected { BorderType::Thick } else { BorderType::Rounded })
        .border_style(border)
        .style(styles.overlay)
        .title(title);
    if !version.is_empty() {
        block = block.title(Line::styled(format!(" {version} "), dim(theme)).right_aligned());
    }
    if let Some(age) = card.stale(now) {
        let age = tr!(hosts_age, time = format::span(age));
        block = block.title_bottom(Line::styled(format!(" {age} "), dim(theme)).right_aligned());
    }
    let inside = block.inner(area);
    block.render(area, buf);
    let inside = Rect {
        x: inside.x.saturating_add(CARD_PADDING),
        width: inside.width.saturating_sub(2 * CARD_PADDING),
        ..inside
    };
    match (&card.reach, &card.figures) {
        (Reach::Failed(error), _) => wrapped(buf, inside, error, styles.notice),
        (_, Some(figures)) => card_figures(card, figures, look, inside, now, buf),
        (Reach::Waiting, None) => text(buf, row(inside, 0), tr!(hosts_asking), dim(theme)),
        (Reach::Silent { since }, None) => text(buf, row(inside, 0), silent(*since, now), styles.notice),
        (_, None) => no_figures(card, look, inside, buf),
    }
}

/// The mark before a host's name, and its style.
pub(crate) fn mark(card: &HostCard, theme: &Theme) -> (&'static str, Style) {
    match card.reach {
        Reach::Answering => {
            let style = card
                .figures
                .as_ref()
                .and_then(HostFigures::worst)
                .map_or_else(|| emphasis(theme), |severity| severity_style(severity, theme));
            (ANSWERING_MARK, style)
        }
        Reach::Silent { .. } | Reach::Failed(_) => (QUIET_MARK, theme.styles.notice),
        Reach::Waiting => (WAITING_MARK, dim(theme)),
        Reach::Offline => (OFFLINE_MARK, dim(theme)),
    }
}

/// The host's Borg version, marked when it is older than this one.
pub(crate) fn version_text(card: &HostCard) -> String {
    match card.info.version.as_deref() {
        Some(version) if card.older() => format!("{version} {OLDER_MARK}"),
        Some(version) => version.to_owned(),
        None => String::new(),
    }
}

/// The CPU, RAM and disk bars, then the watcher and how long the host has been up.
fn card_figures(card: &HostCard, figures: &HostFigures, look: &Look<'_>, area: Rect, now: Instant, buf: &mut Buffer) {
    let theme = look.theme;
    let labels = [tr!(summary_cpu), tr!(summary_ram), tr!(summary_disk)];
    let label = borg_core::label_width(&labels);
    let memory = figures.memory.filter(|memory| memory.total > 0);
    let bars = [
        (figures.cpu, figures.load_concern(), load_extra(figures)),
        (
            memory.map(|memory| memory.share()),
            figures.memory_concern(),
            memory.map(|memory| format!("{}/{}", format::size(memory.used), format::size(memory.total))),
        ),
        (
            figures.disk.as_ref().map(DiskFigures::share),
            figures.disk_concern(),
            figures.disk.as_ref().map(|disk| tr!(hosts_free, size = format::size(disk.available))),
        ),
    ];
    for (offset, (title, (share, concern, extra))) in labels.into_iter().zip(bars).enumerate() {
        let bar = Bar { label: title, label_cells: label, share, concern, extra: extra.unwrap_or_default() };
        bar.draw(look, row(area, offset as u16), buf);
    }
    match card.reach {
        Reach::Silent { since } => text(buf, row(area, 3), silent(since, now), theme.styles.notice),
        _ => {
            if let Some(watch) = figures.watch {
                text(
                    buf,
                    row(area, 3),
                    watch_text(&watch, SystemTime::now()),
                    concern_style(figures.watch_concern(), theme),
                );
            }
        }
    }
    let mut parts = Vec::new();
    if let Some(uptime) = figures.uptime {
        parts.push((up_text(uptime), theme.styles.overlay));
    }
    if let Some(containers) = figures.containers.filter(|containers| containers.total > 0) {
        let part = (containers_text(containers), concern_style(figures.containers_concern(), theme));
        // Containers in trouble come first, so a narrow card cuts the uptime instead.
        if containers.troubled > 0 {
            parts.insert(0, part);
        } else {
            parts.push(part);
        }
    }
    joined(buf, row(area, 4), parts, theme);
}

/// What follows the CPU bar: how many processes wait for a core, or the load's share of them.
fn load_extra(figures: &HostFigures) -> Option<String> {
    match figures.waiting() {
        Some(waiting) => Some(borg_core::waiting_text(waiting)),
        None => figures.load_share().map(|share| tr!(hosts_load, share = percent(share))),
    }
}

/// A label, a meter, a percentage and a note, the meter taking the room the others leave.
struct Bar<'a> {
    label: &'a str,
    label_cells: u16,
    share: Option<u32>,
    concern: Option<Severity>,
    extra: String,
}

impl Bar<'_> {
    fn draw(&self, look: &Look<'_>, line: Rect, buf: &mut Buffer) {
        let theme = look.theme;
        let label_cells = self.label_cells.min(line.width);
        text(buf, Rect { width: label_cells, ..line }, self.label, dim(theme));
        let after = |x: u16| Rect { x, width: line.right().saturating_sub(x), ..line };
        let start = line.x.saturating_add(label_cells + 1).min(line.right());
        let Some(share) = self.share else {
            text(buf, after(start), UNKNOWN, dim(theme));
            return;
        };
        let style = concern_style(self.concern, theme);
        let room = line.right().saturating_sub(start);
        let mut extra = EXTRA_CELLS;
        let mut cells = room.saturating_sub(1 + PERCENT_CELLS + 2 + extra);
        if cells < METER_LEAST {
            extra = 0;
            cells = room.saturating_sub(1 + PERCENT_CELLS);
        }
        let tone = if self.concern.is_some() { Fill::Warning } else { Fill::Accent };
        meter(buf, Rect { x: start, width: cells, ..line }, f64::from(share) / f64::from(WHOLE), tone, look);
        let x = start.saturating_add(cells + 1).min(line.right());
        let width = PERCENT_CELLS.min(line.right().saturating_sub(x));
        text(buf, Rect { x, width, ..line }, format::pad_start(&percent(share), usize::from(PERCENT_CELLS)), style);
        if extra > 0 {
            let x = x.saturating_add(PERCENT_CELLS + 2).min(line.right());
            let rest = after(x);
            text(buf, rest, format::truncate_end(&self.extra, usize::from(rest.width)), style);
        }
    }
}

/// A host that answers without figures: its Borg is too old to send them.
fn no_figures(card: &HostCard, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let width = usize::from(area.width);
    let version = card.info.version.as_deref().unwrap_or(tr!(remote_unknown));
    let mut lines = wrap(&tr!(hosts_no_figures, version = version), width);
    if card.older() {
        lines.extend(wrap(tr!(hosts_update_for_figures), width));
    }
    for (offset, line) in lines.into_iter().take(usize::from(area.height)).enumerate() {
        text(buf, row(area, offset as u16), line, dim(theme));
    }
    let mut parts = vec![(tr!(hosts_sessions, count = card.sessions.len()), theme.styles.overlay)];
    if let Some(latency) = card.info.latency_ms {
        parts.push((tr!(hosts_latency, ms = latency), dim(theme)));
    }
    joined(buf, row(area, area.height.saturating_sub(1)), parts, theme);
}

/// Parts of a line one after another, between separators.
fn joined(buf: &mut Buffer, line: Rect, parts: Vec<(String, Style)>, theme: &Theme) {
    let mut spans = Vec::new();
    for (index, (value, style)) in parts.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(SEPARATOR, dim(theme)));
        }
        spans.push(Span::styled(value, style));
    }
    if !line.is_empty() {
        Line::from(spans).render(line, buf);
    }
}

/// Lines of `value` wrapped to `area`, as many as fit.
fn wrapped(buf: &mut Buffer, area: Rect, value: &str, style: Style) {
    for (offset, line) in wrap(value, usize::from(area.width)).into_iter().take(usize::from(area.height)).enumerate() {
        text(buf, row(area, offset as u16), line, style);
    }
}

pub(crate) fn concern_style(concern: Option<Severity>, theme: &Theme) -> Style {
    concern.map_or(theme.styles.overlay, |severity| severity_style(severity, theme))
}

/// A share in hundredths of a percent, as a whole percentage.
pub(crate) fn percent(share: u32) -> String {
    format!("{}%", (share + HUNDREDTHS / 2) / HUNDREDTHS)
}

pub(crate) fn silent(since: Instant, now: Instant) -> String {
    tr!(hosts_silent, time = format::span(now.saturating_duration_since(since)))
}

/// How long the host has been up, in its largest unit.
fn up_text(seconds: u64) -> String {
    let (amount, noun) = if seconds >= DAY {
        (seconds / DAY, Count::Day)
    } else if seconds >= HOUR {
        (seconds / HOUR, Count::Hour)
    } else if seconds >= MINUTE {
        (seconds / MINUTE, Count::Minute)
    } else {
        (seconds, Count::Second)
    };
    tr!(hosts_up, time = count(usize::try_from(amount).unwrap_or(usize::MAX), noun))
}

/// The process watcher: what nobody has read yet and what Borg froze, or how it stands.
pub(crate) fn watch_text(watch: &WatchFigures, now: SystemTime) -> String {
    if !watch.on {
        return tr!(hosts_watch_off).into();
    }
    let mut parts = Vec::new();
    if watch.frozen > 0 {
        parts.push(tr!(hosts_frozen, processes = count(watch.frozen, Count::Process)));
    }
    let unread = watch.unread_alerts + watch.unread_warnings;
    if unread > 0 {
        parts.push(tr!(hosts_unread, alerts = count(unread, Count::Alert)));
    }
    if !parts.is_empty() {
        return parts.join(SEPARATOR);
    }
    if let Some(until) = watch.learning_until {
        return tr!(hosts_learning, left = left(until.duration_since(now).unwrap_or_default()));
    }
    tr!(hosts_watch_quiet).into()
}

/// Time left, in days once there are a few of them.
fn left(time: Duration) -> String {
    let seconds = time.as_secs();
    if seconds >= DAYS_FROM * DAY {
        return count(usize::try_from(seconds.div_ceil(DAY)).unwrap_or(usize::MAX), Count::Day);
    }
    format::span(time)
}

pub(crate) fn containers_text(containers: ContainerFigures) -> String {
    if containers.troubled > 0 {
        return tr!(hosts_containers_troubled, troubled = containers.troubled, total = containers.total);
    }
    tr!(hosts_containers, running = containers.running, total = containers.total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fill_the_width_and_stop_at_the_bottom() {
        let area = Rect::new(1, 3, 78, 16);
        assert_eq!(card_rect(area, 2, 0, 0), Some(Rect::new(1, 3, 38, CARD_HEIGHT)));
        assert_eq!(card_rect(area, 2, 0, 1), Some(Rect::new(40, 3, 38, CARD_HEIGHT)));
        assert_eq!(card_rect(area, 2, 0, 3), Some(Rect::new(40, 10, 38, CARD_HEIGHT)));
        assert_eq!(card_rect(area, 2, 0, 4), None);
        assert_eq!(card_rect(area, 2, 1, 1), None);
        assert_eq!(card_rect(area, 2, 1, 2), Some(Rect::new(1, 3, 38, CARD_HEIGHT)));
        assert_eq!(card_lines(area), 2);
    }

    #[test]
    fn uptime_reads_in_its_largest_unit() {
        assert_eq!(up_text(16 * DAY + 3 * HOUR), "Up 16 days");
        assert_eq!(up_text(5 * HOUR + 59 * MINUTE), "Up 5 hours");
        assert_eq!(up_text(42), "Up 42 seconds");
    }

    #[test]
    fn the_watcher_says_what_needs_reading_first() {
        let now = SystemTime::now();
        let quiet = WatchFigures { on: true, ..Default::default() };
        assert_eq!(watch_text(&quiet, now), "Watcher on, nothing unread");
        let learning = WatchFigures { learning_until: Some(now + Duration::from_secs(3 * DAY)), ..quiet };
        assert_eq!(watch_text(&learning, now), "Watcher learning, 3 days left");
        let busy = WatchFigures { unread_alerts: 1, unread_warnings: 1, frozen: 1, ..learning };
        assert_eq!(watch_text(&busy, now), "1 process frozen · 2 alerts unread");
        assert_eq!(watch_text(&WatchFigures::default(), now), "Watcher off");
    }
}
