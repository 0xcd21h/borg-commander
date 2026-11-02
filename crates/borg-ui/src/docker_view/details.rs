//! The selected project's or container's facts, resource tiles and live charts.

use super::{
    CHART_MIN_ROWS, DOCKER_TILE_WIDTH, LABEL_WIDTH, MOUNTS_SHOWN, SIDE_BY_SIDE_FROM, columns, cpu, dot, frame, header,
    local, project_name, running_of, tone, working_label,
};
use crate::{
    Theme,
    activity_view::{Unit, body, chart, dim, emphasis, heading, row, text},
    format,
    look::Look,
};
use borg_core::{Docker, DockerGroup};
use borg_docker::Container;
use borg_i18n::tr;
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};

/// Labelled values, one per row: the label dimmed in a column `label` cells wide, the value
/// after it.
pub(crate) struct Facts<'a> {
    pub(crate) area: Rect,
    pub(crate) y: u16,
    pub(crate) label: u16,
    pub(crate) theme: &'a Theme,
}

impl Facts<'_> {
    pub(crate) fn add(&mut self, buf: &mut Buffer, label: &str, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() || self.y >= self.area.height {
            return;
        }
        let line = row(self.area, self.y);
        text(buf, Rect { width: self.label.min(line.width), ..line }, label, dim(self.theme));
        let rest =
            Rect { x: line.x + self.label.min(line.width), width: line.width.saturating_sub(self.label), ..line };
        text(buf, rest, format::truncate_end(&value, usize::from(rest.width)), self.theme.styles.overlay);
        self.y += 1;
    }
}

/// Labelled values in a row of tiles, as in the activity monitor. Returns the rows used.
pub(crate) fn tiles(buf: &mut Buffer, area: Rect, tiles: &[(&str, String)], theme: &Theme) -> u16 {
    if tiles.is_empty() || area.width < DOCKER_TILE_WIDTH {
        return 0;
    }
    let per_row = (area.width / DOCKER_TILE_WIDTH).clamp(1, tiles.len() as u16);
    let width = area.width / per_row;
    let mut y = 0;
    for line in tiles.chunks(usize::from(per_row)) {
        for (i, (label, value)) in line.iter().enumerate() {
            let x = area.x + i as u16 * width;
            let cell = |dy: u16| Rect { x, width: width.saturating_sub(1), ..row(area, y + dy) };
            text(buf, cell(0), *label, dim(theme));
            let accent =
                if i == 0 && y == 0 { heading(theme) } else { theme.styles.overlay.add_modifier(Modifier::BOLD) };
            text(buf, cell(1), value.clone(), accent);
        }
        y += 3;
    }
    y
}

/// One chart's values, a second apart, with its title and what they measure.
pub(crate) struct Series<'a> {
    pub(crate) title: &'a str,
    pub(crate) values: &'a [u64],
    pub(crate) unit: Unit,
}

impl Series<'_> {
    fn draw(&self, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
        chart(self.title, self.values, self.unit, look, area, buf);
    }
}

/// Two charts in what is left of the pane: side by side when wide, stacked when tall,
/// otherwise the first alone.
pub(crate) fn charts(buf: &mut Buffer, area: Rect, [first, second]: [Series<'_>; 2], look: &Look<'_>) {
    if area.height < CHART_MIN_ROWS {
        return;
    }
    if area.width >= SIDE_BY_SIDE_FROM {
        let half = area.width / 2;
        first.draw(look, Rect { width: half - 1, ..area }, buf);
        second.draw(look, Rect { x: area.x + half + 1, width: area.width - half - 1, ..area }, buf);
    } else if area.height > CHART_MIN_ROWS * 2 {
        let half = area.height / 2;
        first.draw(look, Rect { height: half - 1, ..area }, buf);
        second.draw(look, Rect { y: area.y + half, height: area.height - half, ..area }, buf);
    } else {
        first.draw(look, area, buf);
    }
}

/// A container's or project's CPU and memory charts.
fn usage<'a>(cpu: &'a [u64], memory: &'a [u64]) -> [Series<'a>; 2] {
    [
        Series { title: tr!(docker_cpu_chart), values: cpu, unit: Unit::Percent },
        Series { title: tr!(docker_memory_chart), values: memory, unit: Unit::Bytes },
    ]
}

