use std::{path::Path, time::Instant};

use borg_core::{AppState, Layout, Overlay, Side, View, menus};
use borg_gfx::Placement;
use borg_term::Screen;
use jiff::Zoned;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::Widget,
};

use crate::{
    Palette, Theme, ThemeSet, art_view,
    aside_view::{AsideScene, draw_aside},
    blend::crossfade,
    composer_view::draw_composer,
    dialog_view,
    fx::{self, Unfold},
    job_view,
    keybar::draw_keybar,
    look::Look,
    menu_view,
    panel_view::{Activity, PanelScene, draw_panel},
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
}

impl Pixels {
    pub fn pictures(self) -> bool {
        self != Self::Off
    }
}

struct Chrome {
    buf: Buffer,
    cursor: Option<Position>,
    placements: Vec<Placement>,
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
    if let Some(position) = cursor.filter(|position| area.contains(*position)) {
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
    draw_main(buf, scene, area)
}

fn draw_main(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) -> (Option<Position>, Vec<Placement>) {
    let opacity = scene.state.panel_opacity(scene.now);
    let mut cursor = None;
    let mut placements = Vec::new();
    if opacity <= FULLY_HIDDEN {
        draw_console(buf, scene, area);
        cursor = composer_over_shell(buf, scene, area);
    } else if opacity >= FULLY_SHOWN {
        let chrome = chrome(scene, area);
        *buf = chrome.buf;
        cursor = chrome.cursor;
        placements = chrome.placements;
    } else {
        let mut terminal = Buffer::empty(area);
        draw_console(&mut terminal, scene, area);
        let chrome = chrome(scene, area);
        crossfade(&chrome.buf, &terminal, opacity, scene.palette, buf);
    }
    (cursor.or_else(|| shell_cursor(scene, opacity)), placements)
}

fn draw_console(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) {
    let body = Rect { height: area.height.saturating_sub(1), ..area };
    term_view::draw_screen(&scene.screen, body, buf);
    if area.is_empty() {
        return;
    }
    let footer = Rect { y: area.bottom() - 1, height: 1, ..area };
    let theme = scene.theme_at(scene.state.appearance.current());
    let offset = scene.screen.scrollback_offset();
    let history = if offset > 0 { format!(" · ↑{offset} lines · Shift-End live") } else { String::new() };
    let hint = if scene.state.shell.captured() {
        " · program running"
    } else if area.width >= 100 {
        " · Ctrl-O panels · wheel / Shift-PgUp/PgDn history · Shift-drag select"
    } else {
        " · Ctrl-O panels · wheel history"
    };
    buf.set_style(footer, theme.styles.key_label);
    Line::from(vec![
        Span::styled(" Borg console", theme.styles.key),
        Span::styled(history, theme.styles.key_label),
        Span::styled(hint, theme.styles.key_label),
    ])
    .render(footer, buf);
}

fn composer_over_shell(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) -> Option<Position> {
    let Some(Overlay::Composer(composer)) = &scene.state.overlay else { return None };
    let theme = scene.theme_at(scene.state.appearance.current());
    let (_, row) = scene.screen.cursor();
    let anchor = Rect { y: area.y + row, height: 1, ..area };
    let (_, position) = draw_composer(composer, &scene.look(theme), anchor, area, buf);
    Some(position)
}

fn chrome(scene: &Scene<'_>, area: Rect) -> Chrome {
    let appearance = &scene.state.appearance;
    let current = scene.theme_at(appearance.current());
    match appearance.transition(scene.now) {
        Some((from, progress)) => {
            let incoming = chrome_with(scene, current, area);
            let outgoing = chrome_with(scene, scene.theme_at(from), area);
            let mut blended = Buffer::empty(area);
            crossfade(&incoming.buf, &outgoing.buf, progress, scene.palette, &mut blended);
            Chrome { buf: blended, ..incoming }
        }
        None => chrome_with(scene, current, area),
    }
}

fn chrome_with(scene: &Scene<'_>, theme: &Theme, area: Rect) -> Chrome {
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

    let mut placements = Vec::new();
    let finding = match &state.overlay {
        Some(Overlay::Find(find)) => Some((find, borg_core::find_summary(state, find))),
        _ => None,
    };
    let mut panel_cursor = None;
    let aside_side = state.aside.map(|_| state.active.other());
    for side in Side::ALL {
        let active = side == state.active;
        let rect = layout.panels[side.index()];
        let aside = state.aside.filter(|_| aside_side == Some(side));
        if let Some(aside) = aside {
            let shown = AsideScene {
                aside,
                focused: state.preview_focused,
                peeked: state.peeked.as_ref(),
                area: rect,
                icons: state.settings.preferences.nerd_font,
                icon_spacing: state.settings.preferences.icon_spacing,
                pictures: scene.pixels.pictures(),
                clock: scene.clock,
            };
            let pictures = draw_aside(&shown, &look, &mut buf);
            if state.overlay.is_none() {
                placements.extend(pictures);
            }
        }
        let panel = PanelScene {
            panel: state.panel(side),
            active: active && !state.preview_focused,
            area: rect,
            motion: &state.motions[side.index()],
            activity: activities[side.index()].clone(),
            icons: state.settings.preferences.nerd_font,
            icon_spacing: state.settings.preferences.icon_spacing,
            find: finding.filter(|(find, _)| find.side == side),
            home: scene.home,
            clock: scene.clock,
            now: scene.now,
        };
        if aside.is_none() {
            panel_cursor = draw_panel(&panel, &look, &mut buf).or(panel_cursor);
        }
        if !active && aside.is_none() && theme.effects.inactive_dim > FULLY_HIDDEN {
            let busy = activities[side.index()].is_some();
            let dimmed = Rect { height: rect.height.saturating_sub(u16::from(busy)), ..rect };
            fx::fade_to_background(&mut buf, dimmed, theme.effects.inactive_dim, scene.palette);
        }
    }
    if let Some(status) = layout.status {
        draw_status(state.active_panel(), status, styles, scene.clock, &mut buf);
    }

    let top = scene.screen.prompt_top(layout.prompt.height);
    for (offset, y) in (layout.prompt.top()..layout.prompt.bottom()).enumerate() {
        term_view::draw_row(&scene.screen, top.saturating_add(offset as u16), y, layout.prompt, &mut buf);
    }
    draw_keybar(layout.keybar, styles, state.notice.as_deref(), &mut buf);

    let cursor = draw_overlay(scene, &look, &layout, &mut buf);
    Chrome { buf, cursor: cursor.or(panel_cursor), placements }
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
    backdrop(scene, look, layout, overlay, buf);
    layer(scene, look, layout, buf, overlay, opacity).filter(|_| opacity >= CURSOR_HANDOFF)
}

fn layer(
    scene: &Scene<'_>,
    look: &Look<'_>,
    layout: &Layout,
    buf: &mut Buffer,
    overlay: &Overlay,
    opacity: f32,
) -> Option<Position> {
    if opacity <= FULLY_HIDDEN || matches!(overlay, Overlay::Find(_)) {
        return None;
    }
    let mut over = buf.clone();
    let painted = paint_overlay(scene, look, layout, &mut over, overlay, opacity);
    if opacity >= FULLY_SHOWN {
        *buf = over;
    } else {
        let under = buf.clone();
        crossfade(&over, &under, opacity, scene.palette, buf);
    }
    painted
}

fn bar_of(layout: &Layout, overlay: &Overlay) -> Option<Rect> {
    let open = matches!(overlay, Overlay::Menu(_));
    layout.menu.or_else(|| open.then_some(Rect { height: 1, ..layout.screen }))
}

fn backdrop_area(layout: &Layout, overlay: &Overlay) -> Rect {
    let screen = layout.screen;
    match overlay {
        Overlay::Menu(_) => {
            let below = bar_of(layout, overlay).map_or(screen.y, |bar| bar.bottom());
            Rect { y: below, height: screen.bottom().saturating_sub(below), ..screen }
        }
        _ => screen,
    }
}

fn backdrop(scene: &Scene<'_>, look: &Look<'_>, layout: &Layout, overlay: &Overlay, buf: &mut Buffer) {
    let screen = layout.screen;
    let bar = bar_of(layout, overlay);
    let amount = match overlay {
        Overlay::Find(_) | Overlay::Composer(_) => return,
        Overlay::Menu(_) => MENU_BACKDROP,
        _ => BACKDROP,
    };
    fx::fade_to_background(buf, backdrop_area(layout, overlay), amount, scene.palette);
    if let (Some(bar), Overlay::Menu(cursor)) = (bar, overlay) {
        menu_view::draw_menu_bar(&menus(scene.state), Some(cursor.menu), bar, look.theme, buf);
    }
    if matches!(overlay, Overlay::About(_)) && scene.state.raining(scene.now) {
        art_view::draw_rain_backdrop(look, screen, buf);
    }
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
    let bar = bar_of(layout, overlay);

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
        Overlay::Settings { selected } | Overlay::Plugins { selected } => {
            (crate::settings_view::draw(state, *selected, theme, screen, buf), None, Unfold::FromCenter)
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
        Overlay::Composer(composer) => {
            let (rect, position) = draw_composer(composer, look, layout.prompt, screen, buf);
            (rect, Some(position), Unfold::Up)
        }
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
    fx::shadow(buf, shown, scene.palette);
    cursor
}

fn shell_cursor(scene: &Scene<'_>, opacity: f32) -> Option<Position> {
    let state = scene.state;
    if !scene.screen.cursor_visible() || state.overlay.is_some() || state.preview_focused {
        return None;
    }
    let (col, row) = scene.screen.cursor();
    let in_panels = opacity >= CURSOR_HANDOFF && state.effective_view() == View::Panels;
    let row = if in_panels {
        let prompt = state.layout(scene.now).prompt;
        prompt.y + row.saturating_sub(scene.screen.prompt_top(prompt.height))
    } else {
        row
    };
    Some(Position::new(col, row))
}
