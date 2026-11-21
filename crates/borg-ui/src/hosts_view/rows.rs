//! The Hosts page as a table: a row for each host, offline ones included, with the figures
//! that fit the pane.

use std::time::Instant;

use borg_core::{DiskFigures, HostCard, Hosts, Reach};
use borg_i18n::tr;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use super::{UNKNOWN, concern_style, mark, percent, version_text};
use crate::{
    activity_view::{body, dim, inner, row, text},
    format,
    look::Look,
};

const HEADER_ROW: u16 = 2;
/// Cells a share takes, then the other fixed columns.
const SHARE_WIDTH: u16 = 4;
const ALERTS_WIDTH: u16 = 6;
const CONTAINERS_WIDTH: u16 = 7;
const VERSION_WIDTH: u16 = 9;
const SEEN_WIDTH: u16 = 12;
/// Panes at least this wide show each column.
const LOAD_FROM: u16 = 46;
const ALERTS_FROM: u16 = 56;
const VERSION_FROM: u16 = 68;
const CONTAINERS_FROM: u16 = 80;
const SEEN_FROM: u16 = 94;
/// What the Seen column shows while Borg waits for a host's first answer.
const WAITING: &str = "…";

/// What a column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Host,
    Cpu,
    Load,
    Ram,
    Disk,
    Alerts,
    Containers,
    Version,
    Seen,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Host => tr!(hosts_host_column),
            Self::Cpu => tr!(summary_cpu),
            Self::Load => tr!(summary_load),
            Self::Ram => tr!(summary_ram),
            Self::Disk => tr!(summary_disk),
            Self::Alerts => tr!(hosts_alerts_column),
            Self::Containers => tr!(hosts_containers_column),
            Self::Version => tr!(hosts_version_column),
            Self::Seen => tr!(hosts_seen_column),
        }
    }

    /// Numbers line up on the right.
    fn numeric(self) -> bool {
        matches!(self, Self::Cpu | Self::Load | Self::Ram | Self::Disk | Self::Alerts | Self::Containers)
    }
}