/// The sum of several histories, aligned so that their latest samples line up.
pub(super) fn summed(histories: &[&[u64]]) -> Vec<u64> {
    let length = histories.iter().map(|h| h.len()).max().unwrap_or(0);
    (0..length)
        .map(|i| histories.iter().filter_map(|history| history.get((i + history.len()).checked_sub(length)?)).sum())
        .collect()
}

pub(super) fn draw_details(
    docker: &Docker,
    look: &Look<'_>,
    rect: Rect,
    shell: Option<&Screen<'_>>,
    clock: &Zoned,
    buf: &mut Buffer,
) -> Option<Position> {
    let focused = docker.details_focused;
    if let Some(container) = docker.container() {
        let title = match &container.project {
            Some(project) => format!("{project} › {}", container.label()),
            None => container.label().to_owned(),
        };
        let inner = frame(docker, look, rect, &title, focused, buf);
        if inner.is_empty() {
            return None;
        }
        header(container, docker, look.theme, inner, buf);
        if let (Some(_), Some(screen)) = (docker.visible_shell(), shell) {
            let area = borg_core::shell_rect(rect);
            crate::term_view::draw_screen(screen, area, buf);
            let (x, y) = screen.cursor();
            let cursor = Position::new(area.x.saturating_add(x), area.y.saturating_add(y));
            return (docker.typing() && screen.cursor_visible() && area.contains(cursor)).then_some(cursor);
        }
        container_details(docker, container, look, rect, clock, buf);
    } else if let Some(group) = docker.group() {
        let inner = frame(docker, look, rect, &project_name(group), focused, buf);
        group_details(docker, group, look, inner, rect, buf);
    } else {
        frame(docker, look, rect, tr!(docker_container), focused, buf);
    }
    None
}

