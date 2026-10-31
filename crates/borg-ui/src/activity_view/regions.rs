//! A process's memory regions, a region's page map and the snapshot of its bytes.

use super::files::file_preview;
use super::{dim, emphasis, heading, metrics, readable, row, size, sized_line, sizes, text, wrap};
use crate::{
    Theme,
    format::{self, Count},
    look::Look,
    spinner::Spinner,
};
use borg_core::{
    Activity, ActivityPage, CENSUS_LEGEND, MemoryCensus, MemoryGroup, MemoryProtection, PageClass, RegionRow,
    SnapshotRow, census_grid,
};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

const LARGEST: usize = 6;

pub(super) fn regions(activity: &Activity, theme: &Theme, inner: Rect, body: Rect, buf: &mut Buffer) {
    let columns = [tr!(memory_footprint_column), tr!(memory_resident_column), tr!(memory_mapped_column)];
    text(
        buf,
        row(inner, 2),
        sized_line(tr!(memory_region_name), &columns.map(String::from), inner.width),
        theme.styles.menu,
    );
    let top = activity.list_start(ActivityPage::Regions, usize::from(body.height));
    for (index, item) in activity.region_tree().iter().enumerate().skip(top).take(usize::from(body.height)) {
        let style = if index == activity.region_cursor {
            theme.styles.cursor
        } else if matches!(item.row, RegionRow::Group(_)) {
            heading(theme)
        } else {
            theme.styles.overlay
        };
        let line = row(body, (index - top) as u16);
        buf.set_style(line, style);
        let indent = "  ".repeat(item.depth);
        let arrow = if item.expanded { "▾" } else { "▸" };
        let name = match item.row {
            RegionRow::Group(group) => {
                format!("{arrow} {} ({})", group.label(), if item.files > 0 { item.files } else { item.totals.regions })
            }
            RegionRow::File(first) => format!("{indent}{arrow} {}", activity.file_name(first)),
            RegionRow::Region(i) => {
                let region = &activity.regions()[i];
                let label = if activity.grouped() { region.kind() } else { region.name.as_str() };
                format!("{indent}{} {label}", region.permissions)
            }
        };
        text(buf, line, sized_line(&name, &sizes(&item.totals), line.width), style);
    }
}

/// Writes preview lines top to bottom and drops what does not fit.
pub(super) struct Page<'a> {
    pub(super) buf: &'a mut Buffer,
    pub(super) area: Rect,
    pub(super) y: u16,
}

impl Page<'_> {
    pub(super) fn room(&self) -> u16 {
        self.area.height.saturating_sub(self.y)
    }
    pub(super) fn line(&mut self, value: impl Into<String>, style: Style) -> Rect {
        let line = row(self.area, self.y);
        text(self.buf, line, value, style);
        self.y = self.y.saturating_add(1);
        line
    }
    pub(super) fn wrapped(&mut self, value: &str, style: Style) {
        for line in wrap(value, usize::from(self.area.width).max(1)) {
            self.line(line, style);
        }
    }
    pub(super) fn gap(&mut self) {
        self.y = self.y.saturating_add(1);
    }
}

pub(super) fn group_preview(activity: &Activity, group: MemoryGroup, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let members: Vec<_> = activity.group_members(group).take(LARGEST).collect();
    let totals = activity.group_totals(group);
    let whole = activity.region_totals().footprint;
    let mut page = Page { buf, area: body, y: 0 };
    page.line(group.label(), heading(theme));
    let count = format::count(totals.regions, Count::Region);
    page.line(
        if group.by_file() {
            format!("{} · {count}", format::count(activity.group_members(group).count(), Count::File))
        } else {
            count
        },
        dim(theme),
    );
    page.gap();
    metrics(&mut page, &totals, whole, look);
    page.gap();
    page.wrapped(group.about(), theme.styles.overlay);
    if group != MemoryGroup::Reserved && page.room() > 3 {
        page.gap();
        // Members rank by footprint, then by resident size, so a group that costs nothing
        // still shows what occupies the most RAM.
        let by_footprint = totals.footprint.unwrap_or(0) > 0;
        page.line(if by_footprint { tr!(memory_largest) } else { tr!(memory_largest_resident) }, heading(theme));
        let width = page.area.width;
        for (row, totals) in members.iter().take(LARGEST.min(usize::from(page.room().saturating_sub(5)).max(1))) {
            let name = match *row {
                RegionRow::File(first) => activity.file_name(first).to_owned(),
                RegionRow::Region(i) => activity.regions()[i].name.clone(),
                RegionRow::Group(_) => continue,
            };
            let value = if by_footprint { size(totals.footprint) } else { size(totals.resident) };
            page.line(sized_line(&format!("  {name}"), &[value], width), theme.styles.overlay);
        }
    }
    if page.room() > 4 {
        page.gap();
        for note in [tr!(memory_footprint_note), tr!(memory_resident_note), tr!(memory_mapped_note)] {
            page.wrapped(note, dim(theme));
        }
    }
}

