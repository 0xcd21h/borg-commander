//! The dialog P opens: the host to scan, which ports, and how fast.

use std::net::IpAddr;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

use super::{MAX_TARGET, PortChoice, PortListError, ScanSpeed};
use crate::{AppState, Effect, Overlay, input::LineInput};

const LIST_LIMIT: usize = 256;
const NAME_PUNCTUATION: [char; 3] = ['.', '-', '_'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Target,
    Ports,
    /// The ports to try, shown when the choice is a list.
    List,
    Speed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortsKind {
    Common,
    All,
    List,
}

impl PortsKind {
    const ALL: [Self; 3] = [Self::Common, Self::All, Self::List];

    pub fn label(self) -> &'static str {
        match self {
            Self::Common => tr!(port_scan_common),
            Self::All => tr!(port_scan_all),
            Self::List => tr!(port_scan_list),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanForm {
    pub target: LineInput,
    pub kind: PortsKind,
    pub list: LineInput,
    pub speed: ScanSpeed,
    pub field: FormField,
    pub error: Option<String>,
    /// The name being looked up before the confirmation.
    pub resolving: Option<String>,
}

impl PortScanForm {
    pub fn new(target: String) -> Self {
        let field = if target.is_empty() { FormField::Target } else { FormField::Ports };
        Self {
            target: LineInput::new(target),
            kind: PortsKind::Common,
            list: LineInput::default(),
            speed: ScanSpeed::Fast,
            field,
            error: None,
            resolving: None,
        }
    }

    /// The fields the dialog shows.
    pub fn fields(&self) -> &'static [FormField] {
        const WITH_LIST: [FormField; 4] = [FormField::Target, FormField::Ports, FormField::List, FormField::Speed];
        const WITHOUT_LIST: [FormField; 3] = [FormField::Target, FormField::Ports, FormField::Speed];
        if self.kind == PortsKind::List { &WITH_LIST } else { &WITHOUT_LIST }
    }

    pub fn choice(&self) -> PortChoice {
        match self.kind {
            PortsKind::Common => PortChoice::Common,
            PortsKind::All => PortChoice::All,
            PortsKind::List => PortChoice::Custom(self.list.text().trim().to_owned()),
        }
    }

    /// The target and the ports, or what is wrong with them.
    pub(super) fn checked(&self) -> Result<(String, Vec<u16>), String> {
        let target = self.target.text().trim().to_owned();
        if target.is_empty() {
            return Err(tr!(port_scan_no_target).to_owned());
        }
        if target.parse::<IpAddr>().is_err() && !is_host_name(&target) {
            return Err(tr!(port_scan_bad_target, target = target));
        }
        let ports = self.choice().ports().map_err(|error| match error {
            PortListError::Empty => tr!(port_scan_no_ports).to_owned(),
            PortListError::NotAPort(item) => {
                tr!(port_scan_not_a_port, item = item, first = super::FIRST_PORT, last = super::LAST_PORT)
            }
            PortListError::Backwards(item) => tr!(port_scan_backwards, item = item),
        })?;
        Ok((target, ports))
    }

    fn step(&mut self, forward: bool) {
        let fields = self.fields();
        let at = fields.iter().position(|field| *field == self.field).unwrap_or_default();
        let next = if forward { (at + 1) % fields.len() } else { (at + fields.len() - 1) % fields.len() };
        self.field = fields[next];
    }

    fn cycle(&mut self, forward: bool) {
        match self.field {
            FormField::Ports => self.kind = cycled(&PortsKind::ALL, self.kind, forward),
            FormField::Speed => self.speed = cycled(&ScanSpeed::ALL, self.speed, forward),
            FormField::Target | FormField::List => {}
        }
    }
}

/// The value after `current` in `all`, or before it, going round.
pub(crate) fn cycled<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let at = all.iter().position(|value| *value == current).unwrap_or_default();
    let next = if forward { (at + 1) % all.len() } else { (at + all.len() - 1) % all.len() };
    all[next]
}

/// A name the resolver can look up: letters, digits, dots, hyphens and underscores.
fn is_host_name(text: &str) -> bool {
    text.len() <= MAX_TARGET
        && text.chars().all(|c| c.is_ascii_alphanumeric() || NAME_PUNCTUATION.contains(&c))
        && text.chars().any(|c| c.is_ascii_alphanumeric())
}

pub(crate) fn on_key(state: &mut AppState, mut form: PortScanForm, key: &KeyEvent) -> Vec<Effect> {
    let code = crate::keyboard::latin(key).code;
    let text_field = matches!(form.field, FormField::Target | FormField::List);
    match code {
        KeyCode::Esc => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Enter => return super::submit(state, form),
        KeyCode::Tab | KeyCode::Down => form.step(true),
        KeyCode::BackTab | KeyCode::Up => form.step(false),
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if !text_field => form.cycle(code != KeyCode::Left),
        _ => {
            let (input, limit) = match form.field {
                FormField::Target => (&mut form.target, MAX_TARGET),
                FormField::List => (&mut form.list, LIST_LIMIT),
                FormField::Ports | FormField::Speed => {
                    state.overlay = Some(Overlay::PortScan(Box::new(form)));
                    return Vec::new();
                }
            };
            let before = input.clone();
            if input.apply(key) == crate::input::Edit::Changed {
                if input.text().chars().count() > limit {
                    *input = before;
                }
                form.error = None;
                form.resolving = None;
            }
        }
    }
    state.overlay = Some(Overlay::PortScan(Box::new(form)));
    Vec::new()
}

/// Pastes into the field being typed.
pub(crate) fn paste(form: &mut PortScanForm, text: &str) {
    let (input, limit) = match form.field {
        FormField::Target => (&mut form.target, MAX_TARGET),
        FormField::List => (&mut form.list, LIST_LIMIT),
        FormField::Ports | FormField::Speed => return,
    };
    let room = limit.saturating_sub(input.text().chars().count());
    let pasted: String = text.chars().filter(|c| !c.is_control()).take(room).collect();
    *input = LineInput::new(format!("{}{pasted}", input.text()));
    form.error = None;
}