pub(super) fn container_details(
    docker: &Docker,
    container: &Container,
    look: &Look<'_>,
    rect: Rect,
    clock: &Zoned,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let body = body(rect);
    let usage = docker.usage.get(&container.id).filter(|_| container.state.is_up());
    let used = usage.map_or(0, |usage| {
        let stats = usage.latest;
        let limit =
            if stats.memory_limit > 0 { format!(" / {}", format::size(stats.memory_limit)) } else { String::new() };
        tiles(
            buf,
            body,
            &[
                (tr!(docker_cpu_tile), format!("{}%", cpu(u64::from(stats.cpu)))),
                (tr!(docker_memory_tile), format!("{}{limit}", format::size(stats.memory))),
                (tr!(docker_network_tile), format!("{} / {}", format::size(stats.net_rx), format::size(stats.net_tx))),
                (tr!(docker_disk_tile), format!("{} / {}", format::size(stats.read), format::size(stats.written))),
                (tr!(docker_pids_tile), stats.pids.to_string()),
            ],
            theme,
        )
    });
    let rest = Rect { y: body.y + used.min(body.height), height: body.height.saturating_sub(used), ..body };
    let mut facts = Facts { area: rest, y: 0, label: LABEL_WIDTH, theme };
    let details = docker.selected_details();
    facts.add(buf, tr!(docker_image), container.image.clone());
    if let (Some(project), Some(service)) = (&container.project, &container.service) {
        facts.add(buf, tr!(docker_compose), format!("{project} · {service}"));
    }
    facts.add(buf, tr!(docker_ports), container.ports.clone());
    let networks = details.map_or_else(
        || container.networks.clone(),
        |d| {
            d.networks
                .iter()
                .map(|n| format!("{} {}", n.name, n.address).trim().to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    facts.add(buf, tr!(docker_networks), networks);
    if let Some(details) = details {
        let mut mounts: Vec<String> = details
            .mounts
            .iter()
            .take(MOUNTS_SHOWN)
            .map(|m| format!("{} → {}{}", m.source, m.destination, if m.writable { "" } else { " (ro)" }))
            .collect();
        if details.mounts.len() > MOUNTS_SHOWN {
            mounts.push(format!("+{}", details.mounts.len() - MOUNTS_SHOWN));
        }
        facts.add(buf, tr!(docker_mounts), mounts.join(", "));
        facts.add(buf, tr!(docker_command), details.command.clone());
        if container.state.is_up() {
            facts.add(
                buf,
                tr!(docker_started),
                details.started.as_deref().map(|t| local(t, clock)).unwrap_or_default(),
            );
            facts.add(buf, tr!(docker_health), details.health.clone().unwrap_or_default());
        } else {
            facts.add(
                buf,
                tr!(docker_finished),
                details.finished.as_deref().map(|t| local(t, clock)).unwrap_or_default(),
            );
            let code = details.exit_code.to_string();
            facts.add(buf, tr!(docker_exit_code), if details.oom_killed { tr!(docker_oom, code = code) } else { code });
        }
        let policy = if details.restart_policy.is_empty() { "no" } else { &details.restart_policy };
        facts.add(buf, tr!(docker_restarts), tr!(docker_restarts_value, policy = policy, count = details.restarts));
    } else {
        facts.add(buf, tr!(docker_command), container.command.clone());
        text(buf, row(rest, facts.y), tr!(docker_reading), dim(theme));
        facts.y += 1;
    }
    facts.add(buf, tr!(docker_created), container.created.get(..16).unwrap_or(&container.created).to_owned());
    let left = Rect { y: rest.y + facts.y + 1, height: rest.height.saturating_sub(facts.y + 1), ..rest };
    match usage {
        Some(usage) => charts(buf, left, self::usage(&usage.cpu, &usage.memory), look),
        None if !container.state.is_up() => text(buf, row(left, 0), tr!(docker_not_running), dim(theme)),
        None => {}
    }
}

pub(super) fn group_details(
    docker: &Docker,
    group: &DockerGroup,
    look: &Look<'_>,
    inner: Rect,
    rect: Rect,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    if inner.is_empty() {
        return;
    }
    let containers: Vec<&Container> = group.containers.iter().map(|&i| &docker.containers[i]).collect();
    text(buf, row(inner, 0), running_of(group), emphasis(theme));
    let folder = containers.iter().find_map(|c| c.project_dir.clone()).unwrap_or_default();
    text(buf, row(inner, 1), folder, theme.styles.overlay);
    let body = body(rect);
    let (cpu_total, memory_total) = docker.group_usage(group);
    let used = tiles(
        buf,
        body,
        &[
            (tr!(docker_cpu_tile), format!("{}%", cpu(cpu_total))),
            (tr!(docker_memory_tile), format::size(memory_total)),
            (tr!(docker_containers_tile), format!("{}/{}", group.running, containers.len())),
        ],
        theme,
    );
    let rest = Rect { y: body.y + used.min(body.height), height: body.height.saturating_sub(used), ..body };
    let mut y = 0;
    for container in &containers {
        if y >= rest.height {
            break;
        }
        let line = row(rest, y);
        let [name, cpu_cell, memory, status] = columns(line);
        Line::from(vec![
            Span::styled(format!("{} ", dot(container)), tone(container, theme)),
            Span::styled(container.label().to_owned(), theme.styles.overlay),
        ])
        .render(name, buf);
        if let Some(usage) = docker.usage.get(&container.id).filter(|_| container.state.is_up()) {
            text(buf, cpu_cell, cpu(u64::from(usage.latest.cpu)), theme.styles.overlay);
            text(buf, memory, format::size(usage.latest.memory), theme.styles.overlay);
        }
        let note =
            docker.working_on(&container.id).map_or_else(|| container.status.clone(), |a| working_label(a).into());
        text(buf, status, format::truncate_end(&note, usize::from(status.width)), dim(theme));
        y += 1;
    }
    let mut facts = Facts { area: rest, y: y + u16::from(y > 0), label: LABEL_WIDTH, theme };
    let files = containers.iter().find(|c| !c.config_files.is_empty()).map(|c| c.config_files.join(", "));
    facts.add(buf, tr!(docker_files), files.unwrap_or_default());
    let usages: Vec<_> = containers.iter().filter_map(|c| docker.usage.get(&c.id)).collect();
    let cpu_history = summed(&usages.iter().map(|u| &u.cpu[..]).collect::<Vec<_>>());
    let memory_history = summed(&usages.iter().map(|u| &u.memory[..]).collect::<Vec<_>>());
    let left = Rect { y: rest.y + facts.y + 1, height: rest.height.saturating_sub(facts.y + 1), ..rest };
    if group.running > 0 {
        charts(buf, left, usage(&cpu_history, &memory_history), look);
    }
}
