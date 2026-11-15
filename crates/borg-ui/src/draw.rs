use borg_i18n::tr;
use std::{path::Path, time::Instant};

use borg_core::{AppState, Layout, Overlay, View, menus};
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
    blend::crossfade,
    dialog_view,
    fx::{self, Unfold},
    job_view,
    look::Look,
    menu_view,
    picker_view::draw_picker,
    term_view,
    viewer_view::draw_viewer,
};

const FULLY_HIDDEN: f32 = 0.0;
const FULLY_SHOWN: f32 = 1.0;
const CURSOR_HANDOFF: f32 = 0.5;
pub(crate) const BACKDROP: f32 = 0.45;
const MENU_BACKDROP: f32 = 0.25;
const UNFOLD_FLOOR: f32 = 0.4;

mod chrome;

use chrome::chrome;

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
    draw_with_cursor(frame, scene).0
}

pub fn draw_with_cursor(frame: &mut Frame<'_>, scene: &Scene<'_>) -> (Vec<Placement>, Option<Position>) {
    let (placements, cursor) = draw_into(frame.buffer_mut(), scene);
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
    (placements, cursor)
}

/// Draws the scene over all of `buf`, and says where the terminal's cursor goes in it.
pub fn draw_into(buf: &mut Buffer, scene: &Scene<'_>) -> (Vec<Placement>, Option<Position>) {
    borg_i18n::with_locale(scene.state.settings.preferences.language, || draw_localized(buf, scene))
}

fn draw_localized(buf: &mut Buffer, scene: &Scene<'_>) -> (Vec<Placement>, Option<Position>) {
    let area = buf.area;
    let theme = scene.theme_at(scene.state.appearance.current());
    let (cursor, placements) = compose(buf, scene, theme, area);
    crate::branding::draw(area, theme, scene.palette, buf);
    if !scene.state.settings.truecolor {
        fx::quantize(buf, scene.palette);
    }
    (placements, cursor.filter(|position| area.contains(*position)))
}

fn compose(buf: &mut Buffer, scene: &Scene<'_>, theme: &Theme, area: Rect) -> (Option<Position>, Vec<Placement>) {
    let under = compose_under_approval(buf, scene, theme, area);
    let Some(approval) = &scene.state.approval else {
        crate::toast_view::draw(scene.state, &scene.look(theme), scene.now, buf);
        return under;
    };
    draw_approval_on_top(buf, scene, approval);
    (None, Vec::new())
}

fn compose_under_approval(
    buf: &mut Buffer,
    scene: &Scene<'_>,
    theme: &Theme,
    area: Rect,
) -> (Option<Position>, Vec<Placement>) {
    let state = scene.state;
    if let Some(editor) = &state.editor {
        let cursor = crate::editor_view::draw_editor(editor, theme, area, buf);
        if state.question_shows() {
            return (console_overlay(buf, scene), Vec::new());
        }
        return (cursor, Vec::new());
    }
    if let Some(viewer) = &state.viewer {
        let (placements, cursor) = draw_viewer(viewer, theme, scene.pixels.pictures(), area, buf);
        if state.question_shows() {
            return (console_overlay(buf, scene), Vec::new());
        }
        return (cursor, placements);
    }
    if state.screensaver {
        let elapsed = scene.now.saturating_duration_since(state.saver_started).as_secs_f32();
        art_view::draw_screensaver(&scene.look(theme), state.saver, scene.clock, elapsed, area, buf);
        return (None, Vec::new());
    }
    if let Some(start) = state.intro {
        art_view::draw_intro(&scene.look(theme), scene.now.saturating_duration_since(start).as_secs_f32(), area, buf);
        return (None, Vec::new());
    }
    draw_main(buf, scene, area)
}

fn draw_approval_on_top(buf: &mut Buffer, scene: &Scene<'_>, approval: &borg_core::Approval) {
    let theme = scene.theme_at(scene.state.appearance.current());
    let screen = scene.state.layout(scene.now).screen;
    fx::fade_to_background(buf, screen, BACKDROP, scene.palette);
    let rect = crate::agent_view::draw_approval(approval, theme, screen, buf);
    fx::shadow(buf, rect, scene.palette);
}

fn draw_main(buf: &mut Buffer, scene: &Scene<'_>, area: Rect) -> (Option<Position>, Vec<Placement>) {
    let opacity = scene.state.panel_opacity(scene.now);
    let mut cursor = None;
    let mut placements = Vec::new();
    if opacity <= FULLY_HIDDEN {
        draw_console(buf, scene, area);
        cursor = console_overlay(buf, scene);
    } else if opacity >= FULLY_SHOWN {
        let chrome = chrome(scene, area);
        *buf = chrome.buf;
        cursor = chrome.cursor;
        placements = chrome.placements;
    } else {
        let mut terminal = Buffer::empty(area);
        draw_console(&mut terminal, scene, area);
        console_overlay(&mut terminal, scene);
        let chrome = chrome(scene, area);
        crossfade(&chrome.buf, &terminal, opacity, scene.palette, buf);
    }
    (cursor.or_else(|| shell_cursor(scene, opacity)), placements)
}

