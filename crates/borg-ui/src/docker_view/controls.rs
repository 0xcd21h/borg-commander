//! The container view's key bar, and what clicks and the wheel do in it.

use super::{FILTER_ROW, SHELL_WHEEL, project_name, tabs};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};
use borg_core::{AppState, Docker, FileOp, Msg, Pointer};
use borg_docker::State;
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

pub(super) fn buttons(docker: &Docker, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let specs: Vec<(&'static str, KeyCode)> = if docker.filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else {
        let paused = docker.container().is_some_and(|c| c.state == State::Paused)
            || (docker.container().is_none()
                && docker
                    .group()
                    .is_some_and(|g| g.containers.iter().any(|&i| docker.containers[i].state == State::Paused)));
        vec![
            (tr!(docker_start_button), KeyCode::Char('s')),
            (tr!(docker_stop_button), KeyCode::Char('x')),
            (tr!(docker_restart_button), KeyCode::Char('r')),
            (if paused { tr!(docker_resume_button) } else { tr!(docker_pause_button) }, KeyCode::Char('p')),
            (tr!(docker_files_button), KeyCode::Char('f')),
            (tr!(docker_shell_button), KeyCode::Char('e')),
            (tr!(activity_filter_button), KeyCode::Char('/')),
            (if docker.details_focused { tr!(docker_list_button) } else { tr!(docker_details_button) }, KeyCode::Tab),
            (if docker.details_focused { tr!(activity_back_button) } else { tr!(activity_close_button) }, KeyCode::Esc),
        ]
    };
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

/// What a panel's footer says while Docker works on its files.
pub(crate) fn file_work_label(work: &borg_core::FileWork) -> String {
    match &work.op {
        FileOp::Delete { .. } => tr!(docker_deleting).into(),
        FileOp::Mkdir { .. } => tr!(docker_creating).into(),
        FileOp::Fetch { .. } => tr!(docker_opening).into(),
    }
}

/// The buttons while typing in a shell: back to the list, and maximize.
pub(super) fn typing_buttons(docker: &Docker, area: Rect) -> Vec<(Rect, &'static str, Pointer)> {
    let mut x = area.x;
    [
        (tr!(docker_overview_tab), Pointer::DockerControl { details: false, code: KeyCode::Char('1') }),
        (tr!(docker_list_button_alt), Pointer::DockerControl { details: false, code: KeyCode::Null }),
        (tr!(docker_maximize_button), Pointer::DockerExpand { details: true, expand: !docker.expanded }),
    ]
    .into_iter()
    .map(|(label, pointer)| {
        let rect = Rect { x, width: (label.width() as u16 + 2).min(area.right().saturating_sub(x)), ..area };
        x += rect.width;
        (rect, label, pointer)
    })
    .collect()
}

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let docker = &state.docker;
    buf.set_style(area, theme.styles.key_label);
    if let Some(shell) = docker.visible_shell().filter(|_| docker.typing()) {
        for (rect, label, _) in typing_buttons(docker, row(area, 0)) {
            text(buf, rect, label, theme.styles.key);
        }
        let hint = tr!(docker_typing_hint, name = shell.name);
        text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
        return;
    }
    for (rect, label, _) in buttons(docker, row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if docker.filtering {
        tr!(activity_filter_hint)
    } else if docker.details_focused {
        tr!(docker_details_hint)
    } else {
        tr!(docker_list_hint)
    };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}

/// The status line: the selected container's name, image and id.
pub(crate) fn status(docker: &Docker) -> String {
    match docker.container() {
        Some(container) => {
            format!("{} · {} · {}", container.name, container.image, container.id.get(..12).unwrap_or(&container.id))
        }
        None => docker.group().map(project_name).unwrap_or_default(),
    }
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let docker = &state.docker;
    let point = Position::new(event.column, event.row);
    let left_click = event.kind == MouseEventKind::Down(MouseButton::Left);
    if layout.keybar.contains(point) {
        if !left_click {
            return None;
        }
        if docker.typing() {
            return typing_buttons(docker, row(layout.keybar, 0))
                .into_iter()
                .find(|(rect, _, _)| rect.contains(point))
                .map(|(_, _, pointer)| Msg::Pointer(pointer));
        }
        return buttons(docker, row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::DockerControl { details: docker.details_focused, code }));
    }
    let side = docker.side?;
    for panel in [side, side.other()] {
        let rect = layout.panels[panel.index()];
        if !rect.contains(point) {
            continue;
        }
        let details = panel != side;
        if left_click
            && let Some((_, _, action)) =
                crate::results_view::controls(rect, docker.expanded).into_iter().find(|(r, _, _)| r.contains(point))
        {
            return Some(Msg::Pointer(match action {
                borg_core::Action::SetExpanded(expand) => Pointer::DockerExpand { details, expand },
                _ => Pointer::DockerHide,
            }));
        }
        if details && docker.visible_shell().is_some() {
            match event.kind {
                MouseEventKind::ScrollUp => return Some(Msg::Pointer(Pointer::DockerShellScroll(-SHELL_WHEEL))),
                MouseEventKind::ScrollDown => return Some(Msg::Pointer(Pointer::DockerShellScroll(SHELL_WHEEL))),
                _ => {}
            }
        }
        if details
            && left_click
            && let Some(container) = docker.container()
            && let Some((_, _, code, _)) =
                tabs(docker, container, row(inner(rect), 1)).into_iter().find(|(r, _, _, _)| r.contains(point))
        {
            return Some(Msg::Pointer(Pointer::DockerControl { details: false, code }));
        }
        let wheel = match event.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = wheel {
            return Some(Msg::Pointer(Pointer::DockerControl { details: false, code }));
        }
        if !left_click {
            return None;
        }
        if details {
            return Some(Msg::Pointer(Pointer::DockerControl { details: true, code: KeyCode::Null }));
        }
        if row(inner(rect), FILTER_ROW).contains(point) {
            return Some(Msg::Pointer(Pointer::DockerControl { details: false, code: KeyCode::Char('/') }));
        }
        let body = body(rect);
        if body.contains(point) {
            let index = docker.list_start(usize::from(body.height)) + usize::from(point.y - body.y);
            return Some(Msg::Pointer(Pointer::DockerRow { index, open: double }));
        }
        return Some(Msg::Pointer(Pointer::DockerControl { details: false, code: KeyCode::Null }));
    }
    None
}
