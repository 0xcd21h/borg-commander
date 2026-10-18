mod app;
mod launch;

use std::io::stdout;

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

#[tokio::main]
async fn main() -> Result<()> {
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
    result
}

async fn run(launch: &Launch, terminal: &mut DefaultTerminal) -> Result<()> {
    App::start(launch, terminal.size()?)?.run(terminal).await
}
