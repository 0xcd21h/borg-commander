//! The rows of a panel of results: entries with their columns, groups, and in a tree the row
//! for the files directly in a folder.

use borg_core::{LineMatch, Panel, Results, Row, SortKey};
use borg_i18n::tr;
use borg_vfs::Entry;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use super::{MARK_WIDTH, match_column, panel_columns, text, tree_indent};
use crate::{format, icons, panel_view::PanelScene, theme::Styles};

/// Where a row draws: its place among the panel's rows, its line, and whether the cursor is on it.
#[derive(Clone, Copy)]
struct Place {
    index: usize,
    area: Rect,
    focused: bool,
}

pub(super) fn draw(scene: &PanelScene<'_>, styles: &Styles<Style>, results: &Results, body: Rect, buf: &mut Buffer) {
    let panel = scene.panel;
    for (offset, index) in (panel.scroll()..panel.len()).take(usize::from(body.height)).enumerate() {
        let area = Rect { y: body.y + offset as u16, height: 1, ..body };
        let place = Place { index, area, focused: index == panel.cursor() };
        match panel.row(index) {
            Some(Row::Group { count, .. }) if panel.is_tree() => direct_files(panel, place, count, styles, buf),
            Some(Row::Group { label, count, collapsed }) => {
                let style = if place.focused { styles.cursor } else { styles.title_active };
                buf.set_style(area, style);
                text(buf, area, &format!("{} {label} · {count}", if collapsed { "▸" } else { "▾" }), style);
            }
            Some(Row::Entry(entry)) => draw_entry(scene, styles, results, entry, place, buf),
            _ => {}
        }
    }
}

/// In a tree, the row under a folder for the files directly in it: their size and count.
fn direct_files(panel: &Panel, place: Place, count: usize, styles: &Styles<Style>, buf: &mut Buffer) {
    let style = if place.focused { styles.cursor } else { styles.footer };
    buf.set_style(place.area, style);
    let parent = (0..place.index).rev().find_map(|i| match panel.row(i) {
        Some(Row::Entry(entry)) => Some(entry),
        _ => None,
    });
    for (key, cell) in panel_columns(panel, place.area) {
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

/// An entry: its mark and its columns, the name in the entry's style and the rest dimmed unless
/// the cursor is on it.
fn draw_entry(
    scene: &PanelScene<'_>,
    styles: &Styles<Style>,
    results: &Results,
    entry: &Entry,
    place: Place,
    buf: &mut Buffer,
) {
    let panel = scene.panel;
    let marked = panel.is_marked(entry);
    let style = entry_style(styles, entry, marked, place.focused && scene.active);
    buf.set_style(place.area, style);
    text(buf, Rect { width: MARK_WIDTH.min(place.area.width), ..place.area }, if marked { "*" } else { " " }, style);
    for (key, cell) in panel_columns(panel, place.area) {
        let value = cell_value(scene, results, entry, place.index, key, cell.width);
        let value = if key == SortKey::Size {
            format::pad_start(&value, usize::from(cell.width.saturating_sub(1)))
        } else {
            value
        };
        let style = if key == SortKey::Name || place.focused {
            style
        } else {
            styles.meta.patch(style.remove_modifier(Modifier::BOLD))
        };
        text(buf, cell, &value, style);
    }
    if let Some(cell) = match_column(panel, place.area)
        && let Some(line) = results.line(entry.identity())
    {
        let (found, current) = crate::find_view::match_styles(styles);
        let (style, found) = if place.focused {
            (style, current)
        } else {
            (styles.meta.patch(style.remove_modifier(Modifier::BOLD)), found)
        };
        let width = cell.width.saturating_sub(1);
        first_match(line, usize::from(width), style, found).render(Rect { width, ..cell }, buf);
    }
}

/// A contents search's first match in a file: its line number, then the line with the match
/// marked, cut at its start when the match would not show otherwise.
fn first_match(line: &LineMatch, width: usize, style: Style, found: Style) -> Line<'static> {
    let number = format!("{}: ", line.line);
    let room = width.saturating_sub(number.width());
    let matched = format::truncate_end(&line.found, room);
    let left = room.saturating_sub(matched.width());
    let before = line.before.trim_start();
    let before = match before.width() {
        fits if fits <= left => before.to_owned(),
        _ if left == 0 => String::new(),
        _ => format::truncate_start(before, left),
    };
    let after = left.saturating_sub(before.width());
    let after = if after == 0 { String::new() } else { format::truncate_end(&line.after, after) };
    Line::from(vec![
        Span::styled(number, style),
        Span::styled(before, style),
        Span::styled(matched, found),
        Span::styled(after, style),
    ])
}

fn entry_style(styles: &Styles<Style>, entry: &Entry, marked: bool, cursor: bool) -> Style {
    if cursor {
        if marked { styles.marked_cursor } else { styles.cursor }
    } else if marked {
        styles.marked
    } else if entry.missing {
        styles.notice
    } else if entry.is_dir() {
        styles.dir
    } else {
        styles.file
    }
}

/// What an entry shows in a column. Sizes still being measured, or measured before, are marked
/// as at least or as remembered.
fn cell_value(
    scene: &PanelScene<'_>,
    results: &Results,
    entry: &Entry,
    index: usize,
    key: SortKey,
    width: u16,
) -> String {
    let panel = scene.panel;
    match key {
        SortKey::Name => name(scene, results, entry, index, width),
        SortKey::Source => {
            let source = entry.identity().parent().map_or_else(String::new, |p| p.display().to_string());
            format::truncate_start(&source, usize::from(width.saturating_sub(1)))
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
    }
}

/// An entry's icon and name; in a tree, indented under its folder after an arrow that says
/// whether a folder is open.
fn name(scene: &PanelScene<'_>, results: &Results, entry: &Entry, index: usize, width: u16) -> String {
    let icon = if scene.icons { icons::icon(entry).glyph } else { icons::portable(entry).glyph };
    let name = format!("{icon} {}{}", entry.name, if entry.missing { tr!(missing_suffix) } else { "" });
    let Some(tree) = results.tree.as_ref() else { return name };
    let arrow = if !tree.branches.contains(entry.identity()) {
        " "
    } else if tree.open.contains(entry.identity()) || shows_children(scene.panel, entry, index) {
        "▾"
    } else {
        "▸"
    };
    format!("{}{arrow} {name}", " ".repeat(tree_indent(scene.panel, entry, width)))
}

/// Whether the next entry after the one at `index` is inside it.
fn shows_children(panel: &Panel, entry: &Entry, index: usize) -> bool {
    (index + 1..panel.len())
        .find_map(|i| match panel.row(i) {
            Some(Row::Entry(child)) => Some(child.identity().parent() == Some(entry.identity())),
            _ => None,
        })
        .unwrap_or(false)
}
