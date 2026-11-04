//! The window Borg opens on start when there are named sessions. It lists them with when each
//! was last used and what runs it now, opens one, names a new one, and removes one that nothing
//! runs after asking, with No chosen first.

use std::{cmp::Reverse, time::SystemTime};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    LineInput,
    input::Edit,
    keyboard::{self, YesNo},
};

/// The ID of the session that holds Borg's state from before named sessions. It is never removed.
pub const DEFAULT_SESSION: &str = "default";
/// The most characters a session's name has.
pub const SESSION_NAME_LIMIT: usize = 40;
/// How many rows PgUp and PgDn move the choice.
const PAGE: usize = 10;

/// What keeps a session running right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionUse {
    /// A Borg in another terminal has it open.
    Terminal,
    /// The background service keeps it running.
    Service,
}

/// A named session as the window lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    pub id: String,
    pub name: String,
    pub last_used: Option<SystemTime>,
    pub in_use: Option<SessionUse>,
}

impl SessionEntry {
    pub fn is_default(&self) -> bool {
        self.id == DEFAULT_SESSION
    }

    /// Whether it may be removed: it is not the Default session and nothing runs it.
    pub fn removable(&self) -> bool {
        !self.is_default() && self.in_use.is_none()
    }

    /// Why it cannot be removed, if it cannot.
    fn kept(&self) -> Option<String> {
        if self.is_default() {
            return Some(tr!(remote_session_default_protected).into());
        }
        self.in_use.map(|in_use| match in_use {
            SessionUse::Terminal => tr!(session_kept_by_terminal, name = self.name),
            SessionUse::Service => tr!(session_kept_by_service, name = self.name),
        })
    }
}

/// What the window is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerStep {
    Choosing,
    /// Typing the name of a new session.
    Naming(LineInput),
    /// Asking whether to remove the session `id`; `accept` is true while Yes is chosen.
    Removing {
        id: String,
        accept: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPicker {
    /// The sessions, the one used last first.
    pub sessions: Vec<SessionEntry>,
    pub selected: usize,
    pub step: PickerStep,
    /// What the last key could not do, or what removing a session came to.
    pub notice: Option<String>,
}

/// What the user asked of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionChoice {
    Open(String),
    /// A new session with this name, opened once it exists.
    Create(String),
    Remove(String),
    Quit,
}

/// Input for the window: a key, a click choosing a row, or the wheel moving the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerInput {
    Key(KeyEvent),
    Row(usize),
    Scroll(isize),
}

impl SessionPicker {
    /// The window over `sessions`, with the one used last chosen among those that can open here.
    pub fn new(sessions: Vec<SessionEntry>) -> Self {
        let sessions = newest_first(sessions);
        let selected = sessions.iter().position(|entry| entry.in_use != Some(SessionUse::Terminal)).unwrap_or_default();
        Self { sessions, selected, step: PickerStep::Choosing, notice: None }
    }

    pub fn chosen(&self) -> Option<&SessionEntry> {
        self.sessions.get(self.selected)
    }

    /// The sessions read again, with the same one chosen while it is listed. A question about
    /// removing a session that something now runs, or that is gone, closes and says why.
    pub fn reload(&mut self, sessions: Vec<SessionEntry>) {
        let chosen = self.chosen().map(|entry| entry.id.clone());
        self.sessions = newest_first(sessions);
        let at = chosen.and_then(|id| self.sessions.iter().position(|entry| entry.id == id));
        self.selected = at.unwrap_or(self.selected).min(self.sessions.len().saturating_sub(1));
        if let PickerStep::Removing { id, .. } = &self.step {
            match self.sessions.iter().find(|entry| &entry.id == id) {
                Some(entry) if entry.removable() => {}
                entry => {
                    self.notice = entry.and_then(SessionEntry::kept);
                    self.step = PickerStep::Choosing;
                }
            }
        }
    }

    pub fn update(&mut self, input: PickerInput) -> Option<SessionChoice> {
        match input {
            PickerInput::Key(key) => self.on_key(&key),
            PickerInput::Row(row) if self.step == PickerStep::Choosing && row < self.sessions.len() => {
                self.choose(row);
                None
            }
            PickerInput::Scroll(rows) if self.step == PickerStep::Choosing => {
                self.choose(self.selected.saturating_add_signed(rows));
                None
            }
            PickerInput::Row(_) | PickerInput::Scroll(_) => None,
        }
    }

