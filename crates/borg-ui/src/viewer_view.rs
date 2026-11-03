use borg_i18n::tr;

use borg_core::{Content, ENTROPY_STRIP, Image, Msg, Pointer, Shown, Viewer};
use borg_gfx::{Placement, draw_halfblocks};
use borg_view::{
    Format, Pager,
    entropy::{Band, Entropy, strip_cell, strip_cell_of},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
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
/// The bars the entropy strip draws, from none to the most bits a byte carries. The longest
/// leaves an eighth of its cell, where the page's place still shows.
const BARS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const BYTE_BITS: f64 = 8.0;
/// What a cell of the strip shows before the entropy of its bytes was read.
const UNREAD: char = '·';
/// Colours high entropy shows in, the first that reads well on the viewer's background.
const HIGH_COLORS: [Color; 2] = [Color::Red, Color::LightRed];

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
            crate::document_view::draw(
                pager,
                theme.styles.viewer,
                crate::find_view::match_styles(&theme.styles),
                body,
                buf,
            );
            if pager.hex() {
                draw_strip(viewer, pager, theme, strip_area(body), buf);
            }
            if viewer.find.open {
                let cursor = crate::find_view::draw_bar(&viewer.find, theme, status, buf);
                return (Vec::new(), Some(cursor).filter(|cursor| status.contains(*cursor)));
            }
            draw_text(viewer, pager, theme, status, buf);
            (Vec::new(), None)
        }
        Content::Image(image) => (draw_image(image, &name(viewer), theme, pixels, body, status, buf), None),
    }
}

/// What a click on the viewer does: on the find bar it presses a switch or a key, and on the
/// entropy strip it scrolls the hex rows to where its cell starts.
pub(crate) fn click(viewer: &Viewer, area: Rect, point: Position) -> Option<Msg> {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let status = crate::branding::content(status);
    if viewer.find.open && status.contains(point) {
        return crate::find_view::click(&viewer.find, status, point).map(Msg::Key);
    }
    let strip = strip_area(body);
    let hex = matches!(&viewer.content, Content::Text(pager) if pager.hex());
    (hex && strip.contains(point)).then(|| {
        let (cell, cells) = (usize::from(point.y - strip.y), usize::from(strip.height));
        Msg::Pointer(Pointer::EntropyStrip { cell, cells })
    })
}

/// Where the entropy strip goes: the columns at the right of the page, which hex rows leave.
fn strip_area(body: Rect) -> Rect {
    let width = ENTROPY_STRIP.min(body.width);
    Rect { x: body.right() - width, width, ..body }
}

/// The entropy strip beside the hex rows: a cell for each stretch of the source, its bar as long
/// as the highest entropy in it and coloured by what that usually means, and the cells of the
/// bytes on the page marked as on a scrollbar.
fn draw_strip(viewer: &Viewer, pager: &Pager, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let (len, cells) = (pager.len(), usize::from(area.height));
    let shown = pager.shown();
    let page = strip_cell_of(len, cells, shown.start)..=strip_cell_of(len, cells, shown.end.saturating_sub(1));
    for (index, y) in (area.top()..area.bottom()).enumerate() {
        let entropy = viewer.profile().and_then(|profile| profile.highest(strip_cell(len, cells, index)));
        let (glyph, style) = match entropy {
            Some(entropy) => (bar(entropy), band_style(entropy.band(), theme.styles.viewer)),
            None => (UNREAD, theme.styles.viewer.add_modifier(Modifier::DIM)),
        };
        let style = if page.contains(&index) { theme.styles.cursor } else { style };
        if let Some(cell) = buf.cell_mut(Position { x: area.x, y }) {
            cell.set_char(glyph).set_style(style);
        }
    }
}

fn bar(entropy: Entropy) -> char {
    let level = (entropy.bits() / BYTE_BITS * (BARS.len() - 1) as f64).round() as usize;
    BARS[level.min(BARS.len() - 1)]
}