/// The columns that fit a pane `width` cells wide, each with its width; `None` takes what is
/// left. Every column is at least as wide as its title.
fn columns(width: u16) -> Vec<(Field, Option<u16>)> {
    let fixed = |field: Field, cells: u16| (field, Some(cells.max(field.title().width() as u16)));
    [
        Some((Field::Host, None)),
        Some(fixed(Field::Cpu, SHARE_WIDTH)),
        (width >= LOAD_FROM).then(|| fixed(Field::Load, SHARE_WIDTH)),
        Some(fixed(Field::Ram, SHARE_WIDTH)),
        Some(fixed(Field::Disk, SHARE_WIDTH)),
        (width >= ALERTS_FROM).then(|| fixed(Field::Alerts, ALERTS_WIDTH)),
        (width >= CONTAINERS_FROM).then(|| fixed(Field::Containers, CONTAINERS_WIDTH)),
        (width >= VERSION_FROM).then(|| fixed(Field::Version, VERSION_WIDTH)),
        (width >= SEEN_FROM).then(|| fixed(Field::Seen, SEEN_WIDTH)),
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

pub(super) fn draw(hosts: &Hosts, look: &Look<'_>, rect: Rect, clock: &Zoned, now: Instant, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let header = row(inner(rect), HEADER_ROW);
    let columns = columns(header.width);
    for ((field, _), cell) in columns.iter().zip(cells(&columns, header)) {
        let title = if field.numeric() {
            format::pad_start(field.title(), usize::from(cell.width))
        } else {
            field.title().to_owned()
        };
        text(buf, cell, title, styles.menu);
    }
    let body = body(rect);
    let start = hosts.list_start(usize::from(body.height));
    let end = hosts.rows.len().min(start + usize::from(body.height));
    for (offset, index) in (start..end).enumerate() {
        let Some(card) = hosts.card(index) else { continue };
        let line = row(body, offset as u16);
        let selected = index == hosts.cursor();
        let base = if selected { styles.cursor } else { styles.overlay };
        buf.set_style(line, base);
        for ((field, _), cell) in columns.iter().zip(cells(&columns, line)) {
            let width = usize::from(cell.width);
            if *field == Field::Host {
                let (mark, style) = mark(card, theme);
                let mark_style = if selected { base } else { style };
                let name = format::truncate_end(card.name(), width.saturating_sub(2));
                Line::from(vec![Span::styled(mark, mark_style), Span::styled(format!(" {name}"), base)])
                    .render(cell, buf);
                continue;
            }
            let (value, style) = value(card, *field, look, clock, now);
            let style = if selected { base } else { style.unwrap_or(base) };
            let value =
                if field.numeric() { format::pad_start(&value, width) } else { format::truncate_end(&value, width) };
            Line::from(Span::styled(value, style)).render(cell, buf);
        }
    }
}

/// What a host's row shows in `field`, and its style when the value calls for one.
fn value(card: &HostCard, field: Field, look: &Look<'_>, clock: &Zoned, now: Instant) -> (String, Option<Style>) {
    let theme = look.theme;
    let figures = card.figures.as_ref().filter(|_| !card.offline());
    let share = |value: Option<u32>| value.map_or_else(|| UNKNOWN.to_owned(), percent);
    let concern = |severity| Some(concern_style(severity, theme));
    match field {
        Field::Host => (card.name().to_owned(), None),
        Field::Cpu => (share(figures.and_then(|figures| figures.cpu)), None),
        Field::Load => {
            let figures = figures.filter(|figures| figures.load_share().is_some());
            figures.map_or((UNKNOWN.to_owned(), None), |figures| {
                (share(figures.load_share()), concern(figures.load_concern()))
            })
        }
        Field::Ram => {
            let memory = figures.and_then(|figures| figures.memory.map(|memory| (memory, figures.memory_concern())));
            memory.map_or((UNKNOWN.to_owned(), None), |(memory, severity)| (percent(memory.share()), concern(severity)))
        }
        Field::Disk => {
            let disk = figures.and_then(|figures| figures.disk.as_ref().map(|disk| (disk, figures.disk_concern())));
            disk.map_or((UNKNOWN.to_owned(), None), |(disk, severity)| {
                (percent(DiskFigures::share(disk)), concern(severity))
            })
        }
        Field::Alerts => {
            match figures.and_then(|figures| figures.watch.map(|watch| (watch, figures.watch_concern()))) {
                Some((watch, severity)) if watch.on => {
                    (format!("{}", watch.unread_alerts + watch.unread_warnings), concern(severity))
                }
                _ => (UNKNOWN.to_owned(), Some(dim(theme))),
            }
        }
        Field::Containers => {
            let containers = figures.and_then(|figures| figures.containers.map(|c| (c, figures.containers_concern())));
            containers.map_or((UNKNOWN.to_owned(), Some(dim(theme))), |(containers, severity)| {
                (format!("{}/{}", containers.running, containers.total), concern(severity))
            })
        }
        Field::Version => {
            let style = card.older().then(|| dim(theme));
            (version_text(card), style)
        }
        Field::Seen => match &card.reach {
            Reach::Answering => (tr!(hosts_now).to_owned(), Some(dim(theme))),
            Reach::Waiting => (WAITING.to_owned(), Some(dim(theme))),
            Reach::Silent { since } => (format::span(now.saturating_duration_since(*since)), Some(theme.styles.notice)),
            Reach::Failed(_) => (tr!(hosts_failed).to_owned(), Some(theme.styles.notice)),
            Reach::Offline => {
                let seen = card.last_seen.map(|seen| format::modified(Some(seen), clock));
                (seen.unwrap_or_else(|| tr!(hosts_offline).to_owned()), Some(dim(theme)))
            }
        },
    }
}
