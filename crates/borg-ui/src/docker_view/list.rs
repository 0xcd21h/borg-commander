//! The container list: Compose projects and their containers, with the filter row above them.

use super::{CPU_WIDTH, DOWN, FILTER_ROW, HEADER_ROW, MEMORY_WIDTH, SHELL_MARK, STATUS_FROM, STATUS_WIDTH, UP, frame};
use crate::{
    Theme,
    activity_view::{FOOTER_ROWS, body, dim, emphasis, heading, readable, row, text, wrap},
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{AppState, Docker, DockerGroup, DockerRow};
use borg_docker::{Action, Container, Health, State};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

/// Name, CPU, memory and status cells of a list row.
pub(super) fn columns(area: Rect) -> [Rect; 4] {
    let status = if area.width >= STATUS_FROM { STATUS_WIDTH } else { 0 };
    let fixed = CPU_WIDTH + MEMORY_WIDTH + status + if status > 0 { 3 } else { 2 };
    let name = area.width.saturating_sub(fixed);
    let mut x = area.x;
    [name, CPU_WIDTH, MEMORY_WIDTH, status].map(|width| {
        let cell = Rect { x, width: width.min(area.right().saturating_sub(x)), ..area };
        x = x.saturating_add(width + 1);
        cell
    })
}

/// The colour of a container's state dot: healthy and running, settling, stopped cleanly,
/// or failing.
pub(super) fn tone(container: &Container, theme: &Theme) -> Style {
    let styles = &theme.styles;
    let failed = matches!(container.state, State::Dead)
        || container.health() == Some(Health::Unhealthy)
        || container.exit_code().is_some_and(|code| code != 0);
    let color = if failed {
        styles.notice.fg
    } else if matches!(container.state, State::Paused | State::Restarting | State::Removing)
        || container.health() == Some(Health::Starting)
    {
        styles.marked.fg
    } else if container.state == State::Running {
        styles.executable.fg
    } else {
        styles.meta.fg
    };
    let base = theme.styles.overlay;
    readable(theme, color).map_or(base, |color| base.fg(color))
}

pub(super) fn dot(container: &Container) -> char {
    if container.state.is_up() { UP } else { DOWN }
}

pub(super) fn working_label(action: Action) -> &'static str {
    match action {
        Action::Start => tr!(docker_starting),
        Action::Stop => tr!(docker_stopping),
        Action::Restart => tr!(docker_restarting),
        Action::Pause => tr!(docker_pausing),
        Action::Unpause => tr!(docker_resuming),
    }
}

pub(super) fn cpu(value: u64) -> String {
    format!("{:.1}", value as f64 / 100.0)
}

pub(super) fn project_name(group: &DockerGroup) -> String {
    group.project.clone().unwrap_or_else(|| tr!(docker_standalone).into())
}

pub(super) fn running_of(group: &DockerGroup) -> String {
    tr!(docker_running_of, running = group.running, total = group.containers.len())
}

