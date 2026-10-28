use crate::{format, icons, look::Look, panel_view::PanelScene};
use borg_core::{Action, Listing, Panel, ResultSource, Row, SortKey};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

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

pub(crate) fn columns(area: Rect, totals: bool) -> Vec<(SortKey, Rect)> {
    let size = SIZE_WIDTH.min(area.width / 3);
    let date = if area.width >= DATE_FROM { DATE_WIDTH } else { 0 };
    let source = if area.width >= SOURCE_FROM { area.width / SOURCE_DIVISOR } else { 0 };
    let files = if totals && area.width >= FILES_FROM { if area.width >= DATE_FROM { 9 } else { 7 } } else { 0 };
    let name = area.width.saturating_sub(size + date + source + files + MARK_WIDTH);
    let mut x = area.x.saturating_add(MARK_WIDTH.min(area.width));
    [
        (SortKey::Name, name),
        (SortKey::Source, source),
        (SortKey::Size, size),
        (SortKey::Files, files),
        (SortKey::Modified, date),
    ]
    .into_iter()
    .filter_map(|(key, width)| {
        let rect = Rect { x, width, ..area };
        x = x.saturating_add(width);
        (width > 0).then_some((key, rect))
    })
    .collect()
}

pub(crate) fn has_totals(panel: &Panel) -> bool {
    panel.is_tree()
        || panel.entries().iter().any(|entry| entry.usage.is_some())
        || matches!(panel.results.as_ref().map(|r| &r.source), Some(ResultSource::Search { request, .. }) if request.options.kind == borg_core::SearchKind::FolderTotals)
}

