//! The shell beside the panels: which view shows, which console, and what the shell is doing.

use borg_term::{Foreground, PromptState, TermMode};
use std::path::PathBuf;

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
        self.went_full_screen |= self.busy() && self.captured();
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
