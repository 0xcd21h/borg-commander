use borg_i18n::tr;
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

const WRAP_ON: &str = tr!(wrap);
const WRAP_OFF: &str = tr!(nowrap);
const HINTS: &str = tr!(w_wrap_scroll_esc_close);
const MARKDOWN_HINTS: &str = tr!(s_source_render_w_wrap_esc_close);
const IMAGE_HINTS: &str = tr!(previous_next_esc_close);
const PICTURE_LAYER: i32 = 0;

pub fn draw_viewer(viewer: &Viewer, theme: &Theme, pixels: bool, area: Rect, buf: &mut Buffer) -> Vec<Placement> {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    buf.set_style(body, theme.styles.viewer);
    buf.set_style(status, theme.styles.viewer_status);
    let status = crate::branding::content(status);
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
    crate::document_view::draw(pager, styles.viewer, body, buf);
    let hints = if pager.markdown() { MARKDOWN_HINTS } else { HINTS };

    let mode = if pager.wrapped() { WRAP_ON } else { WRAP_OFF };
    let presentation = if pager.markdown() { if pager.formatted() { tr!(rendered) } else { tr!(source) } } else { "" };
    let position = format!(
        " {}/{} · {}% · {mode}{presentation}{}",
        format::size(pager.offset()),
        format::size(pager.len()),
        pager.percent(),
        if pager.wrapped() { String::new() } else { format!(tr!(col), pager.column()) },
    );
    let room = usize::from(status.width).saturating_sub(position.len() + hints.len());
    Line::styled(format!(" {}{position}", title(path, room)), styles.viewer_status).render(status, buf);
    Line::styled(hints, styles.viewer_status).right_aligned().render(status, buf);
}

fn title(path: &Path, room: usize) -> String {
    format::truncate_start(&path.display().to_string(), room)
}
