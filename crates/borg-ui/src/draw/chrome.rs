//! The chrome under any window: the panels, or a monitor in their place, the status line, the
//! shell's command line and the key bar.

use borg_core::{EditorSlot, Find, Layout, Side};
use borg_gfx::Placement;
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::Widget,
};

use super::{Chrome, FULLY_HIDDEN, Scene, draw_overlay};
use crate::{
    Theme,
    aside_view::{AsideScene, draw_aside},
    blend::crossfade,
    fx, job_view,
    look::Look,
    panel_view::{Activity, PanelScene, draw_panel},
    status_view::draw_status,
    term_view,
};

/// How far the host's command line fades while a container's files are active.
const HOST_PROMPT_DIM: f32 = 0.5;

pub(super) fn chrome(scene: &Scene<'_>, area: Rect) -> Chrome {
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
    let state = scene.state;
    let mut drawing = Drawing {
        scene,
        theme,
        look: scene.look(theme),
        layout: state.layout(scene.now),
        focused: borg_core::focused_editor(state),
        buf: Buffer::empty(area),
        cursor: None,
        placements: Vec::new(),
    };
    drawing.panels();
    drawing.status();
    drawing.editor_dialog();
    drawing.prompt();
    crate::folders_view::draw_picker(state, &drawing.layout, theme, &mut drawing.buf);
    drawing.keybar();
    let cursor = draw_overlay(scene, &drawing.look, &drawing.layout, &mut drawing.buf);
    Chrome { cursor: cursor.or(drawing.cursor), buf: drawing.buf, placements: drawing.placements }
}

/// The chrome being drawn in one theme, with where the cursor goes and the pictures to place.
struct Drawing<'s, 'a> {
    scene: &'s Scene<'a>,
    theme: &'s Theme,
    look: Look<'s>,
    layout: Layout,
    /// The editor that has the keys, if any.
    focused: Option<EditorSlot>,
    buf: Buffer,
    cursor: Option<Position>,
    placements: Vec<Placement>,
}

impl Drawing<'_, '_> {
    /// The two panels, or the monitor drawn across them.
    fn panels(&mut self) {
        let (state, now) = (self.scene.state, self.scene.now);
        if state.activity.side.is_some() {
            crate::activity_view::draw(
                state,
                &self.look,
                self.layout.panels,
                state.monitor_panels(now),
                now,
                &mut self.buf,
            );
        }
        if state.docker.is_open() {
            self.cursor = crate::docker_view::draw(
                state,
                &self.look,
                self.layout.panels,
                self.scene.docker_shell.as_ref(),
                self.scene.clock,
                now,
                &mut self.buf,
            );
        }
        if state.network.is_open() {
            crate::network_view::draw(state, &self.look, self.layout.panels, now, &mut self.buf);
        }
        if state.monitor_open() {
            return;
        }
        let activities = Side::ALL.map(|side| activity(self.scene, side));
        let finding = match &state.overlay {
            Some(borg_core::Overlay::Find(find)) => Some((find, borg_core::find_summary(state, find))),
            _ => None,
        };
        for side in Side::ALL {
            let rect = self.layout.panels[side.index()];
            if rect.is_empty() {
                continue;
            }
            if state.docked[side.index()].is_some() {
                self.docked(side, rect);
            } else {
                self.panel(side, rect, activities[side.index()].clone(), finding);
            }
        }
    }

    /// A panel's own editor, dimmed while the other panel is active.
    fn docked(&mut self, side: Side, rect: Rect) {
        let state = self.scene.state;
        let Some(editor) = &state.docked[side.index()] else { return };
        let active = side == state.active;
        let cursor = crate::editor_view::draw_docked(editor, self.theme, active, rect, &mut self.buf);
        if self.focused == Some(EditorSlot::Panel(side)) && state.overlay.is_none() {
            self.cursor = cursor;
        }
        if !active && self.theme.effects.inactive_dim > FULLY_HIDDEN {
            fx::fade_to_background(&mut self.buf, rect, self.theme.effects.inactive_dim, self.scene.palette);
        }
    }

