use borg_i18n::tr;
use std::path::Path;

use borg_core::{Content, Image, Viewer};
use borg_gfx::{Placement, draw_halfblocks};
use borg_view::{Format, Pager};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    text::Line,
    widgets::Widget,
};

use unicode_width::UnicodeWidthStr;

use crate::{Theme, format};

const PICTURE_LAYER: i32 = 0;
/// The columns the file's name keeps in the status line before key hints give way.
const TITLE_MIN: usize = 12;
/// The blank columns between where the viewer stands and the key hints.
const GAP: usize = 3;

/// Draws the viewer, returning the pictures to place and where the find bar's cursor belongs.
pub fn draw_viewer(
    viewer: &Viewer,
    theme: &Theme,
    pixels: bool,
    area: Rect,
    buf: &mut Buffer,
) -> (Vec<Placement>, Option<Position>) {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    buf.set_style(body, theme.styles.viewer);
    buf.set_style(status, theme.styles.viewer_status);
    let status = crate::branding::content(status);
    match &viewer.content {
        Content::Text(pager) => {
            crate::document_view::draw(pager, theme.styles.viewer, crate::find_view::match_styles(theme), body, buf);
            if viewer.find.open {
                let cursor = crate::find_view::draw_bar(&viewer.find, theme, status, buf);
                return (Vec::new(), Some(cursor).filter(|cursor| status.contains(*cursor)));
            }
            draw_text(viewer, pager, theme, status, buf);
            (Vec::new(), None)
        }
        Content::Image(image) => (draw_image(image, &viewer.path, theme, pixels, body, status, buf), None),
    }
}

/// The key a click on the viewer's status line presses: on the find bar, a switch or a key.
pub(crate) fn click(viewer: &Viewer, area: Rect, point: Position) -> Option<crossterm::event::KeyEvent> {
    let [_, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let status = crate::branding::content(status);
    if viewer.find.open && status.contains(point) {
        return crate::find_view::click(&viewer.find, status, point);
    }
    None
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
    let room = usize::from(status.width)
        .saturating_sub(details.chars().count() + tr!(previous_next_esc_close).chars().count() + 1);
    Line::styled(format!(" {}{details}", title(path, room)), style).render(status, buf);
    Line::styled(tr!(previous_next_esc_close), style).right_aligned().render(status, buf);
    if pixels {
        return vec![Placement::fitted(image.picture.clone(), body, PICTURE_LAYER)];
    }
    draw_halfblocks(&image.picture, body, buf);
    Vec::new()
}

fn draw_text(viewer: &Viewer, pager: &Pager, theme: &Theme, status: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    let path = &viewer.path;
    // After a step with the bar closed, where the match stands takes the keys' place.
    let found = borg_core::find_bar_summary(&viewer.find);
    let hints = match &found {
        Some(found) => format!("{found} · {} ", tr!(viewer_find_keys)),
        None => match pager.format() {
            Some(Format::Markdown) => tr!(viewer_markdown_keys).to_owned(),
            Some(Format::Json) => tr!(viewer_json_keys).to_owned(),
            None => tr!(viewer_keys).to_owned(),
        },
    };

    let mode = if pager.wrapped() { tr!(wrap) } else { tr!(nowrap) };
    let presentation = match pager.format() {
        Some(_) if !pager.formatted() => tr!(source),
        Some(Format::Markdown) => tr!(rendered),
        Some(Format::Json) => tr!(pretty),
        None => "",
    };
    let position = format!(
        " {}/{} · {}% · {mode}{presentation}{}",
        format::size(pager.offset()),
        format::size(pager.len()),
        pager.percent(),
        if pager.wrapped() { String::new() } else { tr!(col, pager.column()) },
    );
    let width = usize::from(status.width);
    let hints = fit(&hints, width.saturating_sub(position.width() + TITLE_MIN + GAP));
    let room = width.saturating_sub(position.width() + hints.width() + GAP);
    Line::styled(format!(" {}{position}", title(path, room)), styles.viewer_status).render(status, buf);
    Line::styled(hints, styles.viewer_status).right_aligned().render(status, buf);
}

/// The parts of `hints` that fit in `room` columns: those between the first and the last go
/// first, then the last, then all.
fn fit(hints: &str, room: usize) -> String {
    let mut parts: Vec<&str> = hints.split(" · ").collect();
    while parts.join(" · ").width() > room {
        match parts.len() {
            0 => break,
            1 => return String::new(),
            2 => drop(parts.pop()),
            _ => drop(parts.remove(1)),
        }
    }
    let shown = parts.join(" · ");
    // A last part that went keeps the space the hints end with.
    if hints.ends_with(' ') && !shown.ends_with(' ') { format!("{shown} ") } else { shown }
}

fn title(path: &Path, room: usize) -> String {
    format::truncate_start(&path.display().to_string(), room)
}
