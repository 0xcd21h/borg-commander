//! Offering a newer Borg. The app looks for a release once a day, and the dialog offers the
//! command that installs it, typed into this computer's shell. The offer waits until the panels
//! show and you pause, so that it takes no key meant for something else. S skips that version
//! until a newer one is out, N turns the daily check off, and Esc leaves the offer for the next
//! day's check.

use std::time::{Duration, Instant};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    AppState, Console, Effect, Overlay, View,
    command::{CommandLine, SUBMIT},
};

const UPDATE: &str = "update";
const SKIP: char = 's';
const NEVER: char = 'n';
/// How long the keyboard and mouse rest before the offer opens.
pub(crate) const QUIET: Duration = Duration::from_secs(2);

/// A newer release, and the command that installs it here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: String,
    /// The version running now.
    pub current: String,
    /// The Borg program the shell runs `update` with.
    pub program: String,
    /// The command of the package manager that installed Borg, which updates it in its place.
    pub manager: Option<String>,
}

impl UpdateOffer {
    fn line(&self) -> Vec<u8> {
        match &self.manager {
            Some(command) => command.as_bytes().to_vec(),
            None => CommandLine::default().word(&self.program).flag(UPDATE).line(),
        }
    }

    /// The command as the dialog shows it.
    pub fn command(&self) -> String {
        String::from_utf8_lossy(&self.line()).into_owned()
    }
}

/// The app found a newer release: the offer waits for its moment.
pub(crate) fn available(state: &mut AppState, offer: UpdateOffer) {
    state.update_offer = Some(offer);
}

/// The panels of this computer show, and nothing runs or waits that the dialog would cover.
fn can_open(state: &AppState) -> bool {
    state.view == View::Panels
        && state.console == Console::Host
        && state.editor.is_none()
        && !state.screensaver
        && state.can_screensave()
}

/// Opens the waiting offer once it can open and the keyboard and mouse have rested.
pub(crate) fn surface(state: &mut AppState, now: Instant) {
    if can_open(state)
        && now >= state.idle_since + QUIET
        && let Some(offer) = state.update_offer.take()
    {
        state.overlay = Some(Overlay::Update(offer));
    }
}

/// When a waiting offer opens if nothing happens before.
pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    (state.update_offer.is_some() && can_open(state)).then_some(state.idle_since + QUIET)
}

pub(crate) fn on_key(state: &mut AppState, offer: UpdateOffer, key: &KeyEvent) -> Vec<Effect> {
    match crate::keyboard::latin(key).code {
        KeyCode::Enter => {
            state.overlay = None;
            run(state, &offer)
        }
        KeyCode::Char(c) if c.to_ascii_lowercase() == SKIP => {
            state.overlay = None;
            state.notice = Some(tr!(update_skipped, version = offer.version));
            vec![Effect::SkipUpdate(offer.version)]
        }
        KeyCode::Char(c) if c.to_ascii_lowercase() == NEVER => {
            state.overlay = None;
            state.notice = Some(tr!(update_checks_off).to_owned());
            vec![Effect::StopUpdateChecks]
        }
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Types the command at the shell prompt, kept in its history, and shows the shell while it
/// runs, as setting up the background service does.
fn run(state: &mut AppState, offer: &UpdateOffer) -> Vec<Effect> {
    if !crate::update::shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    state.shell.return_view = state.view;
    state.shell.submit();
    state.updating = state.shell.integrated;
    state.view = View::Shell;
    vec![Effect::Write([offer.line(), SUBMIT.to_vec()].concat())]
}

/// The update command ended: a notice says how it went.
pub(crate) fn finished(state: &mut AppState, exit: Option<i32>) {
    if state.console != Console::Host || !std::mem::take(&mut state.updating) {
        return;
    }
    state.notice = Some(match exit {
        Some(code) if code != 0 => tr!(update_failed, code = code),
        _ => tr!(update_finished).to_owned(),
    });
}

#[cfg(test)]
mod tests;