    /// A panel of files, dimmed while inactive but for the activity in its footer, or the
    /// preview beside the active panel.
    fn panel(&mut self, side: Side, rect: Rect, activity: Option<Activity>, finding: Option<(&Find, (usize, usize))>) {
        let state = self.scene.state;
        if let Some(aside) = state.aside.filter(|_| side == state.active.other()) {
            let shown = AsideScene {
                aside,
                focused: state.preview_focused,
                peeked: state.peeked.as_ref(),
                area: rect,
                icons: state.settings.preferences.nerd_font,
                icon_spacing: state.settings.preferences.icon_spacing,
                pictures: self.scene.pixels.pictures(),
                clock: self.scene.clock,
                spinner: crate::spinner::Spinner::of(&state.busy, borg_core::Task::Preview, self.scene.now),
            };
            let pictures = draw_aside(&shown, &self.look, &mut self.buf);
            if state.overlay.is_none() {
                self.placements.extend(pictures);
            }
            return;
        }
        let active = side == state.active;
        let busy = activity.is_some();
        let panel = PanelScene {
            state,
            expanded: active && state.results_expanded(),
            panel: state.panel(side),
            client_of_remote: state.panel(side).remote_mount.is_none()
                && state.panels.iter().any(|panel| panel.remote_mount.is_some()),
            active: active && !state.preview_focused,
            area: rect,
            motion: &state.motions[side.index()],
            activity,
            spinner: crate::spinner::Spinner::of(
                &state.busy,
                if busy { borg_core::Task::Footer(side) } else { borg_core::Task::Listing(side) },
                self.scene.now,
            ),
            icons: state.settings.preferences.nerd_font,
            icon_spacing: state.settings.preferences.icon_spacing,
            find: finding.filter(|(find, _)| find.side == side),
            home: self.scene.home,
            clock: self.scene.clock,
            now: self.scene.now,
        };
        self.cursor = draw_panel(&panel, &self.look, &mut self.buf).or(self.cursor);
        if !active && self.theme.effects.inactive_dim > FULLY_HIDDEN {
            let dimmed = Rect { height: rect.height.saturating_sub(u16::from(busy)), ..rect };
            fx::fade_to_background(&mut self.buf, dimmed, self.theme.effects.inactive_dim, self.scene.palette);
        }
    }

    /// The status line: the monitored process, the containers, the file a panel's editor has
    /// open, or the active panel's hovered entry.
    fn status(&mut self) {
        let Some(status) = self.layout.status else { return };
        let state = self.scene.state;
        let styles = &self.theme.styles;
        if state.activity.side.is_some() {
            let value = state.activity.process().map_or_else(String::new, |p| {
                tr!(
                    activity_process_label,
                    name = p.name,
                    pid = p.key.pid,
                    executable = p.executable.as_ref().map_or_else(String::new, |p| p.display().to_string())
                )
            });
            Line::styled(value, styles.status).render(status, &mut self.buf);
        } else if state.docker.is_open() {
            Line::styled(crate::docker_view::status(&state.docker), styles.status).render(status, &mut self.buf);
        } else if state.network.is_open() {
            Line::styled(crate::network_view::status(&state.network), styles.status).render(status, &mut self.buf);
        } else if let Some(editor) = state.docked[state.active.index()].as_ref().filter(|_| self.focused.is_some()) {
            // The panel's files are out of sight; the status line names the file being edited.
            Line::styled(format!(" {}", editor.path.display()), styles.status).render(status, &mut self.buf);
        } else {
            draw_status(state.active_panel(), status, styles, self.scene.clock, &mut self.buf);
        }
    }

    /// The dialog of the active panel's editor may reach over the other panel, so it goes on top.
    fn editor_dialog(&mut self) {
        let state = self.scene.state;
        let shown = !state.monitor_open();
        if let Some(editor) = state.docked[state.active.index()].as_ref().filter(|_| shown)
            && let Some(field) = crate::editor_view::draw_dialog(
                editor,
                self.theme,
                self.layout.panels[state.active.index()],
                &mut self.buf,
            )
            && self.focused == Some(EditorSlot::Panel(state.active))
            && state.overlay.is_none()
        {
            self.cursor = field;
        }
    }

