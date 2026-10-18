use std::time::Instant;

use borg_core::{AppState, Side, View};
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::{
    Palette, Theme, ThemeSet, blend::crossfade, keybar::draw_keybar, panel_view::draw_panel, picker_view::draw_picker,
    term_view,
};

const FULLY_HIDDEN: f32 = 0.0;
const FULLY_SHOWN: f32 = 1.0;
const CURSOR_HANDOFF: f32 = 0.5;

pub struct Scene<'a> {
    pub state: &'a AppState,
    pub screen: Screen<'a>,
    pub palette: &'a Palette,
    pub themes: &'a ThemeSet,
    pub clock: &'a Zoned,
    pub now: Instant,
}

pub fn draw(frame: &mut Frame<'_>, scene: &Scene<'_>) {
    let area = frame.area();
    let opacity = scene.state.panel_opacity(scene.now);

    if opacity <= FULLY_HIDDEN {
        term_view::draw_screen(&scene.screen, area, frame.buffer_mut());
    } else if opacity >= FULLY_SHOWN {
        *frame.buffer_mut() = chrome(scene, area);
    } else {
        let mut terminal = Buffer::empty(area);
        term_view::draw_screen(&scene.screen, area, &mut terminal);
        crossfade(&chrome(scene, area), &terminal, opacity, scene.palette, frame.buffer_mut());
    }

    if let Some(position) = cursor(scene, opacity) {
        frame.set_cursor_position(position);
    }
}

fn chrome(scene: &Scene<'_>, area: Rect) -> Buffer {
    let appearance = &scene.state.appearance;
    let current = scene.themes.theme(appearance.current());
    match appearance.transition(scene.now) {
        Some((from, progress)) => {
            let incoming = chrome_with(scene, current, area);
            let outgoing = chrome_with(scene, scene.themes.theme(from), area);
            let mut blended = Buffer::empty(area);
            crossfade(&incoming, &outgoing, progress, scene.palette, &mut blended);
            blended
        }
        None => chrome_with(scene, current, area),
    }
}

fn chrome_with(scene: &Scene<'_>, theme: &Theme, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    let state = scene.state;
    let layout = &state.layout;
    let styles = &theme.styles;

    for side in Side::ALL {
        draw_panel(state.panel(side), side == state.active, layout.panels[side.index()], styles, scene.clock, &mut buf);
    }

    let (_, cursor_row) = scene.screen.cursor();
    term_view::draw_row(&scene.screen, cursor_row, layout.prompt.y, layout.prompt, &mut buf);
    draw_keybar(layout.keybar, styles, state.notice.as_deref(), &mut buf);

    if let Some(picker) = &state.picker {
        draw_picker(picker, scene.themes, styles, layout.screen, &mut buf);
    }
    buf
}

fn cursor(scene: &Scene<'_>, opacity: f32) -> Option<Position> {
    if !scene.screen.cursor_visible() || scene.state.picker.is_some() {
        return None;
    }
    let (col, row) = scene.screen.cursor();
    let in_panels = opacity >= CURSOR_HANDOFF && scene.state.effective_view() == View::Panels;
    let row = if in_panels { scene.state.layout.prompt.y } else { row };
    Some(Position::new(col, row))
}
