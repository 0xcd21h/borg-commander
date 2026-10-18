use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use borg_term::{Foreground, PromptState, TermMode};

use crate::{
    About, Appearance, Easing, Effect, Fade, Layout, LineInput, ListingMode, Motion, Pace, Panel, PanelMotion, Picker,
    Settings, Sort, Viewer,
    layout::{Chrome, EVEN_SPLIT, clamp_split},
    settings::SPLASH_DURATION,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellView {
    pub prompt: PromptState,
    pub foreground: Foreground,
    pub mode: TermMode,
}

impl Default for ShellView {
    fn default() -> Self {
        Self { prompt: PromptState::Unknown, foreground: Foreground::Unknown, mode: TermMode::empty() }
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
}

impl ShellStatus {
    pub fn captured(&self) -> bool {
        self.observed.mode.contains(TermMode::ALT_SCREEN)
    }

    pub fn busy(&self) -> bool {
        self.running || self.observed.foreground == Foreground::Program
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
    Select,
    Unselect,
    Mkdir,
    Compress { sources: Vec<String> },
    Copy { sources: Vec<String> },
    Move { sources: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: &'static str,
    pub label: String,
    pub input: LineInput,
    pub purpose: PromptPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPurpose {
    Delete { sources: Vec<String> },
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
    About(About),
    Themes(Picker),
    Menu(MenuCursor),
    Prompt(Prompt),
    Confirm(Confirm),
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
    pub panels: [Panel; 2],
    pub active: Side,
    pub view: View,
    pub shell: ShellStatus,
    pub fade: Fade,
    pub split: Fade,
    pub cols: u16,
    pub rows: u16,
    pub settings: Settings,
    pub notice: Option<String>,
    pub appearance: Appearance,
    pub overlay: Option<Overlay>,
    pub operation: Option<Operation>,
    pub viewer: Option<Viewer>,
    pub epoch: Instant,
    pub motions: [PanelMotion; 2],
    pub overlay_fade: Fade,
    pub leaving: Option<Leaving>,
    pub splash_until: Option<Instant>,
    pub idle_since: Instant,
    pub screensaver: bool,
    pub rain_until: Option<Instant>,
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
        let shell = ShellStatus {
            observed: ShellView::default(),
            cwd: active_dir,
            running: false,
            typed: false,
            unechoed: false,
            integrated: false,
        };
        let panels = startup.panels.clone().map(|setup| Panel::new(setup.dir, 0, setup.mode, setup.sort));

        let mut state = Self {
            panels,
            active: startup.active,
            view: View::Panels,
            shell,
            fade: Fade::settled(OPAQUE, now),
            split: Fade::settled(clamp_split(startup.split, cols), now),
            cols,
            rows,
            settings,
            notice: None,
            appearance,
            overlay: None,
            operation: None,
            viewer: None,
            epoch: now,
            motions: [PanelMotion::settled(now); 2],
            overlay_fade: Fade::settled(TRANSPARENT, now),
            leaving: None,
            splash_until: None,
            idle_since: now,
            screensaver: false,
            rain_until: None,
            generation: 0,
        };
        let animated = state.motion().fade > Duration::ZERO;
        state.splash_until = (state.settings.preferences.splash && animated).then_some(now + SPLASH_DURATION);
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
        Chrome { menu_bar: prefs.menu_bar, status_line: prefs.status_line }
    }

    pub fn layout(&self, now: Instant) -> Layout {
        Layout::new(self.cols, self.rows, self.chrome(), self.split.value(now))
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
            || self.motions.iter().any(|motion| motion.is_animating(now))
            || self.splash_active(now);
        if smooth {
            Some(Pace::Smooth)
        } else if crate::motion::ambient(self, now) {
            Some(Pace::Ambient)
        } else {
            None
        }
    }

    pub fn splash_active(&self, now: Instant) -> bool {
        self.splash_until.is_some_and(|until| now < until)
    }

    pub fn splash_progress(&self, now: Instant) -> Option<f32> {
        let until = self.splash_until.filter(|until| now < *until)?;
        let elapsed = SPLASH_DURATION.saturating_sub(until.saturating_duration_since(now));
        Some(elapsed.as_secs_f32() / SPLASH_DURATION.as_secs_f32())
    }

    pub fn raining(&self, now: Instant) -> bool {
        self.rain_until.is_some_and(|until| now < until)
    }

    pub fn wake_at(&self) -> Option<Instant> {
        let reveal = self.operation.as_ref().filter(|op| !op.revealed).map(|op| op.reveal_at);
        let splash = self.splash_until;
        let rest = (self.settings.preferences.screensaver && !self.screensaver)
            .then_some(self.idle_since + self.settings.screensaver_after);
        let rain = self.rain_until;
        [reveal, splash, rest, rain].into_iter().flatten().min()
    }

    pub fn settle(&mut self) {
        self.panels.iter_mut().for_each(Panel::settle);
    }

    pub(crate) fn panel_mut(&mut self, side: Side) -> &mut Panel {
        &mut self.panels[side.index()]
    }

    pub(crate) fn navigate(&mut self, side: Side, dir: PathBuf, focus: Option<String>) -> Effect {
        let generation = self.next_generation();
        self.panel_mut(side).navigate(dir.clone(), generation, focus);
        Effect::List { side, generation, dir }
    }

    pub(crate) fn reload(&mut self, side: Side) -> Effect {
        let generation = self.next_generation();
        let panel = self.panel_mut(side);
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
        self.leaving = previous.map(|overlay| Leaving {
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
    }

    pub(crate) fn refit(&mut self) {
        let rows = Layout::new(self.cols, self.rows, self.chrome(), EVEN_SPLIT).panel_rows();
        self.panels.iter_mut().for_each(|panel| panel.set_rows(rows));
    }

    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}