    /// The shell's command line. Under a container's or a remote Borg's files it is dimmed and
    /// labelled as the host's: it runs nothing there until a host panel is active.
    fn prompt(&mut self) {
        let (scene, state, prompt) = (self.scene, self.scene.state, self.layout.prompt);
        let top = scene.screen.prompt_top(prompt.height);
        for (offset, y) in (prompt.top()..prompt.bottom()).enumerate() {
            term_view::draw_row(&scene.screen, top.saturating_add(offset as u16), y, prompt, &mut self.buf);
        }
        if state.console_owns_panel() || prompt.is_empty() || state.docker.is_open() {
            return;
        }
        fx::fade_to_background(&mut self.buf, prompt, HOST_PROMPT_DIM, scene.palette);
        let chip =
            if state.active_panel().remote_mount.is_some() { tr!(remote_client_chip) } else { tr!(docker_host_chip) };
        let width = (unicode_width::UnicodeWidthStr::width(chip) as u16).min(prompt.width);
        let area = Rect { x: prompt.right() - width, width, height: 1, ..prompt };
        Line::styled(chip, self.theme.styles.notice.add_modifier(ratatui::style::Modifier::REVERSED))
            .render(area, &mut self.buf);
    }

    /// The key bar of what has the keys: a monitor, an editor or the panels.
    fn keybar(&mut self) {
        let (state, area) = (self.scene.state, self.layout.keybar);
        if state.activity.side.is_some() {
            crate::activity_view::keybar(state, area, self.theme, &mut self.buf);
        } else if state.docker.is_open() {
            crate::docker_view::keybar(state, area, self.theme, &mut self.buf);
        } else if state.network.is_open() {
            crate::network_view::keybar(state, area, self.theme, &mut self.buf);
        } else if self.focused.is_some() {
            crate::editor_view::keybar(state, area, self.theme, &mut self.buf);
        } else {
            let labels = file_labels(state);
            crate::keybar::draw_keybar(state, area, &self.theme.styles, labels, self.scene.now, &mut self.buf);
        }
    }
}

/// What a panel's footer says is under way there: a job, an operation in the shell, the agent's
/// command, a container's file work, or sizes measured in the background.
fn activity(scene: &Scene<'_>, side: Side) -> Option<Activity> {
    let state = scene.state;
    if let Some(job) = state.jobs.iter().rev().find(|job| job.shown_on == side) {
        return Some(job_view::activity(job, scene.now));
    }
    if let Some(operation) = state.operation.as_ref().filter(|op| state.shell.running && op.side == side) {
        return Some(Activity { label: format!("{}…", operation.label), fraction: None });
    }
    if let Some((_, command)) = state.agent.running.as_ref().filter(|_| side == state.active) {
        return Some(Activity { label: tr!(agent_running, command = command), fraction: None });
    }
    if let Some(work) = state.docker.files.iter().find(|work| work.side == side) {
        return Some(Activity { label: crate::docker_view::file_work_label(work), fraction: None });
    }
    let scan = state.sizing.as_ref().filter(|scan| scan.side == side)?;
    Some(Activity { label: tr!(background_sizes_progress, count = scan.progress.visited), fraction: None })
}

/// The key bar's names for F5 and F8: a copy collects into a collection shown in the other
/// panel, and deleting from one only removes it from the collection.
fn file_labels(state: &borg_core::AppState) -> (&'static str, &'static str) {
    let shows_collection = |panel: &borg_core::Panel| {
        matches!(panel.results.as_ref().map(|r| &r.source), Some(borg_core::ResultSource::Collection(_)))
    };
    let copy = if shows_collection(state.panel(state.active.other())) { tr!(collect) } else { tr!(copy) };
    let remove = if shows_collection(state.active_panel()) { tr!(remove) } else { tr!(delete) };
    (copy, remove)
}
