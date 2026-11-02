mod emulator;
mod export;
mod integration;
mod keys;
mod mouse;
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
pub use emulator::{Emulator, PromptState, Screen, Scroll, TypedLine};
pub use integration::{
    SESSION_MARKER, SILENT_CD_KEY, ShellKind, cd_request_file, request_cd, shell_command, user_shell,
};
pub use keys::{encode_key, encode_newline, encode_paste};
pub use mouse::encode_mouse;
pub use osc::OscScanner;
pub use portable_pty::CommandBuilder;
pub use pty::{Foreground, Pty, PtyEvent};
pub use signal::ShellSignal;
pub use size::GridSize;
