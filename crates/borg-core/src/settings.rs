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

#[derive(Debug, Clone)]
pub struct Settings {
    pub after_command: AfterCommand,
    pub motion: Motion,
    pub listing_batch: usize,
    pub scrollback: usize,
    pub show_hidden: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            after_command: AfterCommand::ShowShell,
            motion: Motion::LOCAL,
            listing_batch: 2048,
            scrollback: 10_000,
            show_hidden: false,
        }
    }
}
