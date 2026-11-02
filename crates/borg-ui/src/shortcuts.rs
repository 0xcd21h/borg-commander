use crate::theme::Styles;
use borg_core::{Action, AppState, JobState};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

/// The Jobs button while jobs exist: one job's progress, or how many are running. It is
/// highlighted while a job waits for an answer.
fn jobs(state: &AppState) -> Option<(String, bool)> {
    let waiting = state.jobs.iter().any(|job| matches!(job.state, JobState::Asking(_)));
    let status = match state.jobs.as_slice() {
        [] => return None,
        [job] => crate::job_view::status(job),
        _ if waiting => tr!(jobs_waiting).to_owned(),
        jobs => tr!(jobs_running, count = jobs.len()),
    };
    Some((tr!(alt_j_jobs, status = status), waiting))
}

/// The Docker shortcut, counting the container shells still open.
fn docker_label(state: &AppState) -> String {
    match state.docker.shells.len() {
        0 => tr!(alt_c_docker).into(),
        1 => tr!(alt_c_docker_shells_one).into(),
        count => tr!(alt_c_docker_shells_many, count = count),
    }
}

pub(crate) fn buttons(state: &AppState, area: Rect) -> Vec<(Rect, String, Action, bool)> {
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
        if state.settings.preferences.system_plugin {
            items.insert(1, (tr!(alt_n_network), Action::Network));
        }
        if state.settings.preferences.docker_plugin {
            items.insert(1, (tr!(alt_c_docker), Action::Docker));
        }
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
            (tr!(alt_k_collect), Action::Collect),
            (tr!(shift_f8_delete), Action::DeleteOriginals),
        ]);
    } else {
        if state.last_results[state.active.index()].is_some() {
            items.push((tr!(alt_shift_r_resume), Action::BackResults));
        }
        items.push((tr!(alt_k_collect), Action::Collect));
    }
    // A container's filesystem offers none of the host's searches or sizes.
    if state.active_panel().mount.is_some() {
        items = vec![(tr!(alt_c_docker), Action::Docker)];
    } else if state.sizing.is_some() || !state.active_panel().is_results() {
        items.insert(
            0,
            (if state.sizing.is_some() { tr!(alt_i_stop_sizes) } else { tr!(alt_i_count_sizes) }, Action::MeasureSizes),
        );
    }
    let mut items: Vec<(String, Action, bool)> = items
        .into_iter()
        .map(|(label, action)| {
            (if action == Action::Docker { docker_label(state) } else { label.to_owned() }, action, false)
        })
        .collect();
    if let Some((label, waiting)) = jobs(state) {
        items.insert(0, (label, Action::Jobs, waiting));
    }
    // A connected agent stays in sight; its chip opens the log, and the mode chip stops automatic approvals.
    if state.agent.connected() {
        items.insert(0, (tr!(agent_chip).to_owned(), Action::AgentLog, true));
    }
    if !state.agent.auto_approve.is_empty() {
        items.insert(0, (tr!(agent_approve_all_chip).to_owned(), Action::ApprovalModeOff, true));
    }
    items.insert(0, (tr!(alt_h_remote_hosts).to_owned(), Action::BrowseRemotes, false));
    let mut x = area.x;
    items
        .into_iter()
        .filter_map(|(label, action, attention)| {
            let width = label.width() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let rect = Rect { x, width, ..area };
            x += width;
            Some((rect, label, action, attention))
        })
        .collect()
}

pub(crate) fn draw(state: &AppState, area: Rect, styles: &Styles<Style>, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    buf.set_style(area, styles.key_label);
    for (rect, label, _, attention) in buttons(state, area) {
        let (key, label) = label.split_once(' ').unwrap_or((&label, ""));
        let label_style = if attention { styles.cursor } else { styles.key_label };
        Line::from(vec![Span::styled(key.to_owned(), styles.key), Span::styled(format!(" {label}  "), label_style)])
            .render(rect, buf);
    }
}

pub(crate) fn action_at(state: &AppState, area: Rect, point: Position) -> Option<Action> {
    buttons(state, area).into_iter().find(|(rect, ..)| rect.contains(point)).map(|(_, _, action, _)| action)
}
