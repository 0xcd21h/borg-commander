use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleShell,
    SwitchPanel,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Open,
    Descend,
    Leave,
    InsertName,
    ToggleHidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Anywhere,
    Panels,
    PanelsEmptyLine,
}

impl Scope {
    fn applies(self, view: View, line_empty: bool) -> bool {
        match self {
            Self::Anywhere => true,
            Self::Panels => view == View::Panels,
            Self::PanelsEmptyLine => view == View::Panels && line_empty,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub action: Action,
    pub scope: Scope,
    pub label: Option<&'static str>,
}

const fn bind(code: KeyCode, modifiers: KeyModifiers, action: Action, scope: Scope) -> Binding {
    Binding { code, modifiers, action, scope, label: None }
}

const fn labeled(binding: Binding, label: &'static str) -> Binding {
    Binding { label: Some(label), ..binding }
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;

pub const KEYMAP: &[Binding] = &[
    labeled(bind(KeyCode::Char('o'), CTRL, Action::ToggleShell, Scope::Anywhere), "Shell"),
    labeled(bind(KeyCode::F(10), NONE, Action::Quit, Scope::Panels), "Quit"),
    bind(KeyCode::Tab, NONE, Action::SwitchPanel, Scope::PanelsEmptyLine),
    bind(KeyCode::Up, NONE, Action::Up, Scope::Panels),
    bind(KeyCode::Down, NONE, Action::Down, Scope::Panels),
    bind(KeyCode::PageUp, NONE, Action::PageUp, Scope::Panels),
    bind(KeyCode::PageDown, NONE, Action::PageDown, Scope::Panels),
    bind(KeyCode::Home, NONE, Action::Top, Scope::PanelsEmptyLine),
    bind(KeyCode::End, NONE, Action::Bottom, Scope::PanelsEmptyLine),
    bind(KeyCode::Enter, NONE, Action::Open, Scope::PanelsEmptyLine),
    bind(KeyCode::Right, NONE, Action::Descend, Scope::PanelsEmptyLine),
    bind(KeyCode::Left, NONE, Action::Leave, Scope::PanelsEmptyLine),
    bind(KeyCode::Enter, ALT, Action::InsertName, Scope::Panels),
    bind(KeyCode::Char('.'), ALT, Action::ToggleHidden, Scope::PanelsEmptyLine),
];

pub fn lookup(key: &KeyEvent, view: View, line_empty: bool) -> Option<Action> {
    KEYMAP
        .iter()
        .find(|b| b.code == key.code && b.modifiers == key.modifiers && b.scope.applies(view, line_empty))
        .map(|b| b.action)
}
