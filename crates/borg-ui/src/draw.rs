use borg_i18n::tr;
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
    dialog_view,
    fx::{self, Unfold},
    job_view,
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
pub(crate) const BACKDROP: f32 = 0.45;
/// How far the host's command line fades while a container's files are active.
const HOST_PROMPT_DIM: f32 = 0.5;
const MENU_BACKDROP: f32 = 0.25;
const UNFOLD_FLOOR: f32 = 0.4;

pub struct Scene<'a> {
    pub state: &'a AppState,
    pub screen: Screen<'a>,
    /// The terminal of the container shell shown in the Docker view, if any.
    pub docker_shell: Option<Screen<'a>>,
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
    crate::branding::draw(area, theme, scene.palette, frame.buffer_mut());
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
        art_view::draw_screensaver(&scene.look(theme), state.saver, area, buf);
        return (None, Vec::new());
    }
    if let Some(start) = state.intro {
        art_view::draw_intro(&scene.look(theme), scene.now.saturating_duration_since(start).as_secs_f32(), area, buf);
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
    let history = if offset > 0 { format!(tr!(lines_shift_end_live), offset = offset) } else { String::new() };
    let hint = if scene.state.shell.captured() {
        tr!(program_running)
    } else if area.width >= 100 {
        tr!(ctrl_o_panels_wheel_shift_pgup_pgdn)
    } else {
        tr!(ctrl_o_panels_wheel_history)
    };
    buf.set_style(footer, theme.styles.key_label);
    // A container's console names the container, so its output is never taken for the host's.
    let label = match scene.state.console_container() {
        Some(name) => Span::styled(format!(" ▣ {name}{} ", tr!(docker_console_label)), theme.styles.cursor),
        None => Span::styled(tr!(borg_console), theme.styles.key),
    };
    Line::from(vec![label, Span::styled(history, theme.styles.key_label), Span::styled(hint, theme.styles.key_label)])
        .render(footer, buf);
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
            if let Some(operation) = state.operation.as_ref().filter(|op| state.shell.running && op.side == side) {
                return Some(Activity { label: format!("{}…", operation.label), fraction: None });
            }
            if let Some(work) = state.docker.files.iter().find(|work| work.side == side) {
                return Some(Activity { label: crate::docker_view::file_work_label(work), fraction: None });
            }
            let scan = state.sizing.as_ref().filter(|scan| scan.side == side)?;
            Some(Activity {
                label: format!(tr!(background_sizes_progress), count = scan.progress.visited),
                fraction: None,
            })
        })
    });

    let mut placements = Vec::new();
    let finding = match &state.overlay {
        Some(Overlay::Find(find)) => Some((find, borg_core::find_summary(state, find))),
        _ => None,
    };
    let mut panel_cursor = None;
    let aside_side = state.aside.map(|_| state.active.other());
    if state.activity.side.is_some() {
        crate::activity_view::draw(state, &look, layout.panels, state.monitor_panels(scene.now), scene.now, &mut buf);
    }
    if state.docker.is_open() {
        panel_cursor = crate::docker_view::draw(
            state,
            &look,
            layout.panels,
            scene.docker_shell.as_ref(),
            scene.clock,
            scene.now,
            &mut buf,
        );
    }
    for side in Side::ALL {
        let active = side == state.active;
        let rect = layout.panels[side.index()];
        if rect.is_empty() || state.activity.side.is_some() || state.docker.is_open() {
            continue;
        }
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
                spinner: crate::spinner::Spinner::of(&state.busy, borg_core::Task::Preview, scene.now),
            };
            let pictures = draw_aside(&shown, &look, &mut buf);
            if state.overlay.is_none() {
                placements.extend(pictures);
            }
        }
        let panel = PanelScene {
            expanded: active && state.results_expanded(),
            panel: state.panel(side),
            active: active && !state.preview_focused,
            area: rect,
            motion: &state.motions[side.index()],
            activity: activities[side.index()].clone(),
            spinner: crate::spinner::Spinner::of(
                &state.busy,
                if activities[side.index()].is_some() {
                    borg_core::Task::Footer(side)
                } else {
                    borg_core::Task::Listing(side)
                },
                scene.now,
            ),
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
        if state.activity.side.is_some() {
            let value = state.activity.process().map_or_else(String::new, |p| {
                format!(
                    tr!(activity_process_label),
                    name = p.name,
                    pid = p.key.pid,
                    executable = p.executable.as_ref().map_or_else(String::new, |p| p.display().to_string())
                )
            });
            Line::styled(value, styles.status).render(status, &mut buf);
        } else if state.docker.is_open() {
            Line::styled(crate::docker_view::status(&state.docker), styles.status).render(status, &mut buf);
        } else {
            draw_status(state.active_panel(), status, styles, scene.clock, &mut buf);
        }
    }

    let top = scene.screen.prompt_top(layout.prompt.height);
    for (offset, y) in (layout.prompt.top()..layout.prompt.bottom()).enumerate() {
        term_view::draw_row(&scene.screen, top.saturating_add(offset as u16), y, layout.prompt, &mut buf);
    }
    // Under a container's files, the host's command line is dimmed and labelled as the host's:
    // it runs nothing there until a host panel is active.
    if !state.console_owns_panel() && !layout.prompt.is_empty() && !state.docker.is_open() {
        fx::fade_to_background(&mut buf, layout.prompt, HOST_PROMPT_DIM, scene.palette);
        let chip = tr!(docker_host_chip);
        let width = (unicode_width::UnicodeWidthStr::width(chip) as u16).min(layout.prompt.width);
        let area = Rect { x: layout.prompt.right() - width, width, height: 1, ..layout.prompt };
        Line::styled(chip, styles.notice.add_modifier(ratatui::style::Modifier::REVERSED)).render(area, &mut buf);
    }
    let collection_label = if state
        .panel(state.active.other())
        .results
        .as_ref()
        .is_some_and(|r| matches!(r.source, borg_core::ResultSource::Collection(_)))
    {
        tr!(collect)
    } else {
        tr!(copy)
    };
    let remove_label = match state.active_panel().results.as_ref().map(|r| &r.source) {
        Some(borg_core::ResultSource::Collection(_)) => tr!(remove),
        Some(_) => tr!(delete),
        None => tr!(delete),
    };
    if state.activity.side.is_some() {
        crate::activity_view::keybar(state, layout.keybar, theme, &mut buf);
    } else if state.docker.is_open() {
        crate::docker_view::keybar(state, layout.keybar, theme, &mut buf);
    } else {
        crate::keybar::draw_keybar(state, layout.keybar, styles, (collection_label, remove_label), scene.now, &mut buf);
    }

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
        Overlay::Find(_) => return,
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
        Overlay::Game2048 => {
            (crate::game2048_view::draw(state.game2048.as_ref(), look, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Collections(picker) => {
            (crate::discovery_view::collections(state, picker, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Search(form) => (crate::discovery_view::search(form, theme, screen, buf), None, Unfold::FromCenter),
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
