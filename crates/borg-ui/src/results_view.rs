//! A panel of results: a search's finds, the sizes of a folder's contents or a collection,
//! with their columns, the controls on the frame and the line in the status bar.

use borg_core::{Action, Listing, Panel, ResultSource, Results, Row, SearchProgress, SortKey};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{format, look::Look, panel_view::PanelScene, theme::Styles};

mod rows;

const HEADER_ROWS: u16 = 2;
const MARK_WIDTH: u16 = 2;
const SIZE_WIDTH: u16 = 9;
const DATE_WIDTH: u16 = 12;
const DATE_FROM: u16 = 80;
const SOURCE_FROM: u16 = 40;
const FILES_FROM: u16 = 44;
const CONTROL_GAP: u16 = 1;
const FRAME_WIDTH: u16 = 2;
const SOURCE_DIVISOR: u16 = 3;
/// A contents search's share of the row for the first match, and for the source folder beside it
/// when the panel is wide enough for both.
const MATCH_PERCENT: u16 = 40;
const SOURCE_BESIDE_MATCH_PERCENT: u16 = 20;
const PERCENT: u32 = 100;

pub(crate) fn controls(area: Rect, expanded: bool) -> Vec<(Rect, &'static str, Action)> {
    if area.is_empty() {
        return Vec::new();
    }
    let labels = [if expanded { tr!(panel_restore) } else { tr!(panel_maximize) }, tr!(panel_fold)];
    let width = labels.iter().map(|label| label.width() as u16).sum::<u16>() + CONTROL_GAP;
    if width.saturating_add(FRAME_WIDTH) > area.width {
        return Vec::new();
    }
    let mut x = area.right().saturating_sub(width + 1);
    labels
        .into_iter()
        .zip([Action::SetExpanded(!expanded), Action::BackResults])
        .map(|(label, action)| {
            let rect = Rect { x, y: area.y, width: label.width() as u16, height: 1 };
            x += rect.width + CONTROL_GAP;
            (rect, label, action)
        })
        .collect()
}

pub(crate) fn geometry(area: Rect) -> (Rect, Rect) {
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    let header = Rect { y: inner.y.saturating_add(1), height: u16::from(inner.height > 1), ..inner };
    let body = Rect {
        y: inner.y.saturating_add(HEADER_ROWS.min(inner.height)),
        height: inner.height.saturating_sub(HEADER_ROWS),
        ..inner
    };
    (header, body)
}

/// The columns a row of results sorts by, and where a contents search shows each file's first
/// match: after the name, taking the source folder's room when the panel is narrow.
fn layout(area: Rect, totals: bool, lines: bool) -> (Vec<(SortKey, Rect)>, Option<Rect>) {
    let share = |percent: u16| (u32::from(area.width) * u32::from(percent) / PERCENT) as u16;
    let size = SIZE_WIDTH.min(area.width / 3);
    let date = if area.width >= DATE_FROM { DATE_WIDTH } else { 0 };
    let found = if lines && area.width >= SOURCE_FROM { share(MATCH_PERCENT) } else { 0 };
    let source = match (lines, area.width) {
        (false, width) if width >= SOURCE_FROM => width / SOURCE_DIVISOR,
        (true, width) if width >= DATE_FROM => share(SOURCE_BESIDE_MATCH_PERCENT),
        _ => 0,
    };
    let files = if totals && area.width >= FILES_FROM { if area.width >= DATE_FROM { 9 } else { 7 } } else { 0 };
    let name = area.width.saturating_sub(size + date + found + source + files + MARK_WIDTH);
    let mut x = area.x.saturating_add(MARK_WIDTH.min(area.width));
    let (mut columns, mut line) = (Vec::new(), None);
    for (key, width) in [
        (Some(SortKey::Name), name),
        (None, found),
        (Some(SortKey::Source), source),
        (Some(SortKey::Size), size),
        (Some(SortKey::Files), files),
        (Some(SortKey::Modified), date),
    ] {
        let rect = Rect { x, width, ..area };
        x = x.saturating_add(width);
        match key {
            _ if width == 0 => {}
            Some(key) => columns.push((key, rect)),
            None => line = Some(rect),
        }
    }
    (columns, line)
}

pub(crate) fn columns(area: Rect, totals: bool, lines: bool) -> Vec<(SortKey, Rect)> {
    layout(area, totals, lines).0
}

/// Where a contents search shows each file's first match, when the panel has room for it.
pub(crate) fn match_column(panel: &Panel, area: Rect) -> Option<Rect> {
    let results = panel.results.as_ref()?;
    (!panel.is_tree() && results.contents().is_some()).then(|| layout(area, has_totals(panel), true).1).flatten()
}

