//! The Time Machine window: the selection, the cache and build folders under the panel's
//! folder, and every exclusion there is, each with how backups treat it.

use std::{path::Path, time::Instant};

use borg_core::{AppState, BackupItem, BackupTab, BackupWindow, Exclusion, Task};
use borg_i18n::{Count, count, tr};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    activity_view::{dim, emphasis, heading, row, text},
    dialog_view::centered,
    format,
    look::Look,
    spinner::Spinner,
};

const MAX_WIDTH: u16 = 104;
/// The tabs, the summary and a blank line.
const HEADER_ROWS: u16 = 3;
/// A blank line, the hovered item's two lines of explanation, and the outcome.
const FOOTER_ROWS: u16 = 4;
const MARK: &str = "✓";
const MARK_WIDTH: u16 = 2;
const SIZE_WIDTH: u16 = 8;
const MIN_STATUS_WIDTH: u16 = 12;
const GAP: u16 = 2;

enum Row<'a> {
    Group(&'a Exclusion),
    Item(usize, &'a BackupItem),
}

/// What a click in the window lands on: a tab, by the digit that shows it, or an item.
pub(crate) enum BackupSpot {
    Tab(char),
    Item(usize),
}

/// The window on `screen`, and the area inside its border and margins.
fn frame(screen: Rect) -> (Rect, Rect) {
    let width = screen.width.saturating_sub(4).min(MAX_WIDTH);
    let rect = centered(screen, width, borg_core::backup_window_height(screen.height));
    let body = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    (rect, body)
}

/// The rows of the list, under the tabs and the summary.
fn list_area(body: Rect, screen: Rect) -> Rect {
    let room = body.height.saturating_sub(HEADER_ROWS + FOOTER_ROWS);
    let rows = u16::try_from(borg_core::backup_list_rows(screen.height)).unwrap_or(room).min(room);
    Rect { y: body.y + HEADER_ROWS, height: rows, ..body }
}

/// The keys the border lists, as many as fit `rect`.
pub(crate) fn keys(window: &BackupWindow, rect: Rect) -> &'static str {
    let keys = match window.tab {
        BackupTab::All => tr!(backup_keys_all),
        _ => tr!(backup_keys),
    };
    if keys.width() + 2 <= usize::from(rect.width) { keys } else { tr!(backup_keys_compact) }
}

fn tab_labels() -> [&'static str; 3] {
    [tr!(backup_tab_selection), tr!(backup_tab_caches), tr!(backup_tab_all)]
}

/// The window, and the tab or item under `point`.
pub(crate) fn spot(window: &BackupWindow, screen: Rect, point: Position) -> (Rect, Option<BackupSpot>) {
    let (rect, body) = frame(screen);
    if point.y == body.y {
        let mut x = body.x;
        for (digit, label) in ('1'..='3').zip(tab_labels()) {
            let width = u16::try_from(label.width()).unwrap_or(u16::MAX).saturating_add(2);
            if (x..x.saturating_add(width)).contains(&point.x) {
                return (rect, Some(BackupSpot::Tab(digit)));
            }
            x = x.saturating_add(width + 1);
        }
    }
    let area = list_area(body, screen);
    if !area.contains(point) {
        return (rect, None);
    }
    let start = window.list_start(usize::from(area.height));
    let at = layout(window).into_iter().nth(start + usize::from(point.y - area.y));
    (rect, at.and_then(|row| if let Row::Item(index, _) = row { Some(BackupSpot::Item(index)) } else { None }))
}

pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    home: Option<&Path>,
    now: Instant,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let window = &state.backup.window;
    let theme = look.theme;
    let styles = &theme.styles;
    let (rect, body) = frame(screen);
    let keys = keys(window, rect);
    Clear.render(rect, buf);
    Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(backup_window_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(keys, styles.overlay).centered())
        .render(rect, buf);
    if body.height < HEADER_ROWS + FOOTER_ROWS + 1 || body.width < 16 {
        return rect;
    }
    let spinner = Spinner::of(&state.busy, Task::Backup, now);
    let list = window.list();
    tabs(window, look, row(body, 0), buf);
    summary(window, look, home, spinner, row(body, 1), buf);
    items(window, look, home, list_area(body, screen), buf);
    let footer = Rect { y: body.bottom() - FOOTER_ROWS + 1, height: FOOTER_ROWS - 1, ..body };
    if let Some(item) = list.hovered() {
        Paragraph::new(about(&item.exclusion))
            .style(dim(theme))
            .wrap(Wrap { trim: true })
            .render(Rect { height: 2, ..footer }, buf);
    }
    let outcome = row(footer, 2);
    match window.working {
        Some((_, password)) => {
            let label = if password { tr!(backup_waiting_password) } else { tr!(backup_working) };
            match spinner {
                Some(spinner) => spinner.line(label, look, emphasis(theme)).render(outcome, buf),
                None => text(buf, outcome, label, emphasis(theme)),
            }
        }
        None => {
            if let Some(message) = window.outcome.as_deref().or(list.error.as_deref()) {
                text(buf, outcome, format::truncate_end(message, usize::from(outcome.width)), emphasis(theme));
            }
        }
    }
    rect
}

fn tabs(window: &BackupWindow, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let spans = BackupTab::ALL.into_iter().zip(tab_labels()).flat_map(|(tab, label)| {
        let style = if tab == window.tab { theme.styles.overlay_selected } else { dim(theme) };
        [Span::styled(format!(" {label} "), style), Span::raw(" ")]
    });
    Line::from(spans.collect::<Vec<_>>()).render(area, buf);
}

fn summary(
    window: &BackupWindow,
    look: &Look<'_>,
    home: Option<&Path>,
    spinner: Option<Spinner>,
    area: Rect,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let list = window.list();
    let folder = shown(&window.root, home);
    let folders = count(usize::try_from(list.folders).unwrap_or(usize::MAX), Count::Folder);
    let label = match window.tab {
        BackupTab::Selection if list.items.is_empty() && !list.loading => tr!(backup_selection_empty).to_owned(),
        BackupTab::Selection => {
            tr!(backup_selection_header, items = count(list.items.len(), Count::Item), folder = folder)
        }
        BackupTab::Caches if list.loading => tr!(backup_caches_scanning, folder = folder, folders = folders),
        BackupTab::Caches if list.items.is_empty() => tr!(backup_caches_none, folder = folder),
        BackupTab::Caches => {
            let backed_up =
                list.items.iter().filter(|item| !item.exclusion.excluded()).filter_map(|item| item.size).sum();
            tr!(
                backup_caches_summary,
                folder = folder,
                found = count(list.items.len(), Count::Folder),
                size = format::size(backed_up)
            )
        }
        BackupTab::All if list.loading => tr!(backup_all_scanning, folder = folder, folders = folders),
        BackupTab::All if list.items.is_empty() => tr!(backup_all_none).to_owned(),
        BackupTab::All => tr!(backup_all_summary, folder = folder),
    };
    let label = format::truncate_start(&label, usize::from(area.width).saturating_sub(2));
    match spinner.filter(|_| list.loading) {
        Some(spinner) => spinner.line(&label, look, emphasis(theme)).render(area, buf),
        None => text(buf, area, label, emphasis(theme)),
    }
}