    /// What removing the session `id` came to: it leaves the list, or the window says why it stays.
    pub fn removed(&mut self, id: &str, outcome: Result<(), String>) {
        let Some(at) = self.sessions.iter().position(|entry| entry.id == id) else { return };
        let name = self.sessions[at].name.clone();
        self.notice = Some(match outcome {
            Ok(()) => {
                self.sessions.remove(at);
                self.selected = self.selected.min(self.sessions.len().saturating_sub(1));
                tr!(session_removed, name = name)
            }
            Err(error) => tr!(session_remove_failed, name = name, error = error),
        });
    }

    /// Creating the session failed, and its name stays in the field to change.
    pub fn not_created(&mut self, error: String) {
        self.notice = Some(tr!(session_create_failed, error = error));
    }

    fn choose(&mut self, row: usize) {
        self.selected = row.min(self.sessions.len().saturating_sub(1));
        self.notice = None;
    }

    fn on_key(&mut self, key: &KeyEvent) -> Option<SessionChoice> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && keyboard::latin(key).code == KeyCode::Char('c') {
            return Some(SessionChoice::Quit);
        }
        match std::mem::replace(&mut self.step, PickerStep::Choosing) {
            PickerStep::Choosing => self.choosing_key(key),
            PickerStep::Naming(input) => self.naming_key(input, key),
            PickerStep::Removing { id, accept } => self.removing_key(id, accept, key),
        }
    }

    fn choosing_key(&mut self, key: &KeyEvent) -> Option<SessionChoice> {
        match keyboard::latin(key).code {
            KeyCode::Up => self.choose(self.selected.saturating_sub(1)),
            KeyCode::Down => self.choose(self.selected + 1),
            KeyCode::PageUp => self.choose(self.selected.saturating_sub(PAGE)),
            KeyCode::PageDown => self.choose(self.selected + PAGE),
            KeyCode::Home => self.choose(0),
            KeyCode::End => self.choose(self.sessions.len()),
            KeyCode::Enter => return self.open(),
            KeyCode::F(7) | KeyCode::Char('n' | 'N') => {
                self.notice = None;
                self.step = PickerStep::Naming(LineInput::default());
            }
            KeyCode::F(8) | KeyCode::Delete => self.ask_remove(),
            KeyCode::Esc | KeyCode::F(10) => return Some(SessionChoice::Quit),
            _ => {}
        }
        None
    }

    /// Enter opens the chosen session, unless another terminal has it open.
    fn open(&mut self) -> Option<SessionChoice> {
        let entry = self.chosen()?;
        if entry.in_use == Some(SessionUse::Terminal) {
            self.notice = Some(tr!(session_open_elsewhere, name = entry.name));
            return None;
        }
        Some(SessionChoice::Open(entry.id.clone()))
    }

    /// F8 asks before removing the chosen session, or says why it stays.
    fn ask_remove(&mut self) {
        let Some(entry) = self.chosen() else { return };
        let (id, kept) = (entry.id.clone(), entry.kept());
        if kept.is_none() {
            self.step = PickerStep::Removing { id, accept: false };
        }
        self.notice = kept;
    }

    fn naming_key(&mut self, mut input: LineInput, key: &KeyEvent) -> Option<SessionChoice> {
        match key.code {
            KeyCode::Esc => {
                self.notice = None;
                return None;
            }
            KeyCode::Enter => {
                let name = input.text().trim().to_owned();
                self.notice = name_problem(&name, &self.sessions);
                if self.notice.is_none() {
                    self.step = PickerStep::Naming(input);
                    return Some(SessionChoice::Create(name));
                }
            }
            _ => {
                if input.apply(key) == Edit::Changed {
                    self.notice = None;
                }
            }
        }
        self.step = PickerStep::Naming(input);
        None
    }

    fn removing_key(&mut self, id: String, accept: bool, key: &KeyEvent) -> Option<SessionChoice> {
        match keyboard::yes_no(accept, key) {
            YesNo::Switch => self.step = PickerStep::Removing { id, accept: !accept },
            YesNo::Close(Some(true)) => return Some(SessionChoice::Remove(id)),
            YesNo::Close(_) => {}
            YesNo::Ignore => self.step = PickerStep::Removing { id, accept },
        }
        None
    }
}

