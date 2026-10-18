use std::time::Instant;

use borg_core::{AppState, Layout, Overlay, Side, View, menus};
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::{
    Palette, Theme, ThemeSet, blend::crossfade, dialog_view, keybar::draw_keybar, menu_view, panel_view::draw_panel,
    picker_view::draw_picker, status_view::draw_status, term_view, viewer_view::draw_viewer,
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
    let theme = scene.themes.theme(scene.state.appearance.current());

    if let Some(viewer) = &scene.state.viewer {
        draw_viewer(viewer, theme, area, frame.buffer_mut());
        return;
    }

    let opacity = scene.state.panel_opacity(scene.now);
    let mut cursor = None;
    if opacity <= FULLY_HIDDEN {
        term_view::draw_screen(&scene.screen, area, frame.buffer_mut());
    } else if opacity >= FULLY_SHOWN {
        let (chrome, input) = chrome(scene, area);
        *frame.buffer_mut() = chrome;
        cursor = input;
    } else {
        let mut terminal = Buffer::empty(area);
        term_view::draw_screen(&scene.screen, area, &mut terminal);
        let (chrome, _) = chrome(scene, area);
        crossfade(&chrome, &terminal, opacity, scene.palette, frame.buffer_mut());
    }

    if let Some(position) = cursor.or_else(|| shell_cursor(scene, opacity)) {
        frame.set_cursor_position(position);
    }
}

fn chrome(scene: &Scene<'_>, area: Rect) -> (Buffer, Option<Position>) {
    let appearance = &scene.state.appearance;
    let current = scene.themes.theme(appearance.current());
    match appearance.transition(scene.now) {
        Some((from, progress)) => {
            let (incoming, cursor) = chrome_with(scene, current, area);
            let (outgoing, _) = chrome_with(scene, scene.themes.theme(from), area);
            let mut blended = Buffer::empty(area);
            crossfade(&incoming, &outgoing, progress, scene.palette, &mut blended);
            (blended, cursor)
        }
        None => chrome_with(scene, current, area),
    }
}

fn chrome_with(scene: &Scene<'_>, theme: &Theme, area: Rect) -> (Buffer, Option<Position>) {
    let mut buf = Buffer::empty(area);
    let state = scene.state;
    let layout = state.layout(scene.now);
    let styles = &theme.styles;

    for side in Side::ALL {
        draw_panel(state.panel(side), side == state.active, layout.panels[side.index()], theme, scene.clock, &mut buf);
    }
    if let Some(status) = layout.status {
        draw_status(state.active_panel(), status, styles, scene.clock, &mut buf);
    }

    let (_, cursor_row) = scene.screen.cursor();
    term_view::draw_row(&scene.screen, cursor_row, layout.prompt.y, layout.prompt, &mut buf);
    draw_keybar(layout.keybar, styles, state.notice.as_deref(), &mut buf);

    let cursor = draw_overlay(scene, theme, &layout, &mut buf);
    (buf, cursor)
}

fn draw_overlay(scene: &Scene<'_>, theme: &Theme, layout: &Layout, buf: &mut Buffer) -> Option<Position> {
    let state = scene.state;
    let all = menus(state);
    let open = match &state.overlay {
        Some(Overlay::Menu(cursor)) => Some(*cursor),
        _ => None,
    };
    let bar = layout.menu.or_else(|| open.map(|_| Rect { height: 1, ..layout.screen }));
    if let Some(bar) = bar {
        menu_view::draw_menu_bar(&all, open.map(|c| c.menu), bar, theme, buf);
    }

    match &state.overlay {
        Some(Overlay::Menu(cursor)) => {
            let x = menu_view::title_positions(&all).get(cursor.menu).copied().unwrap_or_default();
            let top = bar.map_or(0, |b| b.y + 1);
            if let Some(menu) = all.get(cursor.menu) {
                menu_view::draw_dropdown(menu, cursor.item, x, top, theme, layout.screen, buf);
            }
            None
        }
        Some(Overlay::Themes(picker)) => {
            draw_picker(picker, scene.themes, theme, layout.screen, buf);
            None
        }
        Some(Overlay::Prompt(prompt)) => Some(dialog_view::draw_prompt(prompt, theme, layout.screen, buf)),
        Some(Overlay::Confirm(confirm)) => {
            dialog_view::draw_confirm(confirm, theme, layout.screen, buf);
            None
        }
        None => None,
    }
}

fn shell_cursor(scene: &Scene<'_>, opacity: f32) -> Option<Position> {
    let state = scene.state;
    if !scene.screen.cursor_visible() || state.overlay.is_some() {
        return None;
    }
    let (col, row) = scene.screen.cursor();
    let in_panels = opacity >= CURSOR_HANDOFF && state.effective_view() == View::Panels;
    let row = if in_panels { state.layout(scene.now).prompt.y } else { row };
    Some(Position::new(col, row))
}
