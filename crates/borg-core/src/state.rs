use std::{path::PathBuf, time::Instant};

use borg_term::{Foreground, PromptState, TermMode};

use crate::{Appearance, Easing, Effect, Fade, Layout, Panel, Picker, Settings};

const OPAQUE: f32 = 1.0;
const TRANSPARENT: f32 = 0.0;

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

#[derive(Debug, Clone)]
pub struct AppState {
    pub panels: [Panel; 2],
    pub active: Side,
    pub view: View,
    pub shell: ShellStatus,
    pub fade: Fade,
    pub layout: Layout,
    pub settings: Settings,
    pub notice: Option<String>,
    pub appearance: Appearance,
    pub picker: Option<Picker>,
    generation: u64,
}

impl AppState {
    pub fn new(
        dirs: [PathBuf; 2],
        cols: u16,
        rows: u16,
        settings: Settings,
        appearance: Appearance,
        now: Instant,
    ) -> (Self, Vec<Effect>) {
        let layout = Layout::new(cols, rows);
        let viewport = layout.viewport();
        let [left, right] = dirs;
        let shell = ShellStatus {
            observed: ShellView::default(),
            cwd: left.clone(),
            running: false,
            typed: false,
            unechoed: false,
            integrated: false,
        };

        let mut state = Self {
            panels: [Panel::new(left.clone(), 0, viewport), Panel::new(right.clone(), 0, viewport)],
            active: Side::Left,
            view: View::Panels,
            shell,
            fade: Fade::settled(OPAQUE, now),
            layout,
            settings,
            notice: None,
            appearance,
            picker: None,
            generation: 0,
        };

        let effects =
            Side::ALL.into_iter().zip([left, right]).map(|(side, dir)| state.navigate(side, dir, None)).collect();
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

    pub fn panel_opacity(&self, now: Instant) -> f32 {
        self.fade.value(now)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.fade.is_animating(now) || self.appearance.is_animating(now)
    }

    pub fn settle(&mut self) {
        self.panels.iter_mut().for_each(Panel::settle);
    }

    pub(crate) fn panel_mut(&mut self, side: Side) -> &mut Panel {
        &mut self.panels[side.index()]
    }

    pub(crate) fn navigate(&mut self, side: Side, dir: PathBuf, focus_child: Option<String>) -> Effect {
        let generation = self.next_generation();
        self.panel_mut(side).navigate(dir.clone(), generation, focus_child);
        Effect::List { side, generation, dir }
    }

    pub(crate) fn reload(&mut self, side: Side) -> Effect {
        let generation = self.next_generation();
        let panel = self.panel_mut(side);
        panel.reload(generation);
        Effect::List { side, generation, dir: panel.destination().to_path_buf() }
    }

    pub(crate) fn sync_fade(&mut self, now: Instant) {
        let target = match self.effective_view() {
            View::Panels => OPAQUE,
            View::Shell => TRANSPARENT,
        };
        if (self.fade.target() - target).abs() > f32::EPSILON {
            self.fade = self.fade.toward(target, now, self.settings.motion.fade, Easing::InOutCubic);
        }
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.layout = Layout::new(cols, rows);
        let viewport = self.layout.viewport();
        self.panels.iter_mut().for_each(|panel| panel.set_viewport(viewport));
    }

    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}
