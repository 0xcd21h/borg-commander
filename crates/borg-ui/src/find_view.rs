//! The find bar in the editor's and the viewer's status line: the query, its switches for case,
//! whole words and regular expressions, which take clicks, and where the match stands.

use borg_core::{FindBar, FindNote, find_bar_summary};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, format, theme::Styles};

/// The narrowest and widest the query's field gets.
const FIELD_MIN: u16 = 12;
const FIELD_MAX: u16 = 40;
/// The switches as VS Code labels them, and the letter that with Alt flips each.
const SWITCHES: [(&str, char); 3] = [("Aa", 'c'), ("ab", 'w'), (".*", 'r')];
const SWITCH_WIDTH: u16 = 4;
/// The room the switches take after a field: a gap and the three of them.
pub(crate) const SWITCHES_WIDTH: u16 = 1 + SWITCHES.len() as u16 * SWITCH_WIDTH;

struct BarLayout {
    label: Rect,
    field: Rect,
    switches: [Rect; 3],
    /// What is left for where the match stands and the keys.
    rest: Rect,
}

fn layout(area: Rect) -> BarLayout {
    let label = Rect { width: (tr!(editor_find).width() as u16 + 2).min(area.width), ..area };
    let field_width = (area.width / 3).clamp(FIELD_MIN, FIELD_MAX).min(area.width.saturating_sub(label.width));
    let field = Rect { x: label.right(), width: field_width, ..area };
    let switches = switch_areas(field, area);
    let start = (field.right() + 1 + SWITCHES_WIDTH).min(area.right());
    BarLayout { label, field, switches, rest: Rect { x: start, width: area.right() - start, ..area } }
}

/// Where the switches stand after `field`, within `area`.
pub(crate) fn switch_areas(field: Rect, area: Rect) -> [Rect; 3] {
    std::array::from_fn(|index| {
        let x = (field.right() + 1 + index as u16 * SWITCH_WIDTH).min(area.right());
        Rect { x, width: SWITCH_WIDTH.min(area.right() - x), ..area }
    })
}

/// Draws the switches, those that are on in `on`.
pub(crate) fn draw_switches(find: &FindBar, areas: [Rect; 3], on: Style, off: Style, buf: &mut Buffer) {
    for ((name, _), (switched, rect)) in SWITCHES.iter().zip([find.case, find.words, find.regex].into_iter().zip(areas))
    {
        Line::styled(format!(" {name} "), if switched { on } else { off }).render(rect, buf);
    }
}

/// The key that flips the switch at `point`, if one is there.
pub(crate) fn switch_key(areas: [Rect; 3], point: Position) -> Option<KeyEvent> {
    let index = areas.iter().position(|rect| rect.contains(point))?;
    Some(KeyEvent::new(KeyCode::Char(SWITCHES[index].1), KeyModifiers::ALT))
}

/// The styles of a match and of the match stepped to, from the theme's marked entries.
pub(crate) fn match_styles(styles: &Styles<Style>) -> (Style, Style) {
    let found = Style { bg: styles.cursor_inactive.bg.or(styles.marked.bg), ..styles.marked };
    (found, styles.marked_cursor)
}

/// Draws the bar over `area`, returning where the field's cursor belongs.
pub(crate) fn draw_bar(find: &FindBar, theme: &Theme, area: Rect, buf: &mut Buffer) -> Position {
    let styles = &theme.styles;
    let base = styles.viewer_status;
    let BarLayout { label, field, switches, rest } = layout(area);
    buf.set_style(area, base);
    Line::styled(format!(" {} ", tr!(editor_find)), base.add_modifier(Modifier::BOLD)).render(label, buf);

    let input = &find.input;
    buf.set_style(field, styles.input);
    let room = usize::from(field.width.saturating_sub(2));
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    let shift = before.width().saturating_sub(room.saturating_sub(1));
    let shown = crate::dialog_view::skip_columns(input.text(), shift);
    Line::styled(format!(" {}", format::truncate_end(&shown, room)), styles.input).render(field, buf);
    let cursor = Position::new(field.x + 1 + (before.width() - shift) as u16, field.y);

    draw_switches(find, switches, styles.cursor, base.add_modifier(Modifier::DIM), buf);

    let summary = find_bar_summary(find).unwrap_or_default();
    let failed = matches!(find.note, Some(FindNote::NoMatch | FindNote::Invalid(_)));
    let style = if failed { base.add_modifier(Modifier::BOLD) } else { base };
    let width = usize::from(rest.width);
    Line::styled(format!(" {}", format::truncate_end(&summary, width.saturating_sub(1))), style).render(rest, buf);
    let keys = tr!(find_keys);
    if summary.width() + keys.width() + 3 <= width {
        Line::styled(keys, base).right_aligned().render(rest, buf);
    }
    cursor
}

/// The key a click at `point` on the bar presses: a switch, or a key named at its end.
pub(crate) fn click(find: &FindBar, area: Rect, point: Position) -> Option<KeyEvent> {
    let BarLayout { switches, rest, .. } = layout(area);
    if let Some(key) = switch_key(switches, point) {
        return Some(key);
    }
    let keys = tr!(find_keys);
    let summary = find_bar_summary(find).unwrap_or_default();
    if !rest.contains(point) || summary.width() + keys.width() + 3 > usize::from(rest.width) {
        return None;
    }
    crate::hints::key_at(keys, rest.right().saturating_sub(keys.width() as u16), point.x)
}
