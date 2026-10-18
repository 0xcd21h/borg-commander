use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{ListingMode, Side, SortKey, View};

pub const SPLIT_STEP: i16 = 2;

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
    OpenMenuBar,
    OpenFileMenu,
    PickTheme,
    ToggleMark,
    MarkUp,
    MarkDown,
    SelectGroup,
    UnselectGroup,
    InvertSelection,
    View,
    Edit,
    Copy,
    Move,
    Mkdir,
    Delete,
    Compress,
    ExtractHere,
    ExtractToOther,
    SwapPanels,
    EqualizePanels,
    NudgeSplit(i16),
    ToggleStatusLine,
    ToggleMenuBar,
    ToggleAnimations,
    ToggleRestoreTerminal,
    ToggleReturnToPanels,
    SetListing(Side, ListingMode),
    SetSort(Side, SortKey),
    ToggleReverse(Side),
    Rescan(Side),
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
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const ALT_SHIFT: KeyModifiers = KeyModifiers::ALT.union(KeyModifiers::SHIFT);

pub const KEYMAP: &[Binding] = &[
    labeled(bind(KeyCode::F(2), NONE, Action::OpenFileMenu, Scope::Panels), "File"),
    labeled(bind(KeyCode::F(3), NONE, Action::View, Scope::Panels), "View"),
    labeled(bind(KeyCode::F(4), NONE, Action::Edit, Scope::Panels), "Edit"),
    labeled(bind(KeyCode::F(5), NONE, Action::Copy, Scope::Panels), "Copy"),
    labeled(bind(KeyCode::F(6), NONE, Action::Move, Scope::Panels), "Move"),
    labeled(bind(KeyCode::F(7), NONE, Action::Mkdir, Scope::Panels), "Mkdir"),
    labeled(bind(KeyCode::F(8), NONE, Action::Delete, Scope::Panels), "Delete"),
    labeled(bind(KeyCode::F(9), NONE, Action::OpenMenuBar, Scope::Panels), "Menu"),
    labeled(bind(KeyCode::F(10), NONE, Action::Quit, Scope::Panels), "Quit"),
    labeled(bind(KeyCode::Char('o'), CTRL, Action::ToggleShell, Scope::Anywhere), "Shell"),
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
    bind(KeyCode::Insert, NONE, Action::ToggleMark, Scope::Panels),
    bind(KeyCode::Char('t'), CTRL, Action::ToggleMark, Scope::PanelsEmptyLine),
    bind(KeyCode::Up, SHIFT, Action::MarkUp, Scope::Panels),
    bind(KeyCode::Down, SHIFT, Action::MarkDown, Scope::Panels),
    bind(KeyCode::Char('+'), NONE, Action::SelectGroup, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('-'), NONE, Action::UnselectGroup, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('*'), NONE, Action::InvertSelection, Scope::PanelsEmptyLine),
    bind(KeyCode::Char('u'), CTRL, Action::SwapPanels, Scope::PanelsEmptyLine),
    bind(KeyCode::Left, ALT_SHIFT, Action::NudgeSplit(-SPLIT_STEP), Scope::Panels),
    bind(KeyCode::Right, ALT_SHIFT, Action::NudgeSplit(SPLIT_STEP), Scope::Panels),
];

pub fn lookup(key: &KeyEvent, view: View, line_empty: bool) -> Option<Action> {
    let modifiers = match key.code {
        KeyCode::Char(_) => key.modifiers - KeyModifiers::SHIFT,
        _ => key.modifiers,
    };
    KEYMAP
        .iter()
        .find(|b| b.code == key.code && b.modifiers == modifiers && b.scope.applies(view, line_empty))
        .map(|b| b.action)
}
