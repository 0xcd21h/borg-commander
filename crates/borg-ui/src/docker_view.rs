//! The Docker view: Compose projects and containers in one pane, the selected one's
//! details and live resource use in the other. It shares the activity monitor's look.

use crate::{
    Theme,
    activity_view::{
        FOOTER_ROWS, TILE_WIDTH, Unit, body, chart, dim, emphasis, heading, inner, readable, row, text, wrap,
    },
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{AppState, Docker, DockerGroup, DockerRow, FileOp, Msg, Pointer};
use borg_docker::{Action, Container, Health, State};
use borg_i18n::tr;
use borg_term::Screen;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use jiff::{Timestamp, Zoned};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

const CPU_WIDTH: u16 = 7;
const MEMORY_WIDTH: u16 = 8;
const STATUS_WIDTH: u16 = 26;
/// Panes narrower than this leave out the status column.
const STATUS_FROM: u16 = 56;
const LABEL_WIDTH: u16 = 10;
const CHART_MIN_ROWS: u16 = 6;
const SIDE_BY_SIDE_FROM: u16 = 60;
/// Lines a wheel notch scrolls a shell's history.
const SHELL_WHEEL: isize = 3;
const MOUNTS_SHOWN: usize = 2;
const UP: char = '●';
/// Marks a container whose shell is open.
const SHELL_MARK: &str = "›_";
const DOWN: char = '○';

/// The row in the list pane's frame where the column header sits.
const HEADER_ROW: u16 = 2;
const FILTER_ROW: u16 = 1;

/// Name, CPU, memory and status cells of a list row.
fn columns(area: Rect) -> [Rect; 4] {
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
fn tone(container: &Container, theme: &Theme) -> Style {
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

fn dot(container: &Container) -> char {
    if container.state.is_up() { UP } else { DOWN }
}

fn working_label(action: Action) -> &'static str {
    match action {
        Action::Start => tr!(docker_starting),
        Action::Stop => tr!(docker_stopping),
        Action::Restart => tr!(docker_restarting),
        Action::Pause => tr!(docker_pausing),
        Action::Unpause => tr!(docker_resuming),
    }
}

fn cpu(value: u64) -> String {
    format!("{:.1}", value as f64 / 100.0)
}

fn project_name(group: &DockerGroup) -> String {
    group.project.clone().unwrap_or_else(|| tr!(docker_standalone).into())
}

fn running_of(group: &DockerGroup) -> String {
    format!(tr!(docker_running_of), running = group.running, total = group.containers.len())
}

/// Docker's times in the clock's zone, to the minute.
fn local(time: &str, clock: &Zoned) -> String {
    time.parse::<Timestamp>().map_or_else(
        |_| time.to_owned(),
        |moment| moment.to_zoned(clock.time_zone().clone()).strftime("%Y-%m-%d %H:%M").to_string(),
    )
}

/// Draws the Docker view and returns where the cursor goes while typing in a shell.
pub(crate) fn draw(
    state: &AppState,
    look: &Look<'_>,
    panels: [Rect; 2],
    shell: Option<&Screen<'_>>,
    clock: &Zoned,
    now: std::time::Instant,
    buf: &mut Buffer,
) -> Option<Position> {
    let docker = &state.docker;
    let side = docker.side?;
    let spinner = Spinner::of(&state.busy, borg_core::Task::Docker, now);
    let list = panels[side.index()];
    let details = panels[side.other().index()];
    if !list.is_empty() {
        draw_list(state, look, list, spinner, buf);
    }
    if details.is_empty() { None } else { draw_details(docker, look, details, shell, clock, buf) }
}

/// The details panel's tabs: the overview, and the container's shell.
fn tabs(docker: &Docker, container: &Container, area: Rect) -> Vec<(Rect, &'static str, KeyCode, bool)> {
    let open = docker.shell(&container.id).is_some();
    let shell = if open { tr!(docker_shell_open_tab) } else { tr!(docker_shell_tab) };
    let showing_shell = open && docker.shell_tab;
    let mut x = area.x;
    [(tr!(docker_overview_tab), KeyCode::Char('1'), !showing_shell), (shell, KeyCode::Char('2'), showing_shell)]
        .into_iter()
        .map(|(label, key, active)| {
            let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
            let rect = Rect { x, width, ..area };
            x += width + 1;
            (rect, label, key, active)
        })
        .collect()
}

fn header(container: &Container, docker: &Docker, theme: &Theme, inner: Rect, buf: &mut Buffer) {
    let status = docker.working_on(&container.id).map_or_else(|| container.status.clone(), |a| working_label(a).into());
    Line::from(vec![
        Span::styled(format!("{} ", dot(container)), tone(container, theme)),
        Span::styled(status, emphasis(theme)),
    ])
    .render(row(inner, 0), buf);
    for (rect, label, _, active) in tabs(docker, container, row(inner, 1)) {
        let style = if active { theme.styles.cursor } else { theme.styles.menu };
        buf.set_style(rect, style);
        text(buf, Rect { x: rect.x + 1, width: rect.width.saturating_sub(1), ..rect }, label, style);
    }
}

fn frame(docker: &Docker, look: &Look<'_>, rect: Rect, title: &str, focused: bool, buf: &mut Buffer) -> Rect {
    let theme = look.theme;
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let controls = crate::results_view::controls(rect, docker.expanded);
    let room = controls.first().map_or(rect.width.saturating_sub(2), |(r, _, _)| r.x.saturating_sub(rect.x + 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(if focused { styles.border_active } else { styles.border })
        .title(Line::styled(
            format::truncate_end(&format!(" {title} "), usize::from(room)),
            if focused { styles.title_active } else { styles.title },
        ));
    let inner = block.inner(rect);
    block.render(rect, buf);
    for (r, label, _) in controls {
        text(buf, r, label, styles.menu);
    }
    inner
}

fn draw_list(state: &AppState, look: &Look<'_>, rect: Rect, spinner: Option<Spinner>, buf: &mut Buffer) {
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
        let summary = format!(tr!(docker_running_of), running = running, total = docker.containers.len());
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
                text(buf, name, format!("{fold} {}", project_name(group)), style.add_modifier(Modifier::BOLD));
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
fn filter_row(docker: &Docker, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
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
        let hint = format!(tr!(activity_filter_typing), shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !docker.filter.is_empty() {
        let hint = format!(tr!(activity_filter_applied), shown = shown, total = total);
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

/// Labelled values, one per row: the label dimmed in a fixed column, the value after it.
struct Facts<'a> {
    area: Rect,
    y: u16,
    theme: &'a Theme,
}

impl Facts<'_> {
    fn add(&mut self, buf: &mut Buffer, label: &str, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() || self.y >= self.area.height {
            return;
        }
        let line = row(self.area, self.y);
        text(buf, Rect { width: LABEL_WIDTH.min(line.width), ..line }, label, dim(self.theme));
        let rest =
            Rect { x: line.x + LABEL_WIDTH.min(line.width), width: line.width.saturating_sub(LABEL_WIDTH), ..line };
        text(buf, rest, format::truncate_end(&value, usize::from(rest.width)), self.theme.styles.overlay);
        self.y += 1;
    }
}

/// Labelled values in a row of tiles, as in the activity monitor. Returns the rows used.
fn tiles(buf: &mut Buffer, area: Rect, tiles: &[(&str, String)], theme: &Theme) -> u16 {
    if tiles.is_empty() || area.width < TILE_WIDTH {
        return 0;
    }
    let per_row = (area.width / TILE_WIDTH).clamp(1, tiles.len() as u16);
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

/// CPU and memory charts in what is left of the pane: side by side when wide, stacked when
/// tall, otherwise CPU alone.
fn charts(buf: &mut Buffer, area: Rect, cpu: &[u64], memory: &[u64], look: &Look<'_>) {
    if area.height < CHART_MIN_ROWS {
        return;
    }
    let cpu_title = tr!(docker_cpu_chart);
    let memory_title = tr!(docker_memory_chart);
    if area.width >= SIDE_BY_SIDE_FROM {
        let half = area.width / 2;
        chart(cpu_title, cpu, Unit::Percent, look, Rect { width: half - 1, ..area }, buf);
        chart(
            memory_title,
            memory,
            Unit::Bytes,
            look,
            Rect { x: area.x + half + 1, width: area.width - half - 1, ..area },
            buf,
        );
    } else if area.height > CHART_MIN_ROWS * 2 {
        let half = area.height / 2;
        chart(cpu_title, cpu, Unit::Percent, look, Rect { height: half - 1, ..area }, buf);
        chart(
            memory_title,
            memory,
            Unit::Bytes,
            look,
            Rect { y: area.y + half, height: area.height - half, ..area },
            buf,
        );
    } else {
        chart(cpu_title, cpu, Unit::Percent, look, area, buf);
    }
}

/// The sum of several histories, aligned so that their latest samples line up.
fn summed(histories: &[&[u64]]) -> Vec<u64> {
    let length = histories.iter().map(|h| h.len()).max().unwrap_or(0);
    (0..length)
        .map(|i| histories.iter().filter_map(|history| history.get((i + history.len()).checked_sub(length)?)).sum())
        .collect()
}

fn draw_details(
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

fn container_details(
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
    let mut facts = Facts { area: rest, y: 0, theme };
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
            facts.add(
                buf,
                tr!(docker_exit_code),
                if details.oom_killed { format!(tr!(docker_oom), code = code) } else { code },
            );
        }
        let policy = if details.restart_policy.is_empty() { "no" } else { &details.restart_policy };
        facts.add(
            buf,
            tr!(docker_restarts),
            format!(tr!(docker_restarts_value), policy = policy, count = details.restarts),
        );
    } else {
        facts.add(buf, tr!(docker_command), container.command.clone());
        text(buf, row(rest, facts.y), tr!(docker_reading), dim(theme));
        facts.y += 1;
    }
    facts.add(buf, tr!(docker_created), container.created.get(..16).unwrap_or(&container.created).to_owned());
    let left = Rect { y: rest.y + facts.y + 1, height: rest.height.saturating_sub(facts.y + 1), ..rest };
    match usage {
        Some(usage) => charts(buf, left, &usage.cpu, &usage.memory, look),
        None if !container.state.is_up() => text(buf, row(left, 0), tr!(docker_not_running), dim(theme)),
        None => {}
    }
}

fn group_details(docker: &Docker, group: &DockerGroup, look: &Look<'_>, inner: Rect, rect: Rect, buf: &mut Buffer) {
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
    let mut facts = Facts { area: rest, y: y + u16::from(y > 0), theme };
    let files = containers.iter().find(|c| !c.config_files.is_empty()).map(|c| c.config_files.join(", "));
    facts.add(buf, tr!(docker_files), files.unwrap_or_default());
    let usages: Vec<_> = containers.iter().filter_map(|c| docker.usage.get(&c.id)).collect();
    let cpu_history = summed(&usages.iter().map(|u| &u.cpu[..]).collect::<Vec<_>>());
    let memory_history = summed(&usages.iter().map(|u| &u.memory[..]).collect::<Vec<_>>());
    let left = Rect { y: rest.y + facts.y + 1, height: rest.height.saturating_sub(facts.y + 1), ..rest };
    if group.running > 0 {
        charts(buf, left, &cpu_history, &memory_history, look);
    }
}

fn buttons(docker: &Docker, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
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
pub(crate) fn file_work_label(state: &AppState, work: &borg_core::FileWork) -> String {
    let name = |side: borg_core::Side| state.panel(side).mount.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    match &work.op {
        FileOp::CopyIn { .. } => format!(tr!(docker_copying_in), name = name(work.side)),
        FileOp::CopyOut { .. } => format!(tr!(docker_copying_out), name = name(work.side.other())),
        FileOp::Delete { .. } => tr!(docker_deleting).into(),
        FileOp::Mkdir { .. } => tr!(docker_creating).into(),
        FileOp::Fetch { .. } => tr!(docker_opening).into(),
    }
}

/// The buttons while typing in a shell: back to the list, and maximize.
fn typing_buttons(docker: &Docker, area: Rect) -> Vec<(Rect, &'static str, Pointer)> {
    let mut x = area.x;
    [
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
        let hint = format!(tr!(docker_typing_hint), name = shell.name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histories_add_up_from_their_latest_samples() {
        assert_eq!(summed(&[&[1, 2, 3], &[10, 20]]), [1, 12, 23]);
        assert!(summed(&[]).is_empty());
    }

    #[test]
    fn narrow_lists_drop_the_status_column() {
        let wide = columns(Rect::new(0, 0, 80, 1));
        assert!(wide[3].width > 0 && wide.iter().map(|c| c.width).sum::<u16>() <= 80);
        assert_eq!(columns(Rect::new(0, 0, 40, 1))[3].width, 0);
    }
}
