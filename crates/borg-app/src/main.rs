use borg_i18n::tr;
mod app;
mod config;
mod document;
mod launch;
mod peek;
mod plugins;
mod preview;
mod search;
mod session;
mod store;
mod update;

use std::{env, io::stdout, process::ExitCode};

use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::supports_keyboard_enhancement,
};

use ratatui::DefaultTerminal;

use crate::{app::App, launch::Launch};

const USAGE_ERROR: u8 = 2;

const USAGE: &str = tr!(cli_usage);

const NESTED_MESSAGE: &str = tr!(cli_already_running);

#[tokio::main]
async fn main() -> Result<ExitCode> {
    match env::args().nth(1).as_deref() {
        None => {}
        Some("--version" | "-V") => {
            println!("borg {}", update::VERSION);
            return Ok(ExitCode::SUCCESS);
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        }
        Some("update") => {
            return Ok(match tokio::task::spawn_blocking(update::run).await? {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!(tr!(cli_update_failed), error = error);
                    ExitCode::FAILURE
                }
            });
        }
        Some(other) => {
            eprintln!(tr!(cli_unknown_argument), other = other, USAGE = USAGE);
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    }
    if env::var_os(borg_term::SESSION_MARKER).is_some() {
        eprintln!("{NESTED_MESSAGE}");
        return Ok(ExitCode::FAILURE);
    }

    let launch = Launch::detect()?;
    let mut terminal = ratatui::try_init()?;

    let modes = TerminalModes { enhanced: supports_keyboard_enhancement().unwrap_or(false) };
    execute!(stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    if modes.enhanced {
        execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    }

    let detected = launch::terminal(launch.graphics);
    let result = run(&launch, &mut terminal, detected).await;
    drop(modes);
    result.map(|()| ExitCode::SUCCESS)
}

struct TerminalModes {
    enhanced: bool,
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture);
        ratatui::restore();
    }
}

async fn run(launch: &Launch, terminal: &mut DefaultTerminal, detected: launch::Terminal) -> Result<()> {
    App::start(launch, terminal.size()?, detected)?.run(terminal).await
}
