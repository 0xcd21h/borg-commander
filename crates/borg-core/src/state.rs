use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    Appearance, Aside, Effect, Fade, Job, ListingMode, Panel, PanelMotion, Peeked, Settings, Sort, Viewer,
    layout::{EVEN_SPLIT, clamp_split},
};

mod layout;
mod overlay;
mod pace;
mod shell;

pub use overlay::{Confirm, ConfirmPurpose, MenuCursor, Overlay, Prompt, PromptPurpose};
pub use shell::{Console, ShellStatus, ShellView, View};

const RESULT_PREVIEW_MIN_ROWS: u16 = 12;
const RESULT_PREVIEW_DIVISOR: u16 = 3;
const RESULT_FRAME_ROWS: u16 = 4;
const PANEL_FRAME_ROWS: u16 = 2;
const OPAQUE: f32 = 1.0;
const TRANSPARENT: f32 = 0.0;
pub const REVEAL_DELAY: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];

    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Leaving {
    pub overlay: Overlay,
    pub fade: Fade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub label: &'static str,
    pub side: Side,
    pub reveal_at: Instant,
    pub revealed: bool,
    pub focus: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PanelSetup {
    pub dir: PathBuf,
    pub focus: Option<String>,
    pub mode: ListingMode,
    pub sort: Sort,
}

impl PanelSetup {
    pub fn at(dir: PathBuf) -> Self {
        Self { dir, focus: None, mode: ListingMode::default(), sort: Sort::default() }
    }
}

#[derive(Debug, Clone)]
pub struct Startup {
    pub panels: [PanelSetup; 2],
    pub active: Side,
    pub split: f32,
}

impl Startup {
    pub fn at(dir: PathBuf) -> Self {
        Self { panels: [PanelSetup::at(dir.clone()), PanelSetup::at(dir)], active: Side::Left, split: EVEN_SPLIT }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub activity: crate::Activity,
    pub docker: crate::Docker,
    pub agent: crate::Agent,
    pub backup: crate::Backup,
    /// When background work started, for spinners that wait before appearing.
    pub busy: crate::Busy,
    pub game2048: Option<crate::Game2048>,
    pub collections: Vec<crate::Collection>,
    pub result_history: [Option<Panel>; 2],
    pub last_results: [Option<Panel>; 2],
    pub panels: [Panel; 2],
    pub active: Side,
    pub view: View,
    /// The console's shell: the host's, or a container's while its panel is active.
    pub shell: ShellStatus,
    pub console: Console,
    pub remote_names: crate::RemoteNames,
    pub remote_identities: std::collections::BTreeMap<String, String>,
    pub(crate) remote_consoles: std::collections::HashMap<(String, String), ShellStatus>,
    pub(crate) failed_remote_console: Option<(String, String)>,
    pub(crate) remote_console_failure: Option<String>,
    /// The remote shell the user agreed to close, whose ending is then no error.
    pub(crate) ending_remote: Option<(String, String)>,
    /// The host shell while a container's shell is the console.
    pub(crate) parked_host: Option<ShellStatus>,
    pub fade: Fade,
    pub split: Fade,
    pub cols: u16,
    pub rows: u16,
    pub settings: Settings,
    pub plugins: crate::Plugins,
    pub notice: Option<String>,
    pub appearance: Appearance,
    pub overlay: Option<Overlay>,
    pub operation: Option<Operation>,
    pub sizing: Option<crate::SizeScan>,
    pub(crate) measured: crate::measure::Measured,
    pub next_size: u64,
    pub viewer: Option<Viewer>,
    /// The file the viewer is opening. A load that arrives for another file, or after Esc
    /// gave it up, is dropped.
    pub opening: Option<PathBuf>,
    /// Where the file the viewer or the editor is opening shows a contents search's match.
    pub(crate) landing: Option<crate::viewer::Landing>,
    /// The panel an editor about to open beside results goes to, and what it gives back.
    pub(crate) beside: Option<(Side, crate::editor::Beside)>,
    pub editor: Option<crate::Editor>,
    /// The editors the panels hold in place of their files, left and right.
    pub docked: [Option<crate::Editor>; 2],
    pub epoch: Instant,
    pub motions: [PanelMotion; 2],
    pub overlay_fade: Fade,
    pub leaving: Option<Leaving>,
    pub last_find: Option<String>,
    pub jobs: Vec<Job>,
    pub aside: Option<Aside>,
    pub preview_focused: bool,
    pub(crate) last_menu: Option<MenuCursor>,
    pub(crate) escaped: Option<Instant>,
    pub peeked: Option<Peeked>,
    pub(crate) peeking: Option<(Aside, PathBuf, Option<crate::FirstMatch>)>,
    pub next_job: u64,
    pub idle_since: Instant,
    pub screensaver: bool,
    /// Which screensaver shows; picked again each time it starts.
    pub saver: Saver,
    /// When the startup intro began. Any input, or its end, clears it.
    pub intro: Option<Instant>,
    pub rain_until: Option<Instant>,
    /// What was copied in Borg, for pasting into its own fields.
    pub clipboard: crate::Clipboard,
    pub search_history: crate::SearchHistory,
    /// The hotlist and the folders the panels visited.
    pub folders: crate::Folders,
    pub cd_picker: crate::CdPicker,
    /// Queries looked for in the editor and the viewer, the latest first.
    pub find_recent: Vec<String>,
    /// The id of the viewer's latest search.
    pub(crate) next_find: u64,
    /// A folder just added to the index, whose first build ends with a notice.
    pub indexing: Option<PathBuf>,
    /// When the next flash over an empty folder starts, while one is shown.
    pub(crate) glitch_wake: Option<Instant>,
    generation: u64,
}

impl AppState {
    pub fn new(
        startup: Startup,
        cols: u16,
        rows: u16,
        settings: Settings,
        appearance: Appearance,
        now: Instant,
    ) -> (Self, Vec<Effect>) {
        let active_dir = startup.panels[startup.active.index()].dir.clone();
        let shell = ShellStatus::new(active_dir);
        let panels = startup.panels.clone().map(|setup| Panel::new(setup.dir, 0, setup.mode, setup.sort));

        let mut state = Self {
            activity: crate::Activity::default(),
            docker: crate::Docker::default(),
            agent: crate::Agent::default(),
            backup: crate::Backup::default(),
            console: Console::Host,
            remote_names: Default::default(),
            remote_identities: Default::default(),
            remote_consoles: Default::default(),
            failed_remote_console: None,
            remote_console_failure: None,
            ending_remote: None,
            parked_host: None,
            busy: crate::Busy::default(),
            game2048: None,
            collections: Vec::new(),
            result_history: [None, None],
            last_results: [None, None],
            panels,
            active: startup.active,
            view: View::Panels,
            shell,
            fade: Fade::settled(OPAQUE, now),
            split: Fade::settled(clamp_split(startup.split, cols), now),
            cols,
            rows,
            settings,
            plugins: crate::Plugins::default(),
            notice: None,
            appearance,
            overlay: None,
            operation: None,
            sizing: None,
            measured: Default::default(),
            next_size: 0,
            viewer: None,
            opening: None,
            landing: None,
            beside: None,
            editor: None,
            docked: [None, None],
            epoch: now,
            motions: [PanelMotion::settled(now); 2],
            overlay_fade: Fade::settled(TRANSPARENT, now),
            leaving: None,
            last_find: None,
            jobs: Vec::new(),
            aside: None,
            preview_focused: false,
            last_menu: None,
            escaped: None,
            peeked: None,
            peeking: None,
            next_job: 0,
            idle_since: now,
            screensaver: false,
            saver: Saver::default(),
            intro: None,
            rain_until: None,
            clipboard: crate::Clipboard::default(),
            search_history: crate::SearchHistory::default(),
            folders: crate::Folders::default(),
            cd_picker: crate::CdPicker::default(),
            find_recent: Vec::new(),
            next_find: 0,
            indexing: None,
            glitch_wake: None,
            generation: 0,
        };
        state.refit();

        let effects = Side::ALL
            .into_iter()
            .zip(startup.panels)
            .map(|(side, setup)| state.navigate(side, setup.dir, setup.focus))
            .collect();
        (state, effects)
    }

    pub fn panel(&self, side: Side) -> &Panel {
        &self.panels[side.index()]
    }

    pub fn active_panel(&self) -> &Panel {
        self.panel(self.active)
    }

    pub fn effective_view(&self) -> View {
        if self.shell.captured() { View::Shell } else { self.view }
    }

    /// Whether the active panel shows the console's own files: a host folder for the host's
    /// shell, or the container's filesystem for its shell.
    pub fn console_owns_panel(&self) -> bool {
        if let Some(mount) = &self.active_panel().remote_mount {
            return self.console
                == (Console::Remote { machine: mount.machine.clone(), session: mount.session.clone() });
        }
        match (&self.console, &self.active_panel().mount) {
            (Console::Host, None) => true,
            (Console::Container(id), Some(mount)) => *id == mount.id,
            _ => false,
        }
    }

    /// The container whose shell is the console, by name, for marking the console.
    pub fn remote_host_name<'a>(&'a self, machine: &'a str) -> &'a str {
        self.remote_identities
            .get(machine)
            .and_then(|identity| self.remote_names.0.get(identity))
            .and_then(|labels| labels.host.as_deref())
            .unwrap_or(machine)
    }

    pub fn remote_session_name<'a>(&'a self, machine: &str, session: &str, name: &'a str) -> &'a str {
        self.remote_identities
            .get(machine)
            .and_then(|identity| self.remote_names.0.get(identity))
            .and_then(|labels| labels.sessions.get(session))
            .map(String::as_str)
            .unwrap_or(name)
    }

    pub fn console_container(&self) -> Option<&str> {
        let Console::Container(id) = &self.console else { return None };
        let mount = self.panels.iter().filter_map(|panel| panel.mount.as_ref()).find(|mount| mount.id == *id);
        mount.map(|mount| mount.name.as_str()).or_else(|| self.docker.shell(id).map(|shell| shell.name.as_str()))
    }

    /// Whether Esc was just pressed with nothing to close, so a digit acts as F1–F10.
    pub fn function_prefix(&self, now: Instant) -> bool {
        self.escaped.is_some_and(|at| now.saturating_duration_since(at) <= crate::update::ESCAPE_WINDOW)
    }

    pub(crate) fn receiving_panel(&mut self, side: Side, generation: u64) -> Option<&mut Panel> {
        let index = side.index();
        if self.panels[index].generation() == generation {
            return Some(&mut self.panels[index]);
        }
        if self.last_results[index].as_ref().is_some_and(|p| p.generation() == generation) {
            return self.last_results[index].as_mut();
        }
        self.result_history[index].as_mut().filter(|p| p.generation() == generation)
    }

    pub(crate) fn panel_mut(&mut self, side: Side) -> &mut Panel {
        &mut self.panels[side.index()]
    }

    pub(crate) fn navigate(&mut self, side: Side, dir: PathBuf, focus: Option<String>) -> Effect {
        self.panel_mut(side).cache_observations();
        if self.panel(side).is_results() {
            self.last_results[side.index()] = Some(self.panel(side).clone());
        }
        let generation = self.next_generation();
        self.panel_mut(side).navigate(dir.clone(), generation, focus);
        Effect::List { side, generation, dir }
    }

    pub(crate) fn reload(&mut self, side: Side) -> Effect {
        let hidden = self.settings.preferences.show_hidden;
        let generation = self.next_generation();
        let panel = self.panel_mut(side);
        if let Some(source) = panel.results.as_ref().map(|r| r.source.clone()) {
            panel.begin_results_refresh(generation);
            return match source {
                crate::ResultSource::Collection(_) => {
                    Effect::RefreshCollection { side, generation, entries: panel.entries().to_vec() }
                }
                crate::ResultSource::Search { mut request, .. } => {
                    request.options.refresh = true;
                    if !matches!(request.options.kind, crate::SearchKind::FolderTree | crate::SearchKind::FolderTotals)
                    {
                        request.hidden = hidden;
                    }
                    if let Some(crate::Results {
                        source: crate::ResultSource::Search { request: saved, progress },
                        ..
                    }) = &mut panel.results
                    {
                        *saved = request.clone();
                        *progress = crate::SearchProgress::default();
                    }
                    Effect::Search { side, generation, request }
                }
            };
        }
        panel.reload(generation);
        Effect::List { side, generation, dir: panel.destination().to_path_buf() }
    }

    pub(crate) fn reload_all(&mut self) -> Vec<Effect> {
        Side::ALL.into_iter().map(|side| self.reload(side)).collect()
    }

    pub(crate) fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}

/// How long the startup intro lasts: the logo settles, then holds briefly.
pub const INTRO: std::time::Duration = std::time::Duration::from_millis(1300);

/// The screensavers borg picks from at random.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Saver {
    /// The spinning cube in a starfield.
    #[default]
    Cube,
    /// The glitching logo.
    Glitch,
}
