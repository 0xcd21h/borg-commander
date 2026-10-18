use std::{path::Path, time::Instant};

use borg_core::{AppState, Layout, Overlay, Side, View, menus};
use borg_gfx::{CellSize, Placement};
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Style},
};

use crate::{
    Palette, Theme, ThemeSet, art_view,
    blend::crossfade,
    dialog_view,
    fx::{self, Unfold},
    job_view,
    keybar::draw_keybar,
    look::Look,
    menu_view,
    panel_view::{Activity, PanelScene, draw_panel},
    picker_view::draw_picker,
    status_view::draw_status,
    term_view, underlay,
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
    pub pixels: Pixels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pixels {
    Off,
    Pictures,
    Underlay(CellSize),
}

impl Pixels {
    pub fn pictures(self) -> bool {
        self != Self::Off
    }

    fn underlay(self) -> Option<CellSize> {
        match self {
            Self::Underlay(cell) => Some(cell),
            Self::Off | Self::Pictures => None,
        }
    }
}

struct Chrome {
    buf: Buffer,
    cursor: Option<Position>,
    cards: Vec<Placement>,
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

pub fn draw(frame: &mut Frame<'_>, scene: &Scene<'_>) -> Vec<Placement> {
    let area = frame.area();
    let theme = scene.theme_at(scene.state.appearance.current());
    let (cursor, placements) = compose(frame.buffer_mut(), scene, theme, area);
    if !scene.state.settings.truecolor {
        fx::quantize(frame.buffer_mut(), scene.palette);
    }
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
    placements
}

fn compose(buf: &mut Buffer, scene: &Scene<'_>, theme: &Theme, area: Rect) -> (Option<Position>, Vec<Placement>) {
    let state = scene.state;
    if let Some(viewer) = &state.viewer {
        return (None, draw_viewer(viewer, theme, scene.pixels.pictures(), area, buf));
    }
    if state.screensaver {
        art_view::draw_screensaver(&scene.look(theme), area, buf);
        return (None, Vec::new());
    }
    let (cursor, cards) = draw_main(buf, scene, area);
    match state.splash_progress(scene.now) {
        Some(progress) => {
            splash(buf, scene, theme, progress, area);
            (None, Vec::new())
        }
        None => (cursor, cards),
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

fn draw_main(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) -> (Option<Position>, Vec<Placement>) {
    let opacity = scene.state.panel_opacity(scene.now);
    let mut cursor = None;
    let mut cards = Vec::new();
    if opacity <= FULLY_HIDDEN {
        term_view::draw_screen(&scene.screen, area, buf);
    } else if opacity >= FULLY_SHOWN {
        let chrome = chrome(scene, area, scene.pixels.underlay());
        *buf = chrome.buf;
        cursor = chrome.cursor;
        cards = chrome.cards;
    } else {
        let mut terminal = Buffer::empty(area);
        term_view::draw_screen(&scene.screen, area, &mut terminal);
        let chrome = chrome(scene, area, None);
        crossfade(&chrome.buf, &terminal, opacity, scene.palette, buf);
    }
    (cursor.or_else(|| shell_cursor(scene, opacity)), cards)
}

fn chrome(scene: &Scene<'_>, area: Rect, underlay: Option<CellSize>) -> Chrome {
    let appearance = &scene.state.appearance;
    let current = scene.theme_at(appearance.current());
    match appearance.transition(scene.now) {
        Some((from, progress)) => {
            let incoming = chrome_with(scene, current, area, None);
            let outgoing = chrome_with(scene, scene.theme_at(from), area, None);
            let mut blended = Buffer::empty(area);
            crossfade(&incoming.buf, &outgoing.buf, progress, scene.palette, &mut blended);
            Chrome { buf: blended, ..incoming }
        }
        None => chrome_with(scene, current, area, underlay),
    }
}

fn chrome_with(scene: &Scene<'_>, theme: &Theme, area: Rect, underlay: Option<CellSize>) -> Chrome {
    let mut buf = Buffer::empty(area);
    let state = scene.state;
    let look = scene.look(theme);
    let layout = state.layout(scene.now);
    let styles = &theme.styles;
    let activities = Side::ALL.map(|side| {
        let job = state.jobs.iter().rev().find(|job| job.shown_on == side);
        job.map(|job| job_view::activity(job, scene.now)).or_else(|| {
            let operation = state.operation.as_ref().filter(|op| state.shell.running && op.side == side)?;
            Some(Activity { label: format!("{}…", operation.label), fraction: None })
        })
    });

    let mut cards = Vec::new();
    let backdrop = match &state.overlay {
        Some(Overlay::Find(_)) | None => FULLY_HIDDEN,
        Some(Overlay::Menu(_)) => MENU_BACKDROP,
        Some(_) => BACKDROP,
    };
    let finding = match &state.overlay {
        Some(Overlay::Find(find)) => Some((find, borg_core::find_summary(state, find))),
        _ => None,
    };
    let mut panel_cursor = None;
    for side in Side::ALL {
        let active = side == state.active;
        let rect = layout.panels[side.index()];
        let panel = PanelScene {
            panel: state.panel(side),
            active,
            area: rect,
            motion: &state.motions[side.index()],
            activity: activities[side.index()].clone(),
            delight: state.settings.preferences.delight,
            icons: state.settings.preferences.icons,
            icon_spacing: state.settings.preferences.icon_spacing,
            find: finding.filter(|(find, _)| find.side == side),
            underlay: underlay.is_some(),
            home: scene.home,
            clock: scene.clock,
            now: scene.now,
        };
        panel_cursor = draw_panel(&panel, &look, &mut buf).or(panel_cursor);
        if let Some(cell) = underlay {
            let spec = underlay::PanelCards {
                panel: state.panel(side),
                motion: &state.motions[side.index()],
                area: rect,
                active,
                dim: backdrop,
                now: scene.now,
            };
            cards.extend(underlay::panel(&spec, &look, cell));
        }
        if !active && theme.effects.inactive_dim > FULLY_HIDDEN {
            let busy = activities[side.index()].is_some();
            let dimmed = Rect { height: rect.height.saturating_sub(u16::from(busy)), ..rect };
            fx::fade_to_background(&mut buf, dimmed, theme.effects.inactive_dim, scene.palette);
        }
    }
    if let Some(status) = layout.status {
        draw_status(state.active_panel(), status, styles, scene.clock, &mut buf);
    }

    let (_, cursor_row) = scene.screen.cursor();
    term_view::draw_row(&scene.screen, cursor_row, layout.prompt.y, layout.prompt, &mut buf);
    draw_keybar(layout.keybar, styles, state.notice.as_deref(), &mut buf);

    let (cursor, shown) = draw_overlay(scene, &look, &layout, underlay.is_some(), &mut buf);
    if let Some(cell) = underlay {
        let reset = Color::Reset;
        let pills =
            [styles.cursor, styles.cursor_inactive, styles.marked_cursor].map(|style| style.bg.unwrap_or(reset));
        let panel_base = [styles.panel.bg.unwrap_or(reset)].into_iter().chain(pills).collect::<Vec<_>>();
        let dimmed = backdrop > FULLY_HIDDEN;
        for rect in layout.panels.iter().copied().filter(|_| !dimmed) {
            cards.extend(underlay::highlights(&buf, rect, &panel_base, underlay::Layer::Panel, &look, cell));
        }
        cards.extend(shown.iter().flat_map(|surface| match surface {
            Surface::Card(rect) => {
                let base = [styles.overlay.bg, styles.menu.bg].map(|bg| bg.unwrap_or(reset));
                let mut placed = underlay::overlay(*rect, &look, cell);
                placed.extend(underlay::highlights(&buf, *rect, &base, underlay::Layer::Overlay, &look, cell));
                placed
            }
            Surface::Strip(rect, style) => {
                let mut placed = underlay::strip(*rect, *style, &look, cell);
                let base = [style.bg.unwrap_or(reset)];
                placed.extend(underlay::highlights(&buf, *rect, &base, underlay::Layer::Overlay, &look, cell));
                placed
            }
        }));
    }
    Chrome { buf, cursor: cursor.or(panel_cursor), cards }
}

#[derive(Debug, Clone, Copy)]
enum Surface {
    Card(Rect),
    Strip(Rect, Style),
}

fn draw_overlay(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    underlay: bool,
    buf: &mut Buffer,
) -> (Option<Position>, Vec<Surface>) {
    let state = scene.state;
    let all = menus(state);
    let mut surfaces = Vec::new();
    if let Some(bar) = layout.menu {
        menu_view::draw_menu_bar(&all, None, bar, look.theme, buf);
        surfaces.push(Surface::Strip(bar, look.styles().menu));
    }
    if let Some(leaving) = state.leaving.as_ref().filter(|leaving| leaving.fade.is_animating(scene.now)) {
        layer(scene, look, layout, buf, &leaving.overlay, leaving.fade.value(scene.now), false);
    }
    let Some(overlay) = state.overlay.as_ref() else { return (None, surfaces) };
    let opacity = state.overlay_fade.value(scene.now);
    let solid = underlay && opacity >= CURSOR_HANDOFF;
    let (cursor, shown) = layer(scene, look, layout, buf, overlay, opacity, solid);
    if solid {
        surfaces.extend(shown);
    }
    (cursor.filter(|_| opacity >= CURSOR_HANDOFF), surfaces)
}

fn layer(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    buf: &mut Buffer,
    overlay: &Overlay,
    opacity: f32,
    solid: bool,
) -> (Option<Position>, Vec<Surface>) {
    if opacity <= FULLY_HIDDEN || matches!(overlay, Overlay::Find(_)) {
        return (None, Vec::new());
    }
    let mut over = buf.clone();
    let painted = paint_overlay(scene, look, layout, &mut over, overlay, opacity, solid);
    if opacity >= FULLY_SHOWN {
        *buf = over;
    } else {
        let under = buf.clone();
        crossfade(&over, &under, opacity, scene.palette, buf);
    }
    painted
}

fn paint_overlay(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    buf: &mut Buffer,
    overlay: &Overlay,
    progress: f32,
    solid: bool,
) -> (Option<Position>, Vec<Surface>) {
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
        Overlay::Find(_) => (Rect::default(), None, Unfold::Down),
        Overlay::Question(question) => {
            let rect = state
                .jobs
                .iter()
                .find(|job| job.id == question.id)
                .map(|job| job_view::draw_question(job, *question, look, screen, buf))
                .unwrap_or_default();
            (rect, None, Unfold::FromCenter)
        }
        Overlay::Jobs(list) => {
            (job_view::draw_jobs(*list, &state.jobs, look, scene.now, screen, buf), None, Unfold::FromCenter)
        }
    };
    let reach = UNFOLD_FLOOR + (FULLY_SHOWN - UNFOLD_FLOOR) * progress;
    let shown = before.map_or(rect, |before| fx::reveal(buf, &before, rect, reach, unfold));
    if !solid {
        fx::shadow(buf, shown, scene.palette);
        return (cursor, Vec::new());
    }
    fx::erase_border(buf, shown);
    let strip =
        bar.filter(|_| layout.menu.is_none() && open.is_some()).map(|bar| Surface::Strip(bar, look.styles().menu));
    (cursor, strip.into_iter().chain(std::iter::once(Surface::Card(shown))).collect())
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