/// Why `name` cannot name a new session, if it cannot.
fn name_problem(name: &str, sessions: &[SessionEntry]) -> Option<String> {
    if name.is_empty() || name.chars().count() > SESSION_NAME_LIMIT || name.chars().any(char::is_control) {
        return Some(tr!(session_name_invalid, limit = SESSION_NAME_LIMIT));
    }
    sessions.iter().any(|entry| entry.name == name).then(|| tr!(session_name_taken, name = name))
}

fn newest_first(mut sessions: Vec<SessionEntry>) -> Vec<SessionEntry> {
    sessions.sort_by_key(|entry| Reverse(entry.last_used));
    sessions
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    const DAY: u64 = 86_400;

    fn entry(id: &str, name: &str, days: u64, in_use: Option<SessionUse>) -> SessionEntry {
        SessionEntry {
            id: id.into(),
            name: name.into(),
            last_used: Some(UNIX_EPOCH + Duration::from_secs(days * DAY)),
            in_use,
        }
    }

    fn picker() -> SessionPicker {
        SessionPicker::new(vec![
            entry(DEFAULT_SESSION, "Default", 10, Some(SessionUse::Terminal)),
            entry("a1", "scratch", 2, None),
            entry("b2", "web", 30, Some(SessionUse::Service)),
            SessionEntry { last_used: None, ..entry("c3", "legacy", 0, None) },
        ])
    }

    fn press(picker: &mut SessionPicker, code: KeyCode) -> Option<SessionChoice> {
        picker.update(PickerInput::Key(KeyEvent::from(code)))
    }

    fn names(picker: &SessionPicker) -> Vec<&str> {
        picker.sessions.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn the_session_used_last_comes_first_and_is_chosen_unless_another_terminal_has_it() {
        let picker = picker();
        assert_eq!(names(&picker), ["web", "Default", "scratch", "legacy"]);
        assert_eq!(picker.chosen().map(|entry| entry.id.as_str()), Some("b2"));
        let mut sessions = picker.sessions;
        sessions[0].in_use = Some(SessionUse::Terminal);
        let picker = SessionPicker::new(sessions);
        assert_eq!(picker.chosen().map(|entry| entry.id.as_str()), Some("a1"));
    }

    #[test]
    fn enter_opens_a_session_unless_another_terminal_has_it_open() {
        let mut picker = picker();
        assert_eq!(press(&mut picker, KeyCode::Enter), Some(SessionChoice::Open("b2".into())));
        press(&mut picker, KeyCode::Down);
        assert_eq!(press(&mut picker, KeyCode::Enter), None);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("another terminal")));
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.notice, None, "moving clears the notice");
        assert_eq!(press(&mut picker, KeyCode::Enter), Some(SessionChoice::Open("a1".into())));
    }

    #[test]
    fn active_sessions_and_the_default_one_are_never_offered_for_removal() {
        let mut picker = picker();
        press(&mut picker, KeyCode::Delete);
        assert_eq!(picker.step, PickerStep::Choosing);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("background service")));
        picker.update(PickerInput::Row(1));
        press(&mut picker, KeyCode::F(8));
        assert_eq!(picker.step, PickerStep::Choosing);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("Default")));
        let mut sessions = picker.sessions.clone();
        sessions[1].id = "d4".into();
        picker.reload(sessions);
        press(&mut picker, KeyCode::F(8));
        assert_eq!(picker.step, PickerStep::Choosing);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("another terminal")));
    }

    #[test]
    fn removal_asks_first_with_no_chosen() {
        let mut picker = picker();
        picker.update(PickerInput::Row(2));
        press(&mut picker, KeyCode::F(8));
        assert_eq!(picker.step, PickerStep::Removing { id: "a1".into(), accept: false });
        assert_eq!(press(&mut picker, KeyCode::Enter), None, "Enter takes No");
        assert_eq!(picker.step, PickerStep::Choosing);
        press(&mut picker, KeyCode::F(8));
        assert_eq!(press(&mut picker, KeyCode::Esc), None);
        assert_eq!(picker.step, PickerStep::Choosing, "Esc closes the question and leaves the window open");
        press(&mut picker, KeyCode::F(8));
        press(&mut picker, KeyCode::Tab);
        assert_eq!(press(&mut picker, KeyCode::Enter), Some(SessionChoice::Remove("a1".into())));
        press(&mut picker, KeyCode::F(8));
        assert_eq!(press(&mut picker, KeyCode::Char('y')), Some(SessionChoice::Remove("a1".into())));
    }

    #[test]
    fn a_removed_session_leaves_the_list_and_a_failed_one_stays_with_the_reason() {
        let mut picker = picker();
        picker.update(PickerInput::Row(3));
        picker.removed("c3", Err("locked".into()));
        assert_eq!(names(&picker), ["web", "Default", "scratch", "legacy"]);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("locked")));
        picker.removed("c3", Ok(()));
        assert_eq!(names(&picker), ["web", "Default", "scratch"]);
        assert_eq!(picker.selected, 2);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("legacy")));
    }

    #[test]
    fn a_question_about_a_session_that_started_meanwhile_closes() {
        let mut picker = picker();
        picker.update(PickerInput::Row(2));
        press(&mut picker, KeyCode::F(8));
        let mut sessions = picker.sessions.clone();
        sessions[2].in_use = Some(SessionUse::Terminal);
        sessions.remove(0);
        picker.reload(sessions);
        assert_eq!(picker.step, PickerStep::Choosing);
        assert_eq!(picker.chosen().map(|entry| entry.id.as_str()), Some("a1"), "the choice follows the session");
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("another terminal")));
    }

    #[test]
    fn a_new_session_needs_a_free_name_of_up_to_forty_characters() {
        let mut picker = picker();
        press(&mut picker, KeyCode::F(7));
        assert_eq!(press(&mut picker, KeyCode::Enter), None);
        assert!(picker.notice.is_some(), "an empty name is refused");
        for letter in "web".chars() {
            press(&mut picker, KeyCode::Char(letter));
        }
        assert_eq!(picker.notice, None, "typing clears the notice");
        assert_eq!(press(&mut picker, KeyCode::Enter), None);
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("web")));
        for letter in "-2".chars() {
            press(&mut picker, KeyCode::Char(letter));
        }
        assert_eq!(press(&mut picker, KeyCode::Enter), Some(SessionChoice::Create("web-2".into())));
        assert!(matches!(picker.step, PickerStep::Naming(_)), "the name stays until the session exists");
        picker.not_created("disk full".into());
        assert!(picker.notice.as_deref().is_some_and(|notice| notice.contains("disk full")));
        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.step, PickerStep::Choosing);
        let long = "x".repeat(SESSION_NAME_LIMIT + 1);
        assert!(name_problem(&long, &picker.sessions).is_some());
        assert_eq!(name_problem(&long[1..], &picker.sessions), None);
    }

    #[test]
    fn esc_f10_and_ctrl_c_quit_without_opening_a_session() {
        let mut picker = picker();
        assert_eq!(press(&mut picker, KeyCode::Esc), Some(SessionChoice::Quit));
        assert_eq!(press(&mut picker, KeyCode::F(10)), Some(SessionChoice::Quit));
        press(&mut picker, KeyCode::F(7));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(picker.update(PickerInput::Key(ctrl_c)), Some(SessionChoice::Quit));
    }

    #[test]
    fn the_wheel_and_clicks_move_the_choice_only_while_choosing() {
        let mut picker = picker();
        picker.update(PickerInput::Scroll(2));
        assert_eq!(picker.selected, 2);
        picker.update(PickerInput::Scroll(-5));
        assert_eq!(picker.selected, 0);
        picker.update(PickerInput::Row(9));
        assert_eq!(picker.selected, 0, "a click below the list chooses nothing");
        picker.update(PickerInput::Row(2));
        press(&mut picker, KeyCode::F(8));
        picker.update(PickerInput::Row(3));
        assert_eq!(picker.selected, 2, "the question keeps the choice");
    }
}