pub(crate) fn has_totals(panel: &Panel) -> bool {
    panel.is_tree()
        || panel.entries().iter().any(|entry| entry.usage.is_some())
        || matches!(panel.results.as_ref().map(|r| &r.source), Some(ResultSource::Search { request, .. }) if request.options.kind == borg_core::SearchKind::FolderTotals)
}

pub(crate) fn panel_columns(panel: &Panel, area: Rect) -> Vec<(SortKey, Rect)> {
    if !panel.is_tree() {
        let lines = panel.results.as_ref().is_some_and(|results| results.contents().is_some());
        return columns(area, has_totals(panel), lines);
    }
    let size = SIZE_WIDTH.min(area.width / 3);
    let files = if area.width >= 34 { 8 } else { 0 };
    let name = area.width.saturating_sub(MARK_WIDTH + size + files);
    let mut x = area.x.saturating_add(MARK_WIDTH.min(area.width));
    [(SortKey::Name, name), (SortKey::Size, size), (SortKey::Files, files)]
        .into_iter()
        .filter_map(|(key, width)| {
            let cell = Rect { x, width, ..area };
            x += width;
            (width > 0).then_some((key, cell))
        })
        .collect()
}

pub(crate) fn tree_indent(panel: &Panel, entry: &borg_vfs::Entry, width: u16) -> usize {
    panel
        .results
        .as_ref()
        .and_then(|r| r.tree.as_ref())
        .and_then(|tree| entry.identity().strip_prefix(&tree.root).ok())
        .map_or(0, |relative| relative.components().count().saturating_mul(2))
        .min(usize::from(width.saturating_sub(6)))
}

fn text(buf: &mut Buffer, area: Rect, value: &str, style: Style) {
    if !area.is_empty() {
        Line::styled(value.to_owned(), style).render(area, buf);
    }
}

pub(crate) fn draw(scene: &PanelScene<'_>, look: &Look<'_>, buf: &mut Buffer) {
    if scene.area.is_empty() {
        return;
    }
    let panel = scene.panel;
    let Some(results) = &panel.results else { return };
    let styles = look.styles();
    let inner = frame(scene, look, results, buf);
    if inner.is_empty() {
        return;
    }
    let context = match panel.listing() {
        Listing::Failed(error) => tr!(search_failed, error = error),
        _ => context(scene, &results.source, inner.width),
    };
    text(buf, Rect { height: 1, ..inner }, &context, styles.footer);
    let (header, body) = geometry(scene.area);
    draw_header(panel, header, styles, buf);
    rows::draw(scene, styles, results, body, buf);
    if panel.is_empty() && body.height > 0 {
        let line = Rect { height: 1, ..body };
        if panel.listing() != &Listing::Loading {
            text(buf, line, tr!(no_matches_ctrl_f_search_alt_v), styles.footer);
        } else if let Some(spinner) = scene.spinner {
            spinner.line(tr!(results_scanning), look, styles.footer).render(line, buf);
        }
    }
}

/// The panel's frame: the results' title up to the controls, and at the bottom what is listed,
/// shown and marked. Returns the room inside it.
fn frame(scene: &PanelScene<'_>, look: &Look<'_>, results: &Results, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let border = if scene.active { styles.border_active } else { styles.border };
    let controls = controls(scene.area, scene.expanded);
    let title_end = controls
        .first()
        .map_or(scene.area.right().saturating_sub(1), |(rect, _, _)| rect.x.saturating_sub(CONTROL_GAP));
    let title_width = title_end.saturating_sub(scene.area.x.saturating_add(1));
    let title = format::truncate_end(&format!(" {} ", results.title()), usize::from(title_width));
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.panel)
        .border_style(border)
        .title(title)
        .title_bottom(footer(scene.panel, results));
    let inner = block.inner(scene.area);
    block.render(scene.area, buf);
    for (rect, label, _) in controls {
        text(buf, rect, label, styles.menu);
    }
    inner
}

/// How many results there are, how many the filter shows and how many are marked, with the
/// marks the filter hides and the size of what is selected.
fn footer(panel: &Panel, results: &Results) -> String {
    let visible_paths: std::collections::HashSet<_> = panel.visible_entries().map(|e| e.identity()).collect();
    let visible = visible_paths.len();
    let hidden_marks = panel.marked().filter(|e| !visible_paths.contains(e.identity())).count();
    let hidden = if hidden_marks > 0 { tr!(hidden_marks, hidden_marks = hidden_marks) } else { String::new() };
    let selected: std::collections::HashSet<_> = if panel.marked_count() > 0 {
        borg_core::selected_paths(panel).into_iter().collect()
    } else {
        Default::default()
    };
    let marked_bytes = panel
        .marked()
        .filter(|entry| selected.contains(entry.identity()) && (!entry.is_dir() || entry.usage.is_some()))
        .fold(0u64, |total, entry| total.saturating_add(entry.size));
    let total_bytes = if marked_bytes > 0 { format!(" · {}", format::size(marked_bytes)) } else { String::new() };
    let filter = if results.filter.is_empty() { String::new() } else { tr!(filter, results.filter) };
    tr!(
        items_marked,
        panel.entries().len(),
        panel.marked_count(),
        filter = filter,
        hidden = hidden,
        total_bytes = total_bytes,
        visible = visible
    )
}

