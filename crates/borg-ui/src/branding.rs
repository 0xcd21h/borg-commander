use crate::{Palette, Rgb, Theme, look::Look};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

const CONTRAST: u32 = 140_000;
pub(crate) fn label(width: u16) -> &'static str {
    if usize::from(width) >= tr!(borg_badge).width() + 2 { tr!(borg_badge) } else { tr!(borg_brand_short) }
}
pub(crate) fn content(area: Rect) -> Rect {
    let width = (label(area.width).width() as u16 + 2).min(area.width);
    Rect { width: area.width.saturating_sub(width), ..area }
}
pub(crate) fn draw(area: Rect, theme: &Theme, palette: &Palette, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let width = area.width.saturating_sub(content(area).width);
    let badge = Rect { x: area.right() - width, y: area.bottom() - 1, width, height: 1 };
    let value = if width < 6 { label(area.width).to_owned() } else { format!(" {} ", label(area.width)) };
    let look = Look { theme, palette, t: 0.0 };
    let gradient = look.accent();
    let spans = value
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let bg = gradient.at(i as f32 / f32::from(width.saturating_sub(1).max(1)));
            let luminance = u32::from(bg.r) * 299 + u32::from(bg.g) * 587 + u32::from(bg.b) * 114;
            let fg = if luminance > CONTRAST { Rgb::BLACK } else { Rgb::WHITE };
            Span::styled(c.to_string(), Style::new().fg(fg.into()).bg(bg.into()).add_modifier(Modifier::BOLD))
        })
        .collect::<Vec<_>>();
    Line::from(spans).render(badge, buf);
}
