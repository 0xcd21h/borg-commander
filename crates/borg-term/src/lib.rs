mod emulator;
mod integration;
mod keys;
mod osc;
mod pty;
mod signal;
mod size;

pub use alacritty_terminal::{
    index::{Column, Line, Point},
    term::{
        TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, NamedColor, Rgb},
};
pub use emulator::{Emulator, PromptState, Screen};
pub use integration::{ShellKind, shell_command};
pub use keys::{encode_key, encode_paste};
pub use osc::OscScanner;
pub use pty::{Foreground, Pty, PtyEvent};
pub use signal::ShellSignal;
pub use size::GridSize;