/// How the strip draws an entropy in `band` on the viewer's `base`: low faint, high bold in red.
fn band_style(band: Band, base: Style) -> Style {
    match band {
        Band::Low => base.add_modifier(Modifier::DIM),
        Band::Typical => base,
        Band::High => {
            let red = HIGH_COLORS.into_iter().find_map(|color| crate::activity_view::readable_on(base, Some(color)));
            red.map_or(base, |color| base.fg(color)).add_modifier(Modifier::BOLD)
        }
    }
}

/// What the status line says of the entropy of the bytes on the page, once it was read.
fn entropy_status(viewer: &Viewer, pager: &Pager) -> Option<String> {
    let entropy = viewer.profile()?.highest(pager.shown())?;
    let band = match entropy.band() {
        Band::Low => tr!(entropy_low),
        Band::Typical => tr!(entropy_typical),
        Band::High => tr!(entropy_high),
    };
    Some(tr!(entropy_status, bits = format::bits(entropy), band = band))
}

fn draw_image(
    image: &Image,
    name: &str,
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
    Line::styled(format!(" {}{details}", title(name, room)), style).render(status, buf);
    Line::styled(tr!(previous_next_esc_close), style).right_aligned().render(status, buf);
    if pixels {
        return vec![Placement::fitted(image.picture.clone(), body, PICTURE_LAYER)];
    }
    draw_halfblocks(&image.picture, body, buf);
    Vec::new()
}

fn draw_text(viewer: &Viewer, pager: &Pager, theme: &Theme, status: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    let name = name(viewer);
    // After a step with the bar closed, where the match stands takes the keys' place.
    let found = borg_core::find_bar_summary(&viewer.find);
    let hints = match &found {
        Some(found) => format!("{found} · {} ", tr!(viewer_find_keys)),
        None if pager.hex() => tr!(viewer_hex_keys).to_owned(),
        None => match pager.format() {
            Some(Format::Markdown) => tr!(viewer_markdown_keys).to_owned(),
            Some(Format::Json) => tr!(viewer_json_keys).to_owned(),
            None => tr!(viewer_keys).to_owned(),
        },
    };

    let entropy = entropy_status(viewer, pager).filter(|_| pager.hex()).map(|entropy| format!(" · {entropy}"));
    let position = format!(
        " {}/{} · {}% · {}{}",
        format::size(pager.offset()),
        format::size(pager.len()),
        pager.percent(),
        mode(pager),
        entropy.unwrap_or_default(),
    );
    let width = usize::from(status.width);
    let hints = fit(&hints, width.saturating_sub(position.width() + TITLE_MIN + GAP));
    let room = width.saturating_sub(position.width() + hints.width() + GAP);
    Line::styled(format!(" {}{position}", title(&name, room)), styles.viewer_status).render(status, buf);
    Line::styled(hints, styles.viewer_status).right_aligned().render(status, buf);
}

/// How the page shows the file: as hex rows, or as text wrapped or not, with how a formatted
/// document shows and the column an unwrapped page starts at.
fn mode(pager: &Pager) -> String {
    if pager.hex() {
        return tr!(hex_rows).to_owned();
    }
    let wrap = if pager.wrapped() { tr!(wrap) } else { tr!(nowrap) };
    let presentation = match pager.format() {
        Some(_) if !pager.formatted() => tr!(source),
        Some(Format::Markdown) => tr!(rendered),
        Some(Format::Json) => tr!(pretty),
        None => "",
    };
    let column = if pager.wrapped() { String::new() } else { tr!(col, pager.column()) };
    format!("{wrap}{presentation}{column}")
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

/// What the status line calls what the viewer shows: a file's path, or what the bytes are.
fn name(viewer: &Viewer) -> String {
    match &viewer.shown {
        Shown::File(path) => path.display().to_string(),
        Shown::Bytes(name) => name.clone(),
    }
}

fn title(name: &str, room: usize) -> String {
    format::truncate_start(name, room)
}