/// Why a preview of the process's memory shows nothing: macOS keeps it from every program.
pub(super) fn protected(name: &str, protection: MemoryProtection, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let why = match protection {
        MemoryProtection::System => tr!(memory_protected_system, name = name),
        MemoryProtection::HardenedRuntime => tr!(memory_protected_runtime, name = name),
    };
    let mut page = Page { buf, area: body, y: 0 };
    page.wrapped(tr!(memory_protected_title), emphasis(theme));
    page.gap();
    page.wrapped(&why, theme.styles.overlay);
    page.gap();
    page.wrapped(tr!(memory_protected_still), dim(theme));
}

pub(super) fn snapshot(activity: &Activity, look: &Look<'_>, body: Rect, spinner: Option<Spinner>, buf: &mut Buffer) {
    let theme = look.theme;
    if activity.list == ActivityPage::Regions {
        if let Some(group) = activity.selected_group() {
            return group_preview(activity, group, look, body, buf);
        }
        if let Some(first) = activity.selected_file() {
            return file_preview(activity, first, look, body, buf);
        }
        if let Some(census) = activity.census.as_ref().filter(|_| activity.map) {
            return page_map(census, activity.census_cursor, activity.page == ActivityPage::Snapshot, look, body, buf);
        }
    }
    if let Some(finding) =
        activity.selected_finding().filter(|f| f.address.is_none() && activity.list == ActivityPage::Integrity)
    {
        let mut page = Page { buf, area: body, y: 0 };
        page.wrapped(tr!(integrity_whole_process), dim(theme));
        page.gap();
        page.wrapped(&finding.detail, emphasis(theme));
        page.gap();
        return page.wrapped(finding.kind.caveat(), theme.styles.overlay);
    }
    let Some(snapshot) = &activity.snapshot else {
        if activity.error.is_none() && activity.preview_waiting() {
            if let Some(spinner) = spinner {
                spinner.line(tr!(activity_loading), look, dim(theme)).render(row(body, 0), buf);
            }
            return;
        }
        if activity.error.is_some()
            && let Some((p, protection)) = activity.process().zip(activity.protection())
        {
            return protected(&p.name, protection, look, body, buf);
        }
        let note = if activity.error.is_some() {
            tr!(activity_preview_failed)
        } else if activity.list == ActivityPage::Regions && activity.selected_region().is_some_and(|r| !r.readable()) {
            tr!(activity_region_unreadable)
        } else if activity.list == ActivityPage::Search {
            tr!(activity_no_matches)
        } else if activity.list == ActivityPage::Integrity {
            tr!(integrity_none)
        } else {
            tr!(activity_region_choose)
        };
        text(buf, row(body, 0), note, dim(theme));
        return;
    };
    let columns = activity.snapshot_columns(usize::from(body.width));
    let details = tr!(
        activity_capture_label,
        pid = snapshot.process.pid,
        address = snapshot.address,
        bytes = snapshot.bytes.len(),
        mode = if activity.text { tr!(activity_text) } else { tr!(activity_hex) }
    );
    text(buf, row(body, 0), details, emphasis(theme));
    let old = activity.previous.as_ref();
    if activity.diff {
        text(
            buf,
            row(body, 1),
            if old.is_some() { tr!(activity_compare) } else { tr!(activity_no_previous) },
            dim(theme),
        );
    } else if let Some(region) = activity.shown_region().filter(|r| r.mapped() > snapshot.bytes.len() as u64) {
        let total = size(Some(region.mapped()));
        let note = match snapshot.address.saturating_sub(region.start) {
            0 => tr!(activity_window_start, size = total),
            offset => tr!(activity_window, offset = format!("0x{offset:x}"), size = total),
        };
        text(buf, row(body, 1), note, dim(theme));
    }
    let hit = activity.marked();
    let printable = |b: u8| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '.' };
    let rows = activity.snapshot_rows(columns);
    for (y, shown) in
        rows.into_iter().skip(activity.offset).take(usize::from(body.height.saturating_sub(2))).enumerate()
    {
        let address = Span::styled(format!("{:016x}  ", snapshot.address + shown.start() as u64), dim(theme));
        let line = match shown {
            SnapshotRow::Zeros { len, .. } => Line::from(vec![
                address,
                Span::styled(tr!(activity_zero_run, size = size(Some(len as u64))), dim(theme)),
            ]),
            SnapshotRow::Bytes(offset) => {
                let chunk = &snapshot.bytes[offset..(offset + columns).min(snapshot.bytes.len())];
                let mut spans = vec![address];
                for (i, &byte) in chunk.iter().enumerate() {
                    let changed = activity.diff && old.is_some_and(|old| old.bytes.get(offset + i) != Some(&byte));
                    let at = snapshot.address.saturating_add((offset + i) as u64);
                    let style = if hit.as_ref().is_some_and(|hit| hit.contains(&at)) {
                        theme.styles.cursor.add_modifier(Modifier::BOLD)
                    } else if changed {
                        theme.styles.marked.add_modifier(Modifier::BOLD)
                    } else {
                        theme.styles.overlay
                    };
                    spans.push(Span::styled(
                        if activity.text { printable(byte).to_string() } else { format!("{byte:02x} ") },
                        style,
                    ));
                }
                if !activity.text {
                    let ascii: String = chunk.iter().map(|&b| printable(b)).collect();
                    spans.push(Span::styled(format!(" {ascii:columns$}"), theme.styles.overlay));
                }
                Line::from(spans)
            }
        };
        line.render(row(body, (y + 2) as u16), buf);
    }
}

