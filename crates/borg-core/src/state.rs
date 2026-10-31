use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use borg_term::{Foreground, PromptState, TermMode};

use crate::{
    About, Appearance, Aside, Easing, Effect, Fade, Find, Job, JobList, Layout, LineInput, ListingMode, Motion, Pace,
    Panel, PanelMotion, Peeked, Picker, Question, Settings, Sort, Viewer,
    layout::{Chrome, EVEN_SPLIT, clamp_split},
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Panels,
    Shell,
}

/// Whose shell the command line and Ctrl-O console belong to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Console {
    #[default]
    Host,
    /// A container's shell, while its filesystem panel is active (Docker command line).
    Container(String),
    Remote {
        machine: String,
        session: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellView {
    pub prompt: PromptState,
    pub command_rows: u16,
    pub foreground: Foreground,
    pub mode: TermMode,
    pub raw_input: bool,
}

impl Default for ShellView {
    fn default() -> Self {
        Self {
            prompt: PromptState::Unknown,
            command_rows: 1,
            foreground: Foreground::Unknown,
            mode: TermMode::empty(),
            raw_input: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellStatus {
    pub observed: ShellView,
    pub cwd: PathBuf,
    pub running: bool,
    pub typed: bool,
    pub unechoed: bool,
    pub integrated: bool,
    /// The shell announced at its latest prompt that it changes folder without a typed
    /// command line.
    pub silent_cd: bool,
    pub went_full_screen: bool,
    pub return_view: View,
}

impl ShellStatus {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            observed: ShellView::default(),
            cwd,
            running: false,
            typed: false,
            unechoed: false,
            integrated: false,
            silent_cd: false,
            went_full_screen: false,
            return_view: View::Panels,
        }
    }

    /// What a shell that is not the console notes from its signals, so it is up to date
    /// when it becomes the console again.
    pub(crate) fn note(&mut self, signal: &borg_term::ShellSignal) {
        use borg_term::ShellSignal;
        match signal {
            ShellSignal::PromptStart => self.integrated = true,
            ShellSignal::CommandStart => self.running = true,
            ShellSignal::CommandEnd { .. } => self.running = false,
            ShellSignal::Cwd { path, .. } => self.cwd = path.clone(),
            _ => {}
        }
    }

    pub fn captured(&self) -> bool {
        self.observed.raw_input || self.observed.mode.contains(TermMode::ALT_SCREEN)
    }

    pub fn busy(&self) -> bool {
        self.running || self.observed.foreground == Foreground::Program
    }

    pub(crate) fn observe(&mut self, view: ShellView) {
        self.observed = view;
        self.went_full_screen |= self.running && self.captured();
    }

    pub(crate) fn mark_typed(&mut self) {
        self.typed = true;
        self.unechoed = true;
    }

    pub(crate) fn clear_line(&mut self) {
        self.typed = false;
        self.unechoed = false;
    }

    pub(crate) fn submit(&mut self) {
        self.clear_line();
        self.running |= self.integrated;
    }

    pub fn line_empty(&self) -> bool {
        match self.observed.prompt {
            PromptState::Empty => !self.unechoed,
            PromptState::Typed => false,
            PromptState::Unknown => !self.typed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuCursor {
    pub menu: usize,
    pub item: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPurpose {
    Search {
        root: PathBuf,
    },
    Collection,
    RemoteName {
        browser: Box<crate::RemoteBrowser>,
        identity: String,
        session: Option<String>,
        original: String,
    },
    ResultsFilter,
    /// A path to exclude from Time Machine, as a fixed path or marked on the item.
    BackupAdd {
        fixed: bool,
    },
    Select,
    Unselect,
    Mkdir,
    Compress {
        sources: Vec<String>,
    },
    Copy {
        sources: Vec<PathBuf>,
        optimize_hdd: bool,
    },
    Move {
        sources: Vec<PathBuf>,
    },
    RemoteCopy {
        sources: Vec<PathBuf>,
        from: Option<crate::RemoteMount>,
        to: Option<crate::RemoteMount>,
    },
    /// Copy between a container's filesystem and a host folder.
    DockerCopy {
        sources: Vec<PathBuf>,
    },
}

impl PromptPurpose {
    /// The checkbox below the field, as its label and whether it is checked.
    pub fn option(&self) -> Option<(&'static str, bool)> {
        match self {
            Self::Copy { optimize_hdd, .. } => Some((borg_i18n::tr!(optimize_copying_for_hard_drives), *optimize_hdd)),
            Self::BackupAdd { fixed } => Some((borg_i18n::tr!(backup_add_fixed), *fixed)),
            _ => None,
        }
    }

    pub(crate) fn toggle_option(&mut self) {
        match self {
            Self::Copy { optimize_hdd: checked, .. } | Self::BackupAdd { fixed: checked } => *checked = !*checked,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: &'static str,
    pub label: String,
    pub input: LineInput,
    pub purpose: PromptPurpose,
    pub option_focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPurpose {
    RemoteSessionDelete {
        machine: String,
        session: String,
        browser: Box<crate::RemoteBrowser>,
    },
    RemoteHostForget {
        machine: String,
        browser: Box<crate::RemoteBrowser>,
    },
    Delete {
        sources: Vec<PathBuf>,
    },
    Quit,
    ShellExit {
        token: String,
    },
    /// `exit` typed in a remote shell: Yes lets that shell end, No keeps it.
    RemoteShellExit {
        token: String,
    },
    Docker {
        action: borg_docker::Action,
        ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: &'static str,
    pub message: String,
    pub purpose: ConfirmPurpose,
    pub accept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Game2048,
    Collections(crate::CollectionPicker),
    Search(crate::SearchForm),
    /// What searches leave out, opened from the search dialog.
    Exclusions(crate::ExclusionEditor),
    About(About),
    Find(Find),
    Question(Question),
    Jobs(JobList),
    Themes(Picker),
    Languages(Picker),
    Settings {
        selected: usize,
    },
    Plugins {
        selected: usize,
    },
    Menu(MenuCursor),
    Prompt(Prompt),
    Confirm(Confirm),
    /// An agent's command, new file or edit, waiting for the user's approval.
    Approval(crate::agent::Approval),
    /// The log of what agents did; `scroll` is `None` while it follows the newest entries.
    AgentLog {
        scroll: Option<usize>,
    },
    /// How to connect Claude to this borg.
    Connect(crate::agent::Connect),
    /// Paired Borg machines and their named sessions.
    Remotes(crate::RemoteBrowser),
    /// Time Machine exclusions; the window's lists live in `AppState::backup`.
    TimeMachine,
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
    pub editor: Option<crate::Editor>,
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
    pub(crate) peeking: Option<(Aside, PathBuf)>,
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
            editor: None,
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

    pub fn chrome(&self) -> Chrome {
        let prefs = &self.settings.preferences;
        Chrome {
            menu_bar: prefs.menu_bar,
            status_line: prefs.status_line,
            prompt_rows: self.shell.observed.command_rows.max(1).min((self.rows / 2).max(1)),
        }
    }

    pub fn results_expanded(&self) -> bool {
        self.active_panel().results.as_ref().is_some_and(|results| results.expanded)
    }

    pub fn layout(&self, now: Instant) -> Layout {
        let mut layout = Layout::new(self.cols, self.rows, self.chrome(), self.split.value(now));
        if let Some(side) = self.activity.side {
            if self.activity.inspecting() {
                layout.panels = inspection_panels(layout.panels);
            }
            if self.activity.expanded {
                let left = layout.panels[0];
                let right = layout.panels[1];
                let focus = if self.activity.preview_focused { side.other() } else { side };
                layout.panels[focus.index()] =
                    ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
                layout.panels[focus.other().index()] = Default::default();
            }
            return layout;
        }
        if let Some(side) = self.docker.side
            && self.docker.expanded
        {
            let left = layout.panels[0];
            let right = layout.panels[1];
            let focus = if self.docker.details_focused { side.other() } else { side };
            layout.panels[focus.index()] =
                ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
            layout.panels[focus.other().index()] = Default::default();
            return layout;
        }
        if self.results_expanded() {
            let left = layout.panels[0];
            let right = layout.panels[1];
            let mut whole = ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
            let other = if self.aside.is_some() && whole.height >= RESULT_PREVIEW_MIN_ROWS {
                let height = whole.height / RESULT_PREVIEW_DIVISOR;
                whole.height -= height;
                ratatui::layout::Rect { y: whole.bottom(), height, ..whole }
            } else {
                ratatui::layout::Rect::default()
            };
            layout.panels[self.active.index()] = whole;
            layout.panels[self.active.other().index()] = other;
        }
        layout
    }

    /// The activity monitor's own panes, which stay beneath the inspection window.
    pub fn monitor_panels(&self, now: Instant) -> [ratatui::layout::Rect; 2] {
        Layout::new(self.cols, self.rows, self.chrome(), self.split.value(now)).panels
    }

    pub fn split_target(&self) -> f32 {
        self.split.target()
    }

    pub fn motion(&self) -> Motion {
        self.settings.effective_motion()
    }

    pub fn effective_view(&self) -> View {
        if self.shell.captured() { View::Shell } else { self.view }
    }

    pub fn panel_opacity(&self, now: Instant) -> f32 {
        self.fade.value(now)
    }

    pub fn pace(&self, now: Instant) -> Option<Pace> {
        let smooth = self.fade.is_animating(now)
            || self.split.is_animating(now)
            || self.appearance.is_animating(now)
            || self.overlay_fade.is_animating(now)
            || self.leaving.as_ref().is_some_and(|leaving| leaving.fade.is_animating(now))
            || self.motions.iter().any(|motion| motion.is_animating(now));
        if smooth {
            Some(Pace::Smooth)
        } else if crate::motion::ambient(self, now) {
            Some(Pace::Ambient)
        } else {
            None
        }
    }

    pub fn raining(&self, now: Instant) -> bool {
        self.rain_until.is_some_and(|until| now < until)
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

    pub fn can_screensave(&self) -> bool {
        !self.shell.busy()
            && self.intro.is_none()
            && self.jobs.is_empty()
            && self.overlay.is_none()
            && self.viewer.is_none()
            && self.activity.side.is_none()
            && !self.docker.is_open()
            && self.sizing.is_none()
    }

    /// Whether Esc was just pressed with nothing to close, so a digit acts as F1–F10.
    pub fn function_prefix(&self, now: Instant) -> bool {
        self.escaped.is_some_and(|at| now.saturating_duration_since(at) <= crate::update::ESCAPE_WINDOW)
    }

    pub fn wake_at(&self) -> Option<Instant> {
        let reveal = self.operation.as_ref().filter(|op| !op.revealed).map(|op| op.reveal_at);
        let prefix = self.escaped.map(|at| at + crate::update::ESCAPE_WINDOW);
        let intro = self.intro.map(|start| start + INTRO);
        let rest = (self.settings.preferences.screensaver && !self.screensaver && self.can_screensave())
            .then_some(self.idle_since + self.settings.screensaver_after);
        let rain = self.rain_until;
        [
            reveal,
            rest,
            rain,
            prefix,
            intro,
            self.busy.reveal_at(),
            self.glitch_wake,
            crate::plugins::wake_at(self),
            crate::activity::wake_at(self),
            crate::docker::wake_at(self),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn settle(&mut self) {
        self.panels.iter_mut().for_each(Panel::settle);
        self.last_results.iter_mut().flatten().for_each(Panel::settle);
        self.result_history.iter_mut().flatten().for_each(Panel::settle);
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

    pub(crate) fn sync_overlay(&mut self, previous: Option<Overlay>, now: Instant) {
        let kind = |overlay: Option<&Overlay>| overlay.map(std::mem::discriminant);
        if self.leaving.as_ref().is_some_and(|leaving| !leaving.fade.is_animating(now)) {
            self.leaving = None;
        }
        if kind(previous.as_ref()) == kind(self.overlay.as_ref()) {
            return;
        }
        let duration = self.motion().fade;
        let shown = self.overlay_fade.value(now);
        self.leaving = previous.filter(|_| !duration.is_zero()).map(|overlay| Leaving {
            overlay,
            fade: Fade::settled(shown, now).toward(TRANSPARENT, now, duration, Easing::OutCubic),
        });
        let start = Fade::settled(TRANSPARENT, now);
        self.overlay_fade =
            if self.overlay.is_some() { start.toward(OPAQUE, now, duration, Easing::OutCubic) } else { start };
    }

    pub(crate) fn sync_fade(&mut self, now: Instant) {
        let target = match self.effective_view() {
            View::Panels => OPAQUE,
            View::Shell => TRANSPARENT,
        };
        if (self.fade.target() - target).abs() > f32::EPSILON {
            self.fade = self.fade.toward(target, now, self.motion().fade, Easing::InOutCubic);
        }
    }

    pub(crate) fn set_split(&mut self, split: f32, now: Instant) {
        let target = clamp_split(split, self.cols);
        if (self.split.target() - target).abs() > f32::EPSILON {
            self.split = self.split.toward(target, now, self.motion().fade, Easing::OutCubic);
        }
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16, now: Instant) {
        self.cols = cols;
        self.rows = rows;
        self.split = Fade::settled(clamp_split(self.split.target(), cols), now);
        self.refit();
        crate::viewer::resize(self);
        crate::editor::resize(self);
    }

    pub(crate) fn refit(&mut self) {
        let layout = self.layout(self.epoch);
        for side in Side::ALL {
            let panel = self.panel_mut(side);
            let reserved = if panel.is_results() { RESULT_FRAME_ROWS } else { PANEL_FRAME_ROWS };
            panel.set_rows(usize::from(layout.panels[side.index()].height.saturating_sub(reserved)));
        }
    }

    pub(crate) fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}

/// Columns and rows left around the process inspection window, which opens over the
/// activity monitor. The overlay shadow fills the right and bottom margins.
const INSPECTION_MARGIN: (u16, u16) = (2, 1);

/// The list and preview panes of the inspection window, splitting it like the panels.
fn inspection_panels([left, right]: [ratatui::layout::Rect; 2]) -> [ratatui::layout::Rect; 2] {
    let whole = ratatui::layout::Rect { width: right.right().saturating_sub(left.x), ..left };
    let (columns, rows) = INSPECTION_MARGIN;
    let outer = ratatui::layout::Rect {
        x: whole.x.saturating_add(columns),
        y: whole.y.saturating_add(rows),
        width: whole.width.saturating_sub(columns * 2),
        height: whole.height.saturating_sub(rows * 2),
    };
    let split = (u32::from(outer.width) * u32::from(left.width) / u32::from(whole.width.max(1))) as u16;
    [
        ratatui::layout::Rect { width: split, ..outer },
        ratatui::layout::Rect { x: outer.x + split, width: outer.width - split, ..outer },
    ]
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