/// The line above the columns: a collection's keys, or how a search went and in which folder.
fn context(scene: &PanelScene<'_>, source: &ResultSource, width: u16) -> String {
    let panel = scene.panel;
    match source {
        ResultSource::Collection(_) => tr!(tab_switch_ins_ctrl_t_mark_ctrl).into(),
        ResultSource::Search { progress, .. } if panel.is_size_view() && progress.cached => {
            let cached = tr!(sizes_cached_at, time = format::modified(progress.measured, scene.clock));
            if progress.limited { tr!(sizes_limited, text = cached) } else { cached }
        }
        ResultSource::Search { request, progress } => {
            let summary = search_summary(panel, progress, request.options.contents.is_some(), scene.clock);
            let room = usize::from(width).saturating_sub(summary.chars().count() + 3);
            format!("{summary} · {}", format::truncate_start(&request.root.display().to_string(), room))
        }
    }
}

/// How a search went: whether it still runs, what it read or the index it used, and how many
/// folders it left out. A contents search counts the files it read instead of the names.
fn search_summary(panel: &Panel, progress: &SearchProgress, contents: bool, clock: &jiff::Zoned) -> String {
    let state = if progress.cancelled {
        tr!(cancelled)
    } else if progress.limited {
        tr!(result_limit_reached)
    } else if panel.listing() == &Listing::Loading {
        tr!(searching_ctrl_c_stop)
    } else {
        tr!(finished)
    };
    let errors = format::count(progress.errors, format::Count::Error);
    let summary = match progress.indexed {
        Some(built) => format!("{state} · {}", tr!(indexed_at, time = format::modified(Some(built), clock))),
        None if contents => tr!(
            contents_progress,
            state = state,
            files = format::count(progress.read, format::Count::File),
            unread = progress.unread,
            errors = errors
        ),
        None => tr!(scanned_skipped, progress.visited, errors, progress.skipped, state = state),
    };
    if progress.excluded > 0 {
        let folders = format::count(progress.excluded, format::Count::Folder);
        format!("{summary} · {}", tr!(folders_left_out, folders = folders))
    } else {
        summary
    }
}

/// The columns' titles, the one the results are sorted by with an arrow for the order.
fn draw_header(panel: &Panel, header: Rect, styles: &Styles<Style>, buf: &mut Buffer) {
    for (key, cell) in panel_columns(panel, header) {
        let label = match key {
            SortKey::Files => tr!(results_view_files),
            SortKey::Name => tr!(name),
            SortKey::Source => tr!(source_folder),
            SortKey::Size => tr!(size),
            SortKey::Modified => tr!(modified),
            SortKey::Extension => tr!(type),
        };
        let arrow = if panel.sort().key == key { if panel.sort().reverse { " ↓" } else { " ↑" } } else { "" };
        text(buf, cell, &format!("{label}{arrow}"), styles.menu);
    }
    if let Some(cell) = match_column(panel, header) {
        text(buf, cell, tr!(results_view_match), styles.menu);
    }
}

pub(crate) fn status(panel: &Panel, width: usize, clock: &jiff::Zoned) -> String {
    match panel.hovered() {
        Some(Row::Entry(entry)) => {
            let totals = entry.usage.as_ref().map_or_else(String::new, |u| {
                tr!(
                    files_folders_content_bytes,
                    u.files,
                    u.folders,
                    if u.partial { tr!(at_least) } else { "" },
                    entry.size
                )
            });
            let observation = entry.usage.as_ref().filter(|u| u.measured.is_some()).map_or_else(String::new, |u| {
                let time = format::modified(u.measured, clock);
                if u.cached { tr!(sizes_cached_status, time = time) } else { tr!(sizes_measured_status, time = time) }
            });
            let suffix = format!("{}{totals}{observation}", if entry.missing { tr!(missing_original) } else { "" });
            let line = panel.results.as_ref().and_then(|results| results.line(entry.identity()));
            let place = match line {
                Some(line) => format!("{}:{}", entry.identity().display(), line.line),
                None => entry.identity().display().to_string(),
            };
            let path = format::truncate_start(&place, width.saturating_sub(suffix.chars().count()));
            format!("{path}{suffix}")
        }
        Some(Row::Group { label, .. }) if panel.is_tree() => label.to_owned(),
        Some(Row::Group { label, count, .. }) => tr!(items_enter_folds_group, count = count, label = label),
        _ => String::new(),
    }
}