/// How the page map draws a class: a glyph that tells it apart without colour, and a colour.
pub(super) fn page_look(class: PageClass, theme: &Theme) -> (char, Style) {
    let (glyph, color) = match class {
        PageClass::Pointers => ('▓', Some(Color::Cyan)),
        PageClass::Text => ('▒', Some(Color::Green)),
        PageClass::Wide => ('≈', Some(Color::Green)),
        PageClass::Code => ('█', Some(Color::Magenta)),
        PageClass::Dense => ('#', Some(Color::Red)),
        PageClass::Data => ('░', Some(Color::Blue)),
        PageClass::Filler => ('+', None),
        PageClass::Zero => ('·', None),
        PageClass::Absent => ('_', None),
        PageClass::Unread => ('×', None),
    };
    (glyph, readable(theme, color).map_or_else(|| dim(theme), |color| theme.styles.overlay.fg(color)))
}

/// The class most of `pages` have, earlier classes winning ties.
pub(super) fn majority(pages: &[PageClass]) -> PageClass {
    let count = |class: &PageClass| pages.iter().filter(|page| *page == class).count();
    PageClass::ALL.into_iter().rev().max_by_key(count).unwrap_or(PageClass::Unread)
}

/// The selected region's pages as a grid, a cell per page or per run of them, with a legend of
/// what share of the region each class takes and, when focused, a cursor to open a page with.
pub(super) fn page_map(
    census: &MemoryCensus,
    cursor: usize,
    focused: bool,
    look: &Look<'_>,
    body: Rect,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let total = census.end.saturating_sub(census.start);
    let title = if census.stride > 1 {
        tr!(
            census_sampled,
            pages = census.pages.len(),
            total = total.div_ceil(census.page.max(1)),
            size = size(Some(total))
        )
    } else {
        tr!(census_title, pages = census.pages.len(), size = size(Some(total)))
    };
    text(buf, row(body, 0), title, emphasis(theme));
    let height = usize::from(body.height.saturating_sub(2)).saturating_sub(CENSUS_LEGEND);
    let (columns, rows, span) = census_grid(census.pages.len(), usize::from(body.width), height);
    let cell = |index: usize| majority(&census.pages[index * span..((index + 1) * span).min(census.pages.len())]);
    let at = (cursor / span).min(census.pages.len().saturating_sub(1) / span);
    if focused && !census.pages.is_empty() {
        let line = tr!(census_cursor, address = census.address(at * span), class = cell(at).label());
        text(buf, row(body, 1), line, theme.styles.overlay);
    }
    let mut legend = vec![Vec::new()];
    let mut x = 0;
    for class in PageClass::ALL {
        let share = census.share(class);
        if share == 0.0 {
            continue;
        }
        let (glyph, style) = page_look(class, theme);
        let label = format!(" {} {:.0}%  ", class.label(), (share * 100.0).max(1.0));
        let width = 1 + label.width();
        if x + width > usize::from(body.width) && x > 0 && legend.len() < CENSUS_LEGEND {
            legend.push(Vec::new());
            x = 0;
        }
        x += width;
        if let Some(line) = legend.last_mut() {
            line.extend([Span::styled(glyph.to_string(), style), Span::styled(label, dim(theme))]);
        }
    }
    for (y, spans) in legend.into_iter().enumerate() {
        Line::from(spans).render(row(body, 2 + y as u16), buf);
    }
    for y in 0..rows.min(height) {
        let line = row(body, (2 + CENSUS_LEGEND + y) as u16);
        for x in 0..columns {
            let index = y * columns + x;
            if index * span >= census.pages.len() {
                break;
            }
            let (glyph, style) = page_look(cell(index), theme);
            let style = if focused && index == at { theme.styles.cursor } else { style };
            if let Some(target) = buf.cell_mut(Position { x: line.x + x as u16, y: line.y }) {
                target.set_char(glyph).set_style(style);
            }
        }
    }
}