pub(crate) fn panel_columns(panel: &Panel, area: Rect) -> Vec<(SortKey, Rect)> {
    if !panel.is_tree() {
        return columns(area, has_totals(panel));
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
    let border = if scene.active { styles.border_active } else { styles.border };
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
    let footer = tr!(
        items_marked,
        panel.entries().len(),
        panel.marked_count(),
        filter = filter,
        hidden = hidden,
        total_bytes = total_bytes,
        visible = visible
    );
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
        .title_bottom(footer);
    let inner = block.inner(scene.area);
    block.render(scene.area, buf);
    for (rect, label, _) in controls {
        text(buf, rect, label, styles.menu);
    }
    if inner.is_empty() {
        return;
    }
    let context = match &results.source {
        ResultSource::Collection(_) => tr!(tab_switch_ins_ctrl_t_mark_ctrl).into(),
        ResultSource::Search { request, progress } => {
            if panel.is_size_view() && progress.cached {
                let cached = tr!(sizes_cached_at, time = format::modified(progress.measured, scene.clock));
                if progress.limited { tr!(sizes_limited, text = cached) } else { cached }
            } else {
                let state = if progress.cancelled {
                    tr!(cancelled)
                } else if progress.limited {
                    tr!(result_limit_reached)
                } else if panel.listing() == &Listing::Loading {
                    tr!(searching_ctrl_c_stop)
                } else {
                    tr!(finished)
                };
                let summary = tr!(
                    scanned_skipped,
                    progress.visited,
                    format::count(progress.errors, format::Count::Error),
                    progress.skipped,
                    state = state
                );
                let room = usize::from(inner.width).saturating_sub(summary.chars().count() + 3);
                format!("{summary} · {}", format::truncate_start(&request.root.display().to_string(), room))
            }
        }
    };
    let context =
        if let Listing::Failed(error) = panel.listing() { tr!(search_failed, error = error) } else { context };
    text(buf, Rect { height: 1, ..inner }, &context, styles.footer);
    let (header, body) = geometry(scene.area);
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
    for (offset, index) in (panel.scroll()..panel.len()).take(usize::from(body.height)).enumerate() {
        let area = Rect { y: body.y + offset as u16, height: 1, ..body };
        let focused = index == panel.cursor();
        match panel.row(index) {
            Some(Row::Group { count, .. }) if panel.is_tree() => {
                let style = if focused { styles.cursor } else { styles.footer };
                buf.set_style(area, style);
                let parent = (0..index).rev().find_map(|i| match panel.row(i) {
                    Some(Row::Entry(entry)) => Some(entry),
                    _ => None,
                });
                for (key, cell) in panel_columns(panel, area) {
                    let value = match key {
                        SortKey::Name => format!(
                            "{}· {}",
                            " ".repeat(parent.map_or(2, |entry| tree_indent(panel, entry, cell.width) + 2)),
                            tr!(tree_direct_files_short)
                        ),
                        SortKey::Size => format!(
                            "{:>width$}",
                            parent
                                .and_then(|entry| entry.usage.as_ref())
                                .map_or_else(String::new, |usage| format::size(usage.direct_bytes)),
                            width = usize::from(cell.width.saturating_sub(1))
                        ),
                        SortKey::Files => count.to_string(),
                        _ => String::new(),
                    };
                    text(buf, cell, &value, style);
                }
            }
            Some(Row::Group { label, count, collapsed }) => {
                let style = if focused { styles.cursor } else { styles.title_active };
                buf.set_style(area, style);
                text(buf, area, &format!("{} {label} · {count}", if collapsed { "▸" } else { "▾" }), style);
            }
            Some(Row::Entry(entry)) => {
                let marked = panel.is_marked(entry);
                let style = if focused && scene.active {
                    if marked { styles.marked_cursor } else { styles.cursor }
                } else if marked {
                    styles.marked
                } else if entry.missing {
                    styles.notice
                } else if entry.is_dir() {
                    styles.dir
                } else {
                    styles.file
                };
                buf.set_style(area, style);
                text(buf, Rect { width: MARK_WIDTH.min(area.width), ..area }, if marked { "*" } else { " " }, style);
                for (key, cell) in panel_columns(panel, area) {
                    let value = match key {
                        SortKey::Name => {
                            let icon =
                                if scene.icons { icons::icon(entry).glyph } else { icons::portable(entry).glyph };
                            let name = format!(
                                "{icon} {}{}",
                                entry.name,
                                if entry.missing { tr!(missing_suffix) } else { "" }
                            );
                            if let Some(tree) = results.tree.as_ref() {
                                let arrow = if tree.branches.contains(entry.identity()) {
                                    if tree.open.contains(entry.identity())
                                        || (index + 1..panel.len())
                                            .find_map(|i| match panel.row(i) {
                                                Some(Row::Entry(child)) => {
                                                    Some(child.identity().parent() == Some(entry.identity()))
                                                }
                                                _ => None,
                                            })
                                            .unwrap_or(false)
                                    {
                                        "▾"
                                    } else {
                                        "▸"
                                    }
                                } else {
                                    " "
                                };
                                format!("{}{arrow} {name}", " ".repeat(tree_indent(panel, entry, cell.width)))
                            } else {
                                name
                            }
                        }
                        SortKey::Source => {
                            let source =
                                entry.identity().parent().map_or_else(String::new, |p| p.display().to_string());
                            format::truncate_start(&source, usize::from(cell.width.saturating_sub(1)))
                        }
                        SortKey::Size if entry.missing => "—".into(),
                        SortKey::Files => entry
                            .usage
                            .as_ref()
                            .map_or_else(|| "—".into(), |u| format!("{}{}", if u.partial { "≥" } else { "" }, u.files)),
                        SortKey::Size if entry.usage.is_some() => format!(
                            "{}{}{}",
                            if entry.usage.as_ref().is_some_and(|u| u.cached) { "~" } else { "" },
                            if entry.usage.as_ref().is_some_and(|u| u.partial) { "≥" } else { "" },
                            format::size(entry.size)
                        ),
                        SortKey::Size if panel.is_tree() => "—".into(),
                        SortKey::Size if entry.is_dir() => tr!(dir).into(),
                        SortKey::Size => format::size(entry.size),
                        SortKey::Modified => format::modified(entry.modified, scene.clock),
                        SortKey::Extension => String::new(),
                    };
                    let value = if key == SortKey::Size {
                        format::pad_start(&value, usize::from(cell.width.saturating_sub(1)))
                    } else {
                        value
                    };
                    let style = if key == SortKey::Name || focused {
                        style
                    } else {
                        styles.meta.patch(style.remove_modifier(Modifier::BOLD))
                    };
                    text(buf, cell, &value, style);
                }
            }
            _ => {}
        }
    }
    if panel.is_empty() && body.height > 0 {
        let line = Rect { height: 1, ..body };
        if panel.listing() != &Listing::Loading {
            text(buf, line, tr!(no_matches_ctrl_f_search_alt_v), styles.footer);
        } else if let Some(spinner) = scene.spinner {
            spinner.line(tr!(results_scanning), look, styles.footer).render(line, buf);
        }
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
            let path = format::truncate_start(
                &entry.identity().display().to_string(),
                width.saturating_sub(suffix.chars().count()),
            );
            format!("{path}{suffix}")
        }
        Some(Row::Group { label, .. }) if panel.is_tree() => label.to_owned(),
        Some(Row::Group { label, count, .. }) => tr!(items_enter_folds_group, count = count, label = label),
        _ => String::new(),
    }
}
