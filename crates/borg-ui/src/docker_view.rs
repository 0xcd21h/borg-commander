//! The Docker view: Compose projects and containers in one pane, the selected one's
//! details and live resource use in the other. It shares the activity monitor's look.

use crate::{
    Theme,
    activity_view::{emphasis, row, text},
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{AppState, Docker};
use borg_docker::Container;
use borg_i18n::tr;
use borg_term::Screen;
use crossterm::event::KeyCode;
use jiff::{Timestamp, Zoned};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

mod controls;
mod details;
mod list;

pub(crate) use controls::{file_work_label, keybar, panel_mouse, status};
use details::draw_details;
use list::{columns, cpu, dot, draw_list, project_name, running_of, tone, working_label};

const CPU_WIDTH: u16 = 7;
const MEMORY_WIDTH: u16 = 8;
const STATUS_WIDTH: u16 = 26;
/// Panes narrower than this leave out the status column.
const STATUS_FROM: u16 = 56;
const LABEL_WIDTH: u16 = 10;
const CHART_MIN_ROWS: u16 = 6;
const SIDE_BY_SIDE_FROM: u16 = 50;
/// Narrow enough for a container's five tiles to share one row in a half-screen panel,
/// which leaves room for its charts.
const DOCKER_TILE_WIDTH: u16 = 11;
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

#[cfg(test)]
mod tests;
