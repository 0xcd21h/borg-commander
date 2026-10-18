use std::path::Path;

use borg_core::Viewer;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Widget,
};

use crate::{Theme, format};

const WRAP_ON: &str = "wrap";
const WRAP_OFF: &str = "nowrap";
const HINTS: &str = "w wrap · ←→ scroll · Esc close ";

pub fn draw_viewer(viewer: &Viewer, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    buf.set_style(body, styles.viewer);
    for (row, text) in viewer.pager.rows().iter().enumerate() {
        let Ok(offset) = u16::try_from(row) else { break };
        Line::styled(text.as_str(), styles.viewer).render(Rect { y: body.y + offset, height: 1, ..body }, buf);
    }

    buf.set_style(status, styles.viewer_status);
    let pager = &viewer.pager;
    let mode = if pager.wrapped() { WRAP_ON } else { WRAP_OFF };
    let position = format!(
        " {}/{} · {}% · {mode}{}",
        format::size(pager.offset()),
        format::size(pager.len()),
        pager.percent(),
        if pager.wrapped() { String::new() } else { format!(" · col {}", pager.column()) },
    );
    let room = usize::from(status.width).saturating_sub(position.len() + HINTS.len());
    Line::styled(format!(" {}{position}", title(&viewer.path, room)), styles.viewer_status).render(status, buf);
    Line::styled(HINTS, styles.viewer_status).right_aligned().render(status, buf);
}

fn title(path: &Path, room: usize) -> String {
    format::truncate_start(&path.display().to_string(), room)
}
