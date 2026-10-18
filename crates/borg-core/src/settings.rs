use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterCommand {
    ShowShell,
    ReturnToPanels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    pub fade: Duration,
    pub frame: Duration,
}

impl Motion {
    pub const LOCAL: Self = Self { fade: Duration::from_millis(180), frame: Duration::from_millis(16) };
    pub const REMOTE: Self = Self { fade: Duration::from_millis(120), frame: Duration::from_millis(40) };
    pub const STILL: Self = Self { fade: Duration::ZERO, frame: Duration::from_millis(16) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preferences {
    pub after_command: AfterCommand,
    pub show_hidden: bool,
    pub status_line: bool,
    pub menu_bar: bool,
    pub animations: bool,
    pub restore_terminal: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            after_command: AfterCommand::ShowShell,
            show_hidden: false,
            status_line: true,
            menu_bar: false,
            animations: true,
            restore_terminal: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub preferences: Preferences,
    pub motion: Motion,
    pub listing_batch: usize,
    pub scrollback: usize,
}

impl Settings {
    pub fn effective_motion(&self) -> Motion {
        if self.preferences.animations { self.motion } else { Motion::STILL }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { preferences: Preferences::default(), motion: Motion::LOCAL, listing_batch: 2048, scrollback: 10_000 }
    }
}
