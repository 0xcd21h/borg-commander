//! The activity monitor's key bar, and what clicks and the wheel do in its panes.

use super::processes::columns;
use super::search::{form_field, results_list};
use super::{SEARCH_HEADER, body, inner, row, tabs, text};
use crate::Theme;
use borg_core::{Activity, ActivityPage, AppState, FINDING_NOTE, Msg, Pointer};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

pub(super) fn buttons(activity: &Activity, page: ActivityPage, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let mut specs: Vec<_> = if activity.retry.is_some() && activity.error.is_some() {
        vec![(tr!(activity_admin_button), KeyCode::Char('a')), (tr!(activity_back_button), KeyCode::Esc)]
    } else {
        match page {
            ActivityPage::Processes if activity.filtering => {
                vec![
                    (tr!(activity_filter_keep_button), KeyCode::Enter),
                    (tr!(activity_filter_clear_button), KeyCode::Esc),
                ]
            }
            ActivityPage::Processes => vec![
                (tr!(activity_memory_button), KeyCode::Enter),
                (tr!(activity_filter_button), KeyCode::Char('/')),
                (tr!(activity_signal_button), KeyCode::Char('k')),
                (tr!(activity_pause_button), KeyCode::Char(' ')),
                (tr!(activity_close_button), KeyCode::Esc),
            ],
            ActivityPage::Memory => {
                vec![
                    (tr!(activity_map_button), KeyCode::Enter),
                    (tr!(memory_search_shortcut), KeyCode::Char('/')),
                    (tr!(activity_back_button), KeyCode::Esc),
                ]
            }
            ActivityPage::Regions => vec![
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(activity_capture_button), KeyCode::Enter),
                (
                    if activity.map { tr!(activity_bytes_button) } else { tr!(activity_page_map_button) },
                    KeyCode::Char('m'),
                ),
                (tr!(integrity_button), KeyCode::Char('i')),
                (tr!(activity_refresh_button), KeyCode::Char('r')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
            ActivityPage::Files | ActivityPage::FileDetails => {
                let mut specs =
                    vec![(tr!(activity_refresh_button), KeyCode::Char('r')), (tr!(activity_back_button), KeyCode::Esc)];
                if activity.previewable_file().is_some() {
                    specs.insert(0, (tr!(process_file_view_button), KeyCode::F(3)));
                }
                specs
            }
            ActivityPage::Search => {
                let copies = activity
                    .search_report
                    .as_ref()
                    .and_then(|report| report.hits.get(activity.search_cursor))
                    .is_some_and(|hit| hit.places.len() > 1);
                let mut specs = vec![(tr!(memory_preview_button), KeyCode::Enter)];
                if copies {
                    specs.push((tr!(memory_next_copy_button), KeyCode::Char('c')));
                }
                specs.extend([
                    (tr!(memory_search_shortcut), KeyCode::Char('/')),
                    (tr!(memory_continue_button), KeyCode::Char('n')),
                    (tr!(activity_back_button), KeyCode::Esc),
                ]);
                specs
            }
            ActivityPage::Integrity => vec![
                (tr!(memory_preview_button), KeyCode::Enter),
                (tr!(integrity_again_button), KeyCode::Char('r')),
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
            ActivityPage::Snapshot if activity.map => vec![
                (tr!(activity_open_page_button), KeyCode::Enter),
                (tr!(activity_bytes_button), KeyCode::Char('m')),
                (tr!(activity_refresh_button), KeyCode::Char('r')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
            ActivityPage::Snapshot => vec![
                (tr!(memory_search_shortcut), KeyCode::Char('/')),
                (tr!(activity_mode_button), KeyCode::Char('h')),
                (tr!(activity_diff_button), KeyCode::Char('d')),
                (tr!(activity_refresh_button), KeyCode::Char('r')),
                (tr!(activity_export_button), KeyCode::Char('e')),
                (tr!(activity_back_button), KeyCode::Esc),
            ],
        }
    };
    if activity.retry.is_some() && activity.error.is_none() {
        specs.insert(0, (tr!(activity_admin_button), KeyCode::Char('a')));
    }
    let mut x = area.x;
    specs
        .into_iter()
        .filter_map(|(label, key)| {
            let width = label.width() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let cell = Rect { x, width, ..area };
            x += width;
            Some((cell, label, key))
        })
        .collect()
}

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let activity = &state.activity;
    let page = activity.focused_page();
    buf.set_style(area, theme.styles.key_label);
    let controls = if activity.search_form.is_some() {
        vec![(row(area, 0), tr!(memory_submit_button), KeyCode::Enter)]
    } else {
        buttons(activity, page, row(area, 0))
    };
    for (rect, label, _) in controls {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if activity.search_form.is_some() {
        tr!(memory_search_hint)
    } else {
        match page {
            ActivityPage::Processes if activity.filtering => tr!(activity_filter_hint),
            ActivityPage::Processes => tr!(activity_list_hint),
            ActivityPage::Memory => tr!(activity_memory_hint),
            ActivityPage::Regions => tr!(activity_map_hint),
            ActivityPage::Snapshot if activity.map => tr!(census_hint),
            ActivityPage::Snapshot => tr!(activity_snapshot_hint),
            ActivityPage::Search if activity.search_report.as_ref().is_some_and(|r| r.strings.is_some()) => {
                tr!(memory_strings_hint)
            }
            ActivityPage::Search => tr!(memory_results_hint),
            ActivityPage::Integrity => tr!(integrity_hint),
            ActivityPage::FileDetails if activity.previewable_file().is_some() => tr!(process_file_scroll_hint),
            ActivityPage::Files if activity.previewable_file().is_some() => tr!(process_file_preview_hint),
            ActivityPage::Files | ActivityPage::FileDetails => tr!(process_files_hint),
        }
    };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let activity = &state.activity;
    let preview = activity.preview_focused;
    let point = Position::new(event.column, event.row);
    if layout.keybar.contains(point) {
        if activity.search_form.is_some()
            && row(layout.keybar, 0).contains(point)
            && event.kind == MouseEventKind::Down(MouseButton::Left)
        {
            return Some(Msg::Pointer(Pointer::ActivityControl { preview: true, code: KeyCode::Enter }));
        }
        let page = if preview { activity.page } else { activity.list };
        return buttons(activity, page, row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::ActivityControl { preview, code }));
    }
    let side = activity.side?;
    for panel in [side, side.other()] {
        let rect = layout.panels[panel.index()];
        if !rect.contains(point) {
            continue;
        }
        let preview = panel != side;
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some((_, _, action)) =
                crate::results_view::controls(rect, activity.expanded).into_iter().find(|(r, _, _)| r.contains(point))
        {
            return Some(Msg::Pointer(match action {
                borg_core::Action::SetExpanded(expand) => Pointer::ActivityExpand { preview, expand },
                _ => Pointer::ActivityHide,
            }));
        }

        let code = match event.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = code {
            return Some(Msg::Pointer(Pointer::ActivityControl { preview, code }));
        }
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        let body = body(rect);
        if !preview && activity.list != ActivityPage::Processes && row(inner(rect), 1).contains(point) {
            return tabs(row(inner(rect), 1))
                .into_iter()
                .find(|(r, _, _)| r.contains(point))
                .map(|(_, _, code)| Msg::Pointer(Pointer::ActivityControl { preview: false, code }));
        }
        if !preview && activity.list == ActivityPage::Processes && row(inner(rect), 1).contains(point) {
            return Some(Msg::Pointer(Pointer::ActivityControl { preview: false, code: KeyCode::Char('/') }));
        }
        if !preview && activity.list == ActivityPage::Processes && row(inner(rect), 2).contains(point) {
            return columns(row(inner(rect), 2))
                .into_iter()
                .find(|(_, r)| r.contains(point))
                .map(|(sort, _)| Msg::Pointer(Pointer::ActivitySort(sort)));
        }
        if preview && activity.search_form.is_some() {
            return (0..4)
                .find(|&i| form_field(body, i).contains(point))
                .map(|i| Msg::Pointer(Pointer::ActivitySearchField(i)));
        }
        let body = match activity.list {
            _ if preview => body,
            ActivityPage::Search => results_list(body, SEARCH_HEADER, 0),
            ActivityPage::Integrity => results_list(body, 1, FINDING_NOTE),
            _ => body,
        };
        if body.contains(point) && !preview {
            let index = activity.list_start(activity.list, usize::from(body.height)) + usize::from(point.y - body.y);
            return Some(Msg::Pointer(Pointer::ActivityRow { index, open: double, preview }));
        }
        return Some(Msg::Pointer(Pointer::ActivityControl { preview, code: KeyCode::Null }));
    }
    None
}
