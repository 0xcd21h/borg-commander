mod app;
mod config;
mod launch;
mod session;
mod store;

use std::{env, io::stdout, process::ExitCode};

use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::supports_keyboard_enhancement,
};

use ratatui::DefaultTerminal;

use crate::{app::App, launch::Launch};

const NESTED_MESSAGE: &str =
    "borg is already running in this terminal. Press Ctrl-O to get back to it, or type `exit` to close this shell.";

#[tokio::main]
async fn main() -> Result<ExitCode> {
    if env::var_os(borg_term::SESSION_MARKER).is_some() {
        eprintln!("{NESTED_MESSAGE}");
        return Ok(ExitCode::FAILURE);
    }

    let launch = Launch::detect()?;
    let mut terminal = ratatui::try_init()?;

    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    execute!(stdout(), EnableBracketedPaste)?;
    if enhanced {
        execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    }

    let result = run(&launch, &mut terminal).await;

    if enhanced {
        execute!(stdout(), PopKeyboardEnhancementFlags)?;
    }
    execute!(stdout(), DisableBracketedPaste)?;
    ratatui::restore();
    result.map(|()| ExitCode::SUCCESS)
}

async fn run(launch: &Launch, terminal: &mut DefaultTerminal) -> Result<()> {
    App::start(launch, terminal.size()?)?.run(terminal).await
}
