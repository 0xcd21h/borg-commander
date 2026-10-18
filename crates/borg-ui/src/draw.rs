use std::{path::Path, time::Instant};

use borg_core::{AppState, Layout, Overlay, Side, View, menus};
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::{
    Palette, Theme, ThemeSet, art_view,
    blend::crossfade,
    dialog_view,
    fx::{self, Unfold},
    keybar::draw_keybar,
    look::Look,
    menu_view,
    panel_view::{PanelScene, draw_panel},
    picker_view::draw_picker,
    status_view::draw_status,
    term_view,
    viewer_view::draw_viewer,
};

const FULLY_HIDDEN: f32 = 0.0;
const FULLY_SHOWN: f32 = 1.0;
const CURSOR_HANDOFF: f32 = 0.5;
const BACKDROP: f32 = 0.45;
const MENU_BACKDROP: f32 = 0.25;
const SPLASH_FADE_FROM: f32 = 0.7;
const UNFOLD_FLOOR: f32 = 0.4;

pub struct Scene<'a> {
    pub state: &'a AppState,
    pub screen: Screen<'a>,
    pub palette: &'a Palette,
    pub themes: &'a ThemeSet,
    pub clock: &'a Zoned,
    pub now: Instant,
    pub home: Option<&'a Path>,
}

impl Scene<'_> {
    fn t(&self) -> f32 {
        self.now.saturating_duration_since(self.state.epoch).as_secs_f32()
    }

    fn theme_at(&self, index: usize) -> &Theme {
        self.themes.find(self.state.appearance.choices().get(index).map(|choice| choice.id.as_str()))
    }

    fn look<'b>(&'b self, theme: &'b Theme) -> Look<'b> {
        Look { theme, palette: self.palette, t: self.t() }
    }
}