pub(super) fn draw_list(state: &AppState, look: &Look<'_>, rect: Rect, spinner: Option<Spinner>, buf: &mut Buffer) {
    let docker = &state.docker;
    let theme = look.theme;
    let styles = &theme.styles;
    let focused = !docker.details_focused;
    let title = match &docker.context {
        Some(context) => format!("{} · {context}", tr!(docker_title)),
        None => tr!(docker_title).into(),
    };
    let inner = frame(docker, look, rect, &title, focused, buf);
    if inner.is_empty() {
        return;
    }
    let running = docker.containers.iter().filter(|c| c.state == State::Running).count();
    if docker.loaded && docker.error.is_none() {
        let summary = tr!(docker_running_of, running = running, total = docker.containers.len());
        text(buf, row(inner, 0), summary, emphasis(theme));
    }
    filter_row(docker, look, row(inner, FILTER_ROW), buf);
    let [name, cpu_cell, memory, status] = columns(row(inner, HEADER_ROW));
    for (cell, label) in [
        (name, tr!(docker_name)),
        (cpu_cell, tr!(docker_cpu)),
        (memory, tr!(docker_memory)),
        (status, tr!(docker_status)),
    ] {
        text(buf, cell, label, styles.menu);
    }
    let body = body(rect);
    if let Some(error) = &docker.error {
        for (offset, line) in wrap(error, usize::from(body.width)).into_iter().enumerate() {
            text(buf, row(body, offset as u16), line, styles.notice);
        }
    } else if !docker.loaded {
        if let Some(spinner) = spinner {
            spinner.draw_centered(tr!(docker_loading), look, dim(theme), body, buf);
        }
    } else if docker.rows.is_empty() {
        let note = if docker.filter.is_empty() { tr!(docker_empty) } else { tr!(docker_no_matches) };
        text(buf, row(body, 0), note, dim(theme));
    }
    let start = docker.list_start(usize::from(body.height));
    for (offset, (index, &item)) in
        docker.rows.iter().enumerate().skip(start).take(usize::from(body.height)).enumerate()
    {
        let line = row(body, offset as u16);
        let selected = index == docker.cursor;
        let base = match (selected, focused) {
            (true, true) => styles.cursor,
            (true, false) => styles.cursor_inactive,
            _ => styles.overlay,
        };
        buf.set_style(line, base);
        let [name, cpu_cell, memory, status] = columns(line);
        match item {
            DockerRow::Group(group) => {
                let group = &docker.groups[group];
                let (cpu_total, memory_total) = docker.group_usage(group);
                let fold = if docker.collapsed(group) && docker.filter.is_empty() { '▸' } else { '▾' };
                let style = if selected { base } else { heading(theme) };
                // With nothing running, the name may use the empty CPU and memory columns.
                let label =
                    if group.running > 0 { name } else { Rect { width: status.x.saturating_sub(name.x + 1), ..name } };
                text(buf, label, format!("{fold} {}", project_name(group)), style.add_modifier(Modifier::BOLD));
                if group.running > 0 {
                    text(buf, cpu_cell, cpu(cpu_total), base);
                    text(buf, memory, format::size(memory_total), base);
                }
                text(buf, status, running_of(group), if selected { base } else { dim(theme) });
            }
            DockerRow::Container(container) => {
                let container = &docker.containers[container];
                let dot_style = if selected { base } else { tone(container, theme) };
                Line::from(vec![
                    Span::styled(format!("  {} ", dot(container)), dot_style),
                    Span::styled(container.label().to_owned(), base),
                    Span::styled(
                        if docker.shell(&container.id).is_some() { format!(" {SHELL_MARK}") } else { String::new() },
                        if selected { base } else { heading(theme) },
                    ),
                ])
                .render(name, buf);
                if let Some(usage) = docker.usage.get(&container.id).filter(|_| container.state.is_up()) {
                    text(buf, cpu_cell, cpu(u64::from(usage.latest.cpu)), base);
                    text(buf, memory, format::size(usage.latest.memory), base);
                }
                let note = docker
                    .working_on(&container.id)
                    .map_or_else(|| container.status.clone(), |a| working_label(a).into());
                text(
                    buf,
                    status,
                    format::truncate_end(&note, usize::from(status.width)),
                    if selected { base } else { dim(theme) },
                );
            }
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    match (spinner.filter(|_| docker.loaded), docker.working.first()) {
        (Some(spinner), Some(work)) => {
            spinner.line(working_label(work.action), look, dim(theme)).render(row(footer, 0), buf)
        }
        _ => text(buf, row(footer, 0), state.notice.as_deref().unwrap_or(""), dim(theme)),
    }
}

/// The container filter: an input with a cursor while typing, a badge while a filter
/// applies, and a hint otherwise.
pub(super) fn filter_row(docker: &Docker, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let label = tr!(activity_filter_label);
    let shown = docker.rows.iter().filter(|r| matches!(r, DockerRow::Container(_))).count();
    let shown = if docker.filter.is_empty() { docker.containers.len() } else { shown };
    let total = docker.containers.len();
    if docker.filtering {
        buf.set_style(area, styles.input);
        let typed = format!(" {}", docker.filter);
        Line::from(vec![Span::styled(label, styles.cursor), Span::styled(typed.clone(), styles.input)])
            .render(area, buf);
        let x = area.x.saturating_add((label.width() + typed.width()) as u16);
        if x < area.right() {
            buf[(x, area.y)].set_style(styles.input.add_modifier(Modifier::REVERSED));
        }
        let hint = tr!(activity_filter_typing, shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !docker.filter.is_empty() {
        let hint = tr!(activity_filter_applied, shown = shown, total = total);
        Line::from(vec![
            Span::styled(label, styles.cursor),
            Span::styled(format!(" {} ", docker.filter), heading(theme)),
            Span::styled(hint, dim(theme)),
        ])
        .render(area, buf);
    } else {
        text(buf, area, tr!(docker_filter_placeholder), dim(theme));
    }
}