fn items(window: &BackupWindow, look: &Look<'_>, home: Option<&Path>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let list = window.list();
    let rows = layout(window);
    let visible = usize::from(area.height);
    let start = window.list_start(visible);
    let widest = list.items.iter().map(|item| status(item).width()).max().unwrap_or_default();
    let status_width = u16::try_from(widest).unwrap_or(u16::MAX).clamp(MIN_STATUS_WIDTH, area.width / 2);
    let path_width = area.width.saturating_sub(MARK_WIDTH + SIZE_WIDTH + status_width + GAP * 2);
    for (offset, entry) in rows.iter().skip(start).take(visible).enumerate() {
        let line = row(area, offset as u16);
        match entry {
            Row::Group(group) => text(buf, line, group_label(group), heading(theme)),
            Row::Item(index, item) => {
                let hovered = *index == list.cursor;
                let marked = list.marks.contains(&item.path);
                let base = match (hovered, marked) {
                    (true, true) => styles.marked_cursor,
                    (true, false) => styles.overlay_selected,
                    (false, true) => styles.marked,
                    (false, false) => styles.overlay,
                };
                buf.set_style(line, base);
                let name = match window.tab {
                    BackupTab::All => shown(&item.path, home),
                    _ => relative(&item.path, &window.root, home),
                };
                let name = if item.dir { format!("{name}/") } else { name };
                let size = item.size.map(format::size).unwrap_or_default();
                let status = format::truncate_end(&status(item), usize::from(status_width));
                let tone = if hovered || marked { base } else { tone(&item.exclusion, item.missing, look) };
                Line::from(vec![
                    Span::styled(format::pad_end(if marked { MARK } else { "" }, usize::from(MARK_WIDTH)), base),
                    Span::styled(
                        format::pad_end(
                            &format::truncate_start(&name, usize::from(path_width)),
                            usize::from(path_width),
                        ),
                        base,
                    ),
                    Span::styled(format::pad_start(&size, usize::from(SIZE_WIDTH)), base),
                    Span::styled(" ".repeat(usize::from(GAP * 2)), base),
                    Span::styled(status, tone),
                ])
                .render(line, buf);
            }
        }
    }
}

/// The rows to draw: All exclusions puts each kind under a heading.
fn layout(window: &BackupWindow) -> Vec<Row<'_>> {
    let items = window.list().items.iter().enumerate();
    items
        .flat_map(|(index, item)| window.heading(index).map(Row::Group).into_iter().chain([Row::Item(index, item)]))
        .collect()
}

fn group_label(group: &Exclusion) -> &'static str {
    match group {
        Exclusion::Path => tr!(backup_group_path),
        Exclusion::Volume => tr!(backup_group_volume),
        Exclusion::System => tr!(backup_group_system),
        _ => tr!(backup_group_sticky),
    }
}

pub(crate) fn status_label(exclusion: &Exclusion) -> String {
    match exclusion {
        Exclusion::Included => tr!(backup_status_included).into(),
        Exclusion::Sticky => tr!(backup_status_sticky).into(),
        Exclusion::Path => tr!(backup_status_path).into(),
        Exclusion::Volume => tr!(backup_status_volume).into(),
        Exclusion::Inside(folder) => tr!(backup_status_inside, folder = borg_core::backup_name(folder)),
        Exclusion::System => tr!(backup_status_system).into(),
        Exclusion::Unknown => tr!(backup_status_unknown).into(),
    }
}

fn status(item: &BackupItem) -> String {
    match (item.missing, &item.exclusion) {
        (true, Exclusion::Included) => tr!(backup_status_missing).into(),
        (true, exclusion) => format!("{} · {}", status_label(exclusion), tr!(backup_status_missing)),
        (false, exclusion) => status_label(exclusion),
    }
}

fn tone(exclusion: &Exclusion, missing: bool, look: &Look<'_>) -> Style {
    let theme = look.theme;
    match exclusion {
        _ if missing => dim(theme),
        Exclusion::Sticky | Exclusion::Path | Exclusion::Volume => emphasis(theme),
        Exclusion::Included => theme.styles.overlay,
        Exclusion::Inside(_) | Exclusion::System | Exclusion::Unknown => dim(theme),
    }
}

fn about(exclusion: &Exclusion) -> String {
    match exclusion {
        Exclusion::Included => tr!(backup_about_included).into(),
        Exclusion::Sticky => tr!(backup_about_sticky).into(),
        Exclusion::Path => tr!(backup_about_path).into(),
        Exclusion::Volume => tr!(backup_about_volume).into(),
        Exclusion::Inside(folder) => tr!(backup_about_inside, folder = borg_core::backup_name(folder)),
        Exclusion::System => tr!(backup_about_system).into(),
        Exclusion::Unknown => tr!(backup_about_unknown).into(),
    }
}

/// A path with the home folder shown as `~`.
fn shown(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A path under the window's folder without the folder, others as [`shown`].
fn relative(path: &Path, root: &Path, home: Option<&Path>) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
        _ => shown(path, home),
    }
}