pub fn draw(frame: &mut Frame<'_>, scene: &Scene<'_>) {
    let area = frame.area();
    let theme = scene.theme_at(scene.state.appearance.current());
    let cursor = compose(frame.buffer_mut(), scene, theme, area);
    if !scene.state.settings.truecolor {
        fx::quantize(frame.buffer_mut(), scene.palette);
    }
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn compose(buf: &mut Buffer, scene: &Scene<'_>, theme: &Theme, area: Rect) -> Option<Position> {
    let state = scene.state;
    if let Some(viewer) = &state.viewer {
        draw_viewer(viewer, theme, area, buf);
        return None;
    }
    if state.screensaver {
        art_view::draw_screensaver(&scene.look(theme), area, buf);
        return None;
    }
    let cursor = draw_main(buf, scene, area);
    match state.splash_progress(scene.now) {
        Some(progress) => {
            splash(buf, scene, theme, progress, area);
            None
        }
        None => cursor,
    }
}

fn splash(buf: &mut Buffer, scene: &Scene<'_>, theme: &Theme, progress: f32, area: Rect) {
    let mut art = Buffer::empty(area);
    art_view::draw_splash(&scene.look(theme), progress, scene.clock, area, &mut art);
    let opacity = ((FULLY_SHOWN - progress) / (FULLY_SHOWN - SPLASH_FADE_FROM)).clamp(FULLY_HIDDEN, FULLY_SHOWN);
    if opacity >= FULLY_SHOWN {
        *buf = art;
    } else {
        let under = buf.clone();
        crossfade(&art, &under, opacity, scene.palette, buf);
    }
}

fn draw_main(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) -> Option<Position> {
    let opacity = scene.state.panel_opacity(scene.now);
    let mut cursor = None;
    if opacity <= FULLY_HIDDEN {
        term_view::draw_screen(&scene.screen, area, buf);
    } else if opacity >= FULLY_SHOWN {
        let (chrome, input) = chrome(scene, area);
        *buf = chrome;
        cursor = input;
    } else {
        let mut terminal = Buffer::empty(area);
        term_view::draw_screen(&scene.screen, area, &mut terminal);
        let (chrome, _) = chrome(scene, area);
        crossfade(&chrome, &terminal, opacity, scene.palette, buf);
    }
    cursor.or_else(|| shell_cursor(scene, opacity))
}

fn chrome(scene: &Scene<'_>, area: Rect) -> (Buffer, Option<Position>) {
    let appearance = &scene.state.appearance;
    let current = scene.theme_at(appearance.current());
    match appearance.transition(scene.now) {
        Some((from, progress)) => {
            let (incoming, cursor) = chrome_with(scene, current, area);
            let (outgoing, _) = chrome_with(scene, scene.theme_at(from), area);
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
    let look = scene.look(theme);
    let layout = state.layout(scene.now);
    let styles = &theme.styles;
    let activity = state.operation.as_ref().filter(|_| state.shell.running).map(|op| (op.side, op.label));

    for side in Side::ALL {
        let active = side == state.active;
        let rect = layout.panels[side.index()];
        let panel = PanelScene {
            panel: state.panel(side),
            active,
            area: rect,
            motion: &state.motions[side.index()],
            activity: activity.filter(|(busy, _)| *busy == side).map(|(_, label)| label),
            delight: state.settings.preferences.delight,
            home: scene.home,
            clock: scene.clock,
            now: scene.now,
        };
        draw_panel(&panel, &look, &mut buf);
        if !active && theme.effects.inactive_dim > FULLY_HIDDEN {
            fx::fade_to_background(&mut buf, rect, theme.effects.inactive_dim, scene.palette);
        }
    }
    if let Some(status) = layout.status {
        draw_status(state.active_panel(), status, styles, scene.clock, &mut buf);
    }

    let (_, cursor_row) = scene.screen.cursor();
    term_view::draw_row(&scene.screen, cursor_row, layout.prompt.y, layout.prompt, &mut buf);
    draw_keybar(layout.keybar, styles, state.notice.as_deref(), &mut buf);

    let cursor = draw_overlay(scene, &look, &layout, &mut buf);
    (buf, cursor)
}

fn draw_overlay(scene: &Scene<'_>, look: &Look<'_>, layout: &Layout, buf: &mut Buffer) -> Option<Position> {
    let state = scene.state;
    let all = menus(state);
    if let Some(bar) = layout.menu {
        menu_view::draw_menu_bar(&all, None, bar, look.theme, buf);
    }
    if let Some(leaving) = state.leaving.as_ref().filter(|leaving| leaving.fade.is_animating(scene.now)) {
        layer(scene, look, layout, buf, &leaving.overlay, leaving.fade.value(scene.now));
    }
    let overlay = state.overlay.as_ref()?;
    let opacity = state.overlay_fade.value(scene.now);
    let cursor = layer(scene, look, layout, buf, overlay, opacity);
    cursor.filter(|_| opacity >= CURSOR_HANDOFF)
}

fn layer(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    buf: &mut Buffer,
    overlay: &Overlay,
    opacity: f32,
) -> Option<Position> {
    if opacity <= FULLY_HIDDEN {
        return None;
    }
    let mut over = buf.clone();
    let cursor = paint_overlay(scene, look, layout, &mut over, overlay, opacity);
    if opacity >= FULLY_SHOWN {
        *buf = over;
    } else {
        let under = buf.clone();
        crossfade(&over, &under, opacity, scene.palette, buf);
    }
    cursor
}

fn paint_overlay(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    buf: &mut Buffer,
    overlay: &Overlay,
    progress: f32,
) -> Option<Position> {
    let state = scene.state;
    let theme = look.theme;
    let screen = layout.screen;
    let all = menus(state);
    let open = match overlay {
        Overlay::Menu(cursor) => Some(*cursor),
        _ => None,
    };
    let bar = layout.menu.or_else(|| open.map(|_| Rect { height: 1, ..screen }));
    let (amount, backdrop) = match overlay {
        Overlay::Menu(_) => {
            let below = bar.map_or(screen.y, |bar| bar.bottom());
            (MENU_BACKDROP, Rect { y: below, height: screen.bottom().saturating_sub(below), ..screen })
        }
        _ => (BACKDROP, screen),
    };
    fx::darken(buf, backdrop, amount, scene.palette);
    if let (Some(bar), Some(cursor)) = (bar, open) {
        menu_view::draw_menu_bar(&all, Some(cursor.menu), bar, theme, buf);
    }
    if matches!(overlay, Overlay::About(_)) && state.raining(scene.now) {
        art_view::draw_rain_backdrop(look, screen, buf);
    }

    let before = (progress < FULLY_SHOWN).then(|| buf.clone());
    let (rect, cursor, unfold) = match overlay {
        Overlay::Menu(cursor) => {
            let x = menu_view::title_positions(&all).get(cursor.menu).copied().unwrap_or_default();
            let top = bar.map_or(screen.y, |b| b.bottom());
            let rect = all
                .get(cursor.menu)
                .map(|menu| menu_view::draw_dropdown(menu, cursor.item, x, top, theme, screen, buf))
                .unwrap_or_default();
            (rect, None, Unfold::Down)
        }
        Overlay::Themes(picker) => {
            (draw_picker(picker, state.appearance.choices(), theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Prompt(prompt) => {
            let (rect, position) = dialog_view::draw_prompt(prompt, theme, screen, buf);
            (rect, Some(position), Unfold::FromCenter)
        }
        Overlay::Confirm(confirm) => (dialog_view::draw_confirm(confirm, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::About(_) => (art_view::draw_about(look, screen, buf), None, Unfold::FromCenter),
    };
    let reach = UNFOLD_FLOOR + (FULLY_SHOWN - UNFOLD_FLOOR) * progress;
    let shown = before.map_or(rect, |before| fx::reveal(buf, &before, rect, reach, unfold));
    fx::shadow(buf, shown, scene.palette);
    cursor
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
