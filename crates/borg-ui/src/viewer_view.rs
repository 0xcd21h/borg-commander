use std::path::Path;

use borg_core::{Content, Image, Viewer};
use borg_gfx::{Placement, draw_halfblocks};
use borg_view::Pager;
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
const IMAGE_HINTS: &str = "←→ previous/next · Esc close ";
const PICTURE_LAYER: i32 = 0;

pub fn draw_viewer(viewer: &Viewer, theme: &Theme, pixels: bool, area: Rect, buf: &mut Buffer) -> Vec<Placement> {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    buf.set_style(body, theme.styles.viewer);
    buf.set_style(status, theme.styles.viewer_status);
    match &viewer.content {
        Content::Text(pager) => {
            draw_text(pager, &viewer.path, theme, body, status, buf);
            Vec::new()
        }
        Content::Image(image) => draw_image(image, &viewer.path, theme, pixels, body, status, buf),
    }
}

fn draw_image(
    image: &Image,
    path: &Path,
    theme: &Theme,
    pixels: bool,
    body: Rect,
    status: Rect,
    buf: &mut Buffer,
) -> Vec<Placement> {
    let style = theme.styles.viewer_status;
    let (width, height) = image.original;
    let details = format!(" · {width}×{height} · {} · {}", image.format.to_uppercase(), format::size(image.bytes));
    let room = usize::from(status.width).saturating_sub(details.chars().count() + IMAGE_HINTS.chars().count() + 1);
    Line::styled(format!(" {}{details}", title(path, room)), style).render(status, buf);
    Line::styled(IMAGE_HINTS, style).right_aligned().render(status, buf);
    if pixels {
        return vec![Placement::fitted(image.picture.clone(), body, PICTURE_LAYER)];
    }
    draw_halfblocks(&image.picture, body, buf);
    Vec::new()
}

fn draw_text(pager: &Pager, path: &Path, theme: &Theme, body: Rect, status: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    for (row, text) in pager.rows().iter().enumerate() {
        let Ok(offset) = u16::try_from(row) else { break };
        Line::styled(text.as_str(), styles.viewer).render(Rect { y: body.y + offset, height: 1, ..body }, buf);
    }

    let mode = if pager.wrapped() { WRAP_ON } else { WRAP_OFF };
    let position = format!(
        " {}/{} · {}% · {mode}{}",
        format::size(pager.offset()),
        format::size(pager.len()),
        pager.percent(),
        if pager.wrapped() { String::new() } else { format!(" · col {}", pager.column()) },
    );
    let room = usize::from(status.width).saturating_sub(position.len() + HINTS.len());
    Line::styled(format!(" {}{position}", title(path, room)), styles.viewer_status).render(status, buf);
    Line::styled(HINTS, styles.viewer_status).right_aligned().render(status, buf);
}

fn title(path: &Path, room: usize) -> String {
    format::truncate_start(&path.display().to_string(), room)
}