/// A dialog over the console while the panels are hidden, such as an agent's approval or a
/// job's question: it shows where the user is looking, since it takes the keys.
fn console_overlay(buf: &mut Buffer, scene: &Scene<'_>) -> Option<Position> {
    let state = scene.state;
    let overlay = state.overlay.as_ref()?;
    let look = scene.look(scene.theme_at(state.appearance.current()));
    let layout = state.layout(scene.now);
    let opacity = state.overlay_fade.value(scene.now);
    backdrop(scene, &look, &layout, overlay, buf);
    layer(scene, &look, &layout, buf, overlay, opacity).filter(|_| opacity >= CURSOR_HANDOFF)
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
    let history = if offset > 0 { tr!(lines_shift_end_live, offset = offset) } else { String::new() };
    let hint = if scene.state.shell.captured() {
        tr!(program_running)
    } else if area.width >= 100 {
        tr!(ctrl_o_panels_wheel_shift_pgup_pgdn)
    } else {
        tr!(ctrl_o_panels_wheel_history)
    };
    buf.set_style(footer, theme.styles.key_label);
    // A container's console names the container, so its output is never taken for the host's.
    let label = if let borg_core::Console::Remote { machine, session } = &scene.state.console {
        let name = scene
            .state
            .panels
            .iter()
            .filter_map(|p| p.remote_mount.as_ref())
            .find(|m| m.machine == *machine && m.session == *session)
            .map(|m| m.name.as_str())
            .unwrap_or(session);
        Span::styled(
            tr!(
                remote_console_label,
                machine = scene.state.remote_host_name(machine),
                session = scene.state.remote_session_name(machine, session, name)
            ),
            theme.styles.cursor,
        )
    } else {
        match scene.state.console_container() {
            Some(name) => Span::styled(format!(" ▣ {name}{} ", tr!(docker_console_label)), theme.styles.cursor),
            None => Span::styled(tr!(borg_console), theme.styles.key),
        }
    };
    Line::from(vec![label, Span::styled(history, theme.styles.key_label), Span::styled(hint, theme.styles.key_label)])
        .render(footer, buf);
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
        Overlay::Menu(_) | Overlay::ContextMenu(_) => MENU_BACKDROP,
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
        Overlay::Folders(dialog) => {
            let (rect, cursor) = crate::folders_view::draw_dialog(state, dialog, theme, screen, buf);
            (rect, Some(cursor), Unfold::FromCenter)
        }
        Overlay::Search(form) => {
            (crate::discovery_view::search(form, state, scene.home, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Exclusions(editor) => {
            (crate::discovery_view::exclusions(editor, state, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Menu(cursor) => {
            let x = menu_view::title_positions(&all).get(cursor.menu).copied().unwrap_or_default();
            let top = bar.map_or(screen.y, |b| b.bottom());
            let rect = all
                .get(cursor.menu)
                .map(|menu| menu_view::draw_dropdown(menu, cursor.item, x, top, theme, screen, buf))
                .unwrap_or_default();
            (rect, None, Unfold::Down)
        }
        Overlay::ContextMenu(context) => {
            (menu_view::draw_context_menu(context, theme, screen, buf), None, Unfold::Down)
        }
        Overlay::Settings { selected } | Overlay::Plugins { selected } => {
            (crate::settings_view::draw(state, *selected, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Languages(picker) => {
            (crate::picker_view::draw_languages(picker, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Themes(picker) => {
            (draw_picker(picker, state.appearance.choices(), theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Prompt(prompt) => {
            let typed = prompt.typed.map(|at| scene.now.saturating_duration_since(at).as_secs_f32());
            let (rect, position) = dialog_view::draw_prompt(prompt, look, typed, screen, buf);
            (rect, Some(position), Unfold::FromCenter)
        }
        Overlay::Confirm(confirm) => (dialog_view::draw_confirm(confirm, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::ProcessControl(dialog) => {
            (crate::activity_view::control::draw(dialog, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::EntropyScan(dialog) => {
            (crate::activity_view::entropy::draw_dialog(dialog, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Capture(dialog) => (
            crate::network_view::draw_capture_dialog(dialog, &state.network, theme, screen, buf),
            None,
            Unfold::FromCenter,
        ),
        Overlay::Install(offer) => (crate::install_view::draw(offer, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::Service(offer) => (crate::service_view::draw(offer, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::Update(offer) => (crate::update_view::draw(offer, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::ScanNetwork(form) => {
            (crate::network_view::draw_scan_form(&state.network, form, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::PortScan(form) => {
            (crate::port_scan_view::draw_form(state, form, theme, scene.clock, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::PortScanDone(record) => {
            (crate::port_scan_view::draw_done(record, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::PortScanResult(view) => {
            (crate::port_scan_view::draw_result(view, theme, scene.clock, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::SaveReport(form) => (crate::report_view::draw(form, theme, screen, buf), None, Unfold::FromCenter),
        Overlay::Storage(window) => {
            (crate::storage_view::draw(state, window, scene.clock, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::AgentLog { scroll } => {
            (crate::agent_view::draw_log(state, *scroll, theme, screen, buf), None, Unfold::FromCenter)
        }
        Overlay::Connect(connect) => {
            let (rect, cursor) = crate::agent_view::draw_connect(state, connect, theme, screen, buf);
            (rect, cursor, Unfold::FromCenter)
        }
        Overlay::Remotes(browser) => {
            (crate::remote_view::draw(browser, theme, screen, buf, scene.t()), None, Unfold::FromCenter)
        }
        Overlay::TimeMachine => {
            (crate::backup_view::draw(state, look, scene.home, scene.now, screen, buf), None, Unfold::FromCenter)
        }
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
            (crate::jobs_sheet::draw(*list, &state.jobs, look, scene.now, layout, buf), None, Unfold::FromCenter)
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
