use borg_core::KEYMAP;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Clear, Widget},
};

use crate::{
    Rgb,
    dialog_view::centered,
    format,
    fx::{self, Gradient, LOGO},
    keybar::key_name,
    look::Look,
};

const SUBTITLE: &str = "c o m m a n d e r";
const FIRST_CONTACT: &str = "First contact.";
const FIRST_CONTACT_DAY: (i8, i8) = (4, 5);
const TAGLINE: &str = "Resistance is futile.";
const ABOUT_TITLE: &str = " About ";
const ABOUT_HINT: &str = " Esc then 1–9/0 = F1–F10 · Esc close ";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const SPLASH_CUBE: (u16, u16) = (40, 14);
const SAVER_CUBE: (u16, u16) = (56, 22);
const ABOUT_CUBE: (u16, u16) = (18, 8);
const ABOUT_SIZE: (u16, u16) = (62, 19);
const KEY_COLUMNS: usize = 4;
const KEY_SLOT: usize = 13;
const SPLASH_SPIN: f32 = 1.6;
const SAVER_SPIN: f32 = 0.5;
const FADE_IN_SHARE: f32 = 0.2;
const LOGO_GAP: u16 = 1;
const TEXT_GAP: u16 = 1;
const PADDING: u16 = 2;
const RAIN_DIM: f32 = 0.85;
const RAIN_NIGHT: f32 = 0.6;

fn fill(buf: &mut Buffer, area: Rect, look: &Look<'_>) {
    Clear.render(area, buf);
    buf.set_style(area, look.styles().panel.bg(look.background().into()));
}

fn rows(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

fn sized(area: Rect, (width, height): (u16, u16)) -> (u16, u16) {
    (width.min(area.width), height.min(area.height))
}

fn text(buf: &mut Buffer, area: Rect, y: u16, content: &str, gradient: Gradient) {
    let row = Rect { y, height: 1, ..area };
    if !area.contains((area.x, y).into()) {
        return;
    }
    Line::raw(content).centered().render(row, buf);
    let width = rows(unicode_width::UnicodeWidthStr::width(content)).min(area.width);
    fx::gradient_text(buf, Rect { x: area.x + (area.width - width) / 2, width, ..row }, gradient);
}

fn first_contact(clock: &Zoned) -> bool {
    (clock.month(), clock.day()) == FIRST_CONTACT_DAY
}

pub fn draw_splash(look: &Look<'_>, progress: f32, clock: &Zoned, area: Rect, buf: &mut Buffer) {
    fill(buf, area, look);
    let background = look.background();
    let visibility = (progress / FADE_IN_SHARE).clamp(0.0, 1.0);
    let accent = look.accent().dimmed(background, visibility);

    let (cube_w, cube_h) = sized(area, SPLASH_CUBE);
    let logo_h = rows(LOGO.len());
    let block = cube_h + LOGO_GAP + logo_h + TEXT_GAP * 2 + 2;
    let top = area.y + area.height.saturating_sub(block) / 2;
    let cube = Rect { x: area.x + (area.width - cube_w) / 2, y: top, width: cube_w, height: cube_h };
    fx::draw_cube(buf, cube, look.t * SPLASH_SPIN, accent, background);

    let logo_y = cube.bottom() + LOGO_GAP;
    let logo_x = area.x + area.width.saturating_sub(fx::logo_width()) / 2;
    fx::draw_logo(buf, (logo_x, logo_y), look.t, accent);

    let subtitle_y = logo_y + logo_h + TEXT_GAP;
    let quiet =
        Gradient { from: look.fg(look.styles().meta), to: look.fg(look.styles().meta) }.dimmed(background, visibility);
    text(buf, area, subtitle_y, SUBTITLE, quiet);
    if first_contact(clock) {
        text(buf, area, subtitle_y + 1, FIRST_CONTACT, accent.reversed());
    }
}

pub fn draw_screensaver(look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    fill(buf, area, look);
    let background = look.background();
    fx::draw_stars(buf, area, look.t, background);
    let (width, height) = sized(area, SAVER_CUBE);
    let cube = centered(area, width, height);
    fx::draw_cube(buf, cube, look.t * SAVER_SPIN, look.accent(), background);
}

pub fn draw_rain_backdrop(look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let background = look.background().mix(Rgb::BLACK, RAIN_NIGHT);
    Clear.render(area, buf);
    buf.set_style(area, Style::new().bg(background.into()));
    fx::draw_rain(buf, area, look.t, look.accent().dimmed(background, RAIN_DIM), background);
}

pub fn draw_about(look: &Look<'_>, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = look.styles();
    let (width, height) = sized(screen, ABOUT_SIZE);
    let rect = centered(screen, width, height);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(ABOUT_TITLE, styles.overlay_border).centered())
        .title_bottom(Line::styled(ABOUT_HINT, styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let background = look.bg(styles.overlay);
    let accent = look.accent();

    let (cube_w, cube_h) = sized(inner, ABOUT_CUBE);
    let cube = Rect { x: inner.x + PADDING, y: inner.y + 1, width: cube_w, height: cube_h };
    fx::draw_cube(buf, cube, look.t, accent, background);

    let right =
        Rect { x: cube.right() + PADDING, width: inner.right().saturating_sub(cube.right() + PADDING), ..inner };
    let logo_y = cube.y + 1;
    fx::draw_logo(buf, (right.x, logo_y), look.t, accent);
    let info_y = logo_y + rows(LOGO.len()) + TEXT_GAP;
    let meta = styles.overlay.fg(look.fg(styles.meta).into());
    Line::styled(SUBTITLE, meta).render(Rect { y: info_y, height: 1, ..right }, buf);
    Line::styled(format!("version {VERSION}"), meta).render(Rect { y: info_y + 1, height: 1, ..right }, buf);

    let keys_y = cube.bottom() + TEXT_GAP;
    let keys = Rect { x: inner.x + PADDING, y: keys_y, width: inner.width.saturating_sub(PADDING * 2), height: 1 };
    let reference: Vec<String> = KEYMAP
        .iter()
        .filter_map(|binding| Some(format!("{} {}", key_name(binding.code, binding.modifiers), binding.label?)))
        .collect();
    for (row, chunk) in (0u16..).zip(reference.chunks(KEY_COLUMNS)) {
        let line: String =
            chunk.iter().map(|item| format!("{:<KEY_SLOT$}", format::truncate_end(item, KEY_SLOT))).collect();
        Line::styled(line, styles.overlay).render(Rect { y: keys.y + row, ..keys }, buf);
    }

    let tagline_y = inner.bottom().saturating_sub(2);
    text(buf, inner, tagline_y, TAGLINE, accent);
    rect
}
