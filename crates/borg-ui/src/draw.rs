use std::time::Instant;

use borg_core::{AppState, Side, View};
use borg_term::Screen;
use ratatui::{Frame, buffer::Buffer, layout::Position};

use crate::{Palette, Theme, blend::crossfade, format, keybar::draw_keybar, panel_view::draw_panel, term_view};

const FULLY_HIDDEN: f32 = 0.0;
const FULLY_SHOWN: f32 = 1.0;
const CURSOR_HANDOFF: f32 = 0.5;

pub struct Scene<'a> {
    pub state: &'a AppState,
    pub screen: Screen<'a>,
    pub palette: &'a Palette,
    pub theme: &'a Theme,
    pub now: Instant,
}

pub fn draw(frame: &mut Frame<'_>, scene: &Scene<'_>) {
    let area = frame.area();
    let opacity = scene.state.panel_opacity(scene.now);

    if opacity <= FULLY_HIDDEN {
        term_view::draw_screen(&scene.screen, area, frame.buffer_mut());
    } else if opacity >= FULLY_SHOWN {
        draw_panels(scene, frame.buffer_mut());
    } else {
        let mut terminal = Buffer::empty(area);
        let mut panels = Buffer::empty(area);
        term_view::draw_screen(&scene.screen, area, &mut terminal);
        draw_panels(scene, &mut panels);
        crossfade(&panels, &terminal, opacity, scene.palette, frame.buffer_mut());
    }

    if let Some(position) = cursor(scene, opacity) {
        frame.set_cursor_position(position);
    }
}

fn draw_panels(scene: &Scene<'_>, buf: &mut Buffer) {
    let state = scene.state;
    let layout = &state.layout;
    let now = format::now();

    for side in Side::ALL {
        draw_panel(state.panel(side), side == state.active, layout.panels[side.index()], scene.theme, &now, buf);
    }

    let (_, cursor_row) = scene.screen.cursor();
    term_view::draw_row(&scene.screen, cursor_row, layout.prompt.y, layout.prompt, buf);
    draw_keybar(layout.keybar, scene.theme, state.notice.as_deref(), buf);
}

fn cursor(scene: &Scene<'_>, opacity: f32) -> Option<Position> {
    if !scene.screen.cursor_visible() {
        return None;
    }
    let (col, row) = scene.screen.cursor();
    let in_panels = opacity >= CURSOR_HANDOFF && scene.state.effective_view() == View::Panels;
    let row = if in_panels { scene.state.layout.prompt.y } else { row };
    Some(Position::new(col, row))
}
