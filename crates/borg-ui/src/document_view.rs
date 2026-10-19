use borg_view::{Pager, Paint};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::Palette;

pub(crate) fn draw(pager: &Pager, style: Style, body: Rect, buf: &mut Buffer) {
    let background = Palette::default().background_of(style.bg.unwrap_or(Color::Reset));
    let light = u32::from(background.r) + u32::from(background.g) + u32::from(background.b) > 3 * 128;
    for (y, row) in (body.top()..body.bottom()).zip(pager.rich_rows()) {
        let spans = row.iter().map(|run| Span::styled(&run.text, painted(style, run.paint, light))).collect::<Vec<_>>();
        Line::from(spans).render(Rect { y, height: 1, ..body }, buf);
    }
}

fn painted(mut base: Style, paint: Paint, light: bool) -> Style {
    if let Some([r, g, b]) = if light { paint.light } else { paint.dark } {
        base = base.fg(Color::Rgb(r, g, b));
    }
    for (on, modifier) in [
        (paint.bold, Modifier::BOLD),
        (paint.italic, Modifier::ITALIC),
        (paint.underline, Modifier::UNDERLINED),
        (paint.dim, Modifier::DIM),
    ] {
        if on {
            base = base.add_modifier(modifier);
        }
    }
    base
}
