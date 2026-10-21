use crate::theme::Styles;
use borg_core::{Action, AppState};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

pub(crate) fn buttons(state: &AppState, area: Rect) -> Vec<(Rect, &'static str, Action)> {
    let mut items = vec![
        (tr!(ctrl_f_search), Action::Search),
        (tr!(alt_v_collections), Action::OpenCollection),
        (tr!(alt_d_size_search), Action::AnalyzeSpace),
    ];
    if state.active_panel().is_tree() {
        items = vec![
            (tr!(tree_branches), Action::ToggleTreeNode),
            (tr!(tree_open_folder), Action::Open),
            (tr!(ctrl_r_sizes), Action::RefreshSizes),
        ];
    } else if state.active_panel().is_size_view() {
        items = vec![(tr!(ctrl_f_search), Action::Search), (tr!(ctrl_r_sizes), Action::RefreshSizes)];
    } else if !state.active_panel().is_results() {
        items.insert(1, (tr!(alt_s_sizes), Action::CalculateSizes));
        if state.settings.preferences.system_plugin {
            items.insert(1, (tr!(alt_m_activity), Action::Activity));
        }
    }
    if state.active_panel().is_results() {
        items.extend([
            (tr!(esc_hide), Action::BackResults),
            (if state.results_expanded() { tr!(alt_w_restore) } else { tr!(alt_w_maximize) }, Action::ToggleExpanded),
            (tr!(ctrl_s_filter), Action::FilterResults),
            (tr!(ins_mark), Action::ToggleMark),
            (tr!(alt_b_collect), Action::Collect),
            (tr!(shift_f8_delete), Action::DeleteOriginals),
        ]);
    } else {
        if state.last_results[state.active.index()].is_some() {
            items.push((tr!(alt_r_resume), Action::BackResults));
        }
        items.push((tr!(alt_b_collect), Action::Collect));
    }
    if state.sizing.is_some() || !state.active_panel().is_results() {
        items.insert(
            0,
            (if state.sizing.is_some() { tr!(alt_i_stop_sizes) } else { tr!(alt_i_count_sizes) }, Action::MeasureSizes),
        );
    }
    let mut x = area.x;
    items
        .into_iter()
        .filter_map(|(label, action)| {
            let width = label.width() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let rect = Rect { x, width, ..area };
            x += width;
            Some((rect, label, action))
        })
        .collect()
}

pub(crate) fn draw(state: &AppState, area: Rect, styles: &Styles<Style>, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    buf.set_style(area, styles.key_label);
    for (rect, label, _) in buttons(state, area) {
        let (key, label) = label.split_once(' ').unwrap_or((label, ""));
        Line::from(vec![Span::styled(key, styles.key), Span::styled(format!(" {label}  "), styles.key_label)])
            .render(rect, buf);
    }
}

pub(crate) fn action_at(state: &AppState, area: Rect, point: Position) -> Option<Action> {
    buttons(state, area).into_iter().find(|(rect, _, _)| rect.contains(point)).map(|(_, _, action)| action)
}
