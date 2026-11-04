//! The window Borg opens on start when there are named sessions. It runs in this terminal before
//! Borg itself, reads which sessions a Borg or the background service runs, and does what the
//! user chooses: opening a session, creating one, or removing one that nothing runs.

use std::{
    io::stdout,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use borg_core::{PickerInput, SessionChoice, SessionEntry, SessionPicker, SessionUse};
use borg_ui::{Palette, StartLook, ThemeSet, draw_session_picker, session_picker_mouse};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
};
use ratatui::layout::Rect;

use crate::{
    config::ConfigDir,
    service,
    session::StateDir,
    sessions::{self, NamedSession},
};

/// How often the window reads again which sessions run.
const REFRESH_EVERY: Duration = Duration::from_secs(2);

/// Shows the window until the user opens or creates a session, which it returns, or quits.
pub async fn run(state: &StateDir) -> Result<Option<NamedSession>> {
    let config = ConfigDir::locate().and_then(|dir| dir.load().ok()).unwrap_or_default();
    let themes = match ConfigDir::locate() {
        Some(dir) => ThemeSet::builtin()?.with_user_dir(&dir.themes()).0,
        None => ThemeSet::builtin()?,
    };
    let appearance = themes.appearance(config.theme.as_deref(), config.unlocked());
    let palette = Palette::default();
    let look = StartLook {
        theme: themes.find(appearance.current_id()),
        palette: &palette,
        truecolor: crate::launch::truecolor(),
    };
    let mut picker = SessionPicker::new(entries(state).await?);
    let mut terminal = ratatui::try_init()?;
    let _screen = Screen;
    execute!(stdout(), EnableMouseCapture)?;
    loop {
        let clock = borg_ui::local_now();
        terminal.draw(|frame| {
            if let Some(cursor) = draw_session_picker(&picker, &look, &clock, frame.buffer_mut()) {
                frame.set_cursor_position(cursor);
            }
        })?;
        if !event::poll(REFRESH_EVERY)? {
            picker.reload(entries(state).await?);
            continue;
        }
        let size = terminal.size()?;
        let input = match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Some(PickerInput::Key(key)),
            Event::Mouse(mouse) => session_picker_mouse(&picker, mouse, Rect::new(0, 0, size.width, size.height)),
            _ => None,
        };
        match input.and_then(|input| picker.update(input)) {
            None => {}
            Some(SessionChoice::Quit) => return Ok(None),
            Some(SessionChoice::Open(id)) => return sessions::open(state, &id).map(Some),
            Some(SessionChoice::Create(name)) => match sessions::create(state, &name) {
                Ok(session) => return Ok(Some(session)),
                Err(error) => picker.not_created(format!("{error:#}")),
            },
            Some(SessionChoice::Remove(id)) => {
                picker.removed(&id, sessions::delete(state, &id).map_err(|error| format!("{error:#}")));
                picker.reload(entries(state).await?);
            }
        }
    }
}

/// The sessions the window lists, with what runs each one now.
async fn entries(state: &StateDir) -> Result<Vec<SessionEntry>> {
    let service = service::running_sessions().await;
    let sessions = sessions::list(state)?.into_iter().filter(|session| !session.archived);
    Ok(sessions
        .map(|session| SessionEntry {
            in_use: if service.contains(&session.id) {
                Some(SessionUse::Service)
            } else {
                sessions::held(state, &session.id).then_some(SessionUse::Terminal)
            },
            last_used: (session.last_opened > 0).then(|| UNIX_EPOCH + Duration::from_secs(session.last_opened)),
            id: session.id,
            name: session.name,
        })
        .collect())
}

/// Gives the terminal back as it was when the window closes.
struct Screen;

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();
    }
}
