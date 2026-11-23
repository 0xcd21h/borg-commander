use std::{
    path::{Component, Path, PathBuf},
    time::Instant,
};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use zeroize::Zeroizing;

use crate::{AppState, Effect, LineInput, Overlay, Password, Platform, Prompt, PromptPurpose, Side, Tool};

#[cfg(test)]
mod tests;

pub const VAULT_CONFIG: &str = "gocryptfs.conf";
pub const VAULT_KEY_SEPARATOR: &str = "-";
const KEY_GROUPS_PER_LINE: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultMount {
    pub cipher: PathBuf,
    pub root: PathBuf,
}

impl VaultMount {
    pub fn holds(&self, dir: &Path) -> bool {
        dir.starts_with(&self.root)
    }

    pub fn inside(&self, dir: &Path) -> PathBuf {
        Path::new("/").join(dir.strip_prefix(&self.root).unwrap_or(dir))
    }

    pub fn name(&self) -> String {
        vault_name(&self.cipher)
    }

    fn host(&self) -> PathBuf {
        self.cipher.parent().map_or_else(|| self.root.clone(), Path::to_path_buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultFolder {
    pub cipher: PathBuf,
    pub open: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    WrongPassword,
    NoTool(Option<crate::Installer>),
    NoFuse(Platform),
    Exists,
    Busy,
    TooOld(String),
    NotMounted,
    KeyNotKept,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultKey {
    pub name: String,
    pub key: Password,
    pub typed: LineInput,
}

impl VaultKey {
    pub fn new(name: String, key: Password) -> Self {
        Self { name, key, typed: LineInput::default() }
    }

    pub fn lines(&self, width: usize) -> Vec<String> {
        let groups: Vec<&str> = self.key.as_str().split(VAULT_KEY_SEPARATOR).collect();
        let group = groups.first().map_or(0, |group| group.chars().count()) + VAULT_KEY_SEPARATOR.len();
        let per_line = (width / group.max(1)).clamp(1, KEY_GROUPS_PER_LINE);
        let rows: Vec<String> = groups.chunks(per_line).map(|row| row.join(VAULT_KEY_SEPARATOR)).collect();
        let last = rows.len().saturating_sub(1);
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| if index < last { row + VAULT_KEY_SEPARATOR } else { row })
            .collect()
    }

    pub fn confirmed(&self) -> bool {
        let typed: Zeroizing<String> =
            Zeroizing::new(self.typed.text().chars().filter(|typed| !typed.is_whitespace()).collect());
        let last = self.key.as_str().rsplit(VAULT_KEY_SEPARATOR).next().unwrap_or_default();
        !typed.is_empty() && (typed.eq_ignore_ascii_case(last) || typed.eq_ignore_ascii_case(self.key.as_str()))
    }
}

pub(crate) fn found(state: &mut AppState, side: Side, generation: u64, folder: VaultFolder, now: Instant) {
    let panel = state.panel(side);
    if panel.generation() != generation || panel.refreshing() {
        return;
    }
    let (name, open) = (vault_name(&folder.cipher), folder.open.is_some());
    if !crate::jobs::toast_vault(state, side, folder.cipher, open, now) {
        state.notice = Some(cue_notice(&name, open));
    }
}

fn cue_notice(name: &str, open: bool) -> String {
    let (menu, entry) = (tr!(encryption_menu_title), tr!(vault_open));
    if open {
        tr!(vault_cue_open_notice, name = name, menu = menu, entry = entry)
    } else {
        tr!(vault_cue_locked_notice, name = name, menu = menu, entry = entry)
    }
}

pub(crate) fn pick(state: &mut AppState) -> Vec<Effect> {
    let side = state.active;
    let panel = state.panel(side);
    if panel.foreign() || panel.volume.is_some() {
        state.notice = Some(tr!(vault_here_only).to_owned());
        return Vec::new();
    }
    let hovered = panel.hovered_entry().filter(|entry| entry.is_dir()).map(|entry| entry.path(panel.dir()));
    let shown = (!panel.is_results()).then(|| panel.dir().to_path_buf());
    let folders: Vec<PathBuf> = hovered.into_iter().chain(shown).collect();
    if folders.is_empty() {
        state.notice = Some(tr!(vault_pick_folder).to_owned());
        return Vec::new();
    }
    vec![Effect::PickVault { side, folders }]
}

pub(crate) fn picked(state: &mut AppState, side: Side, folders: &[PathBuf], found: Option<VaultFolder>) -> Vec<Effect> {
    match found {
        Some(VaultFolder { cipher, open: Some(root) }) => opened(state, side, VaultMount { cipher, root }),
        Some(VaultFolder { cipher, open: None }) => {
            ask(state, side, cipher, false);
            Vec::new()
        }
        None => {
            state.notice = Some(not_vaults(folders));
            Vec::new()
        }
    }
}

fn not_vaults(folders: &[PathBuf]) -> String {
    match folders {
        [first, second, ..] => tr!(vault_not_either, first = vault_name(first), second = vault_name(second)),
        [only] => tr!(vault_not_one, name = vault_name(only)),
        [] => tr!(vault_pick_folder).to_owned(),
    }
}

pub(crate) fn shown(state: &mut AppState, side: Side, generation: u64, vault: VaultMount) {
    let panel = state.panel_mut(side);
    if panel.generation() == generation && vault.holds(panel.destination()) {
        panel.vault = Some(vault);
    }
}

pub(crate) fn opened(state: &mut AppState, side: Side, vault: VaultMount) -> Vec<Effect> {
    state.notice = None;
    let root = vault.root.clone();
    state.panel_mut(side).vault = Some(vault);
    vec![state.navigate(side, root, None)]
}

pub(crate) fn created(
    state: &mut AppState,
    side: Side,
    cipher: &Path,
    key: Password,
    opened: Result<VaultMount, VaultError>,
) -> Vec<Effect> {
    state.creating_vaults.retain(|creating| creating != cipher);
    let name = vault_name(cipher);
    let effects = match opened {
        Ok(vault) => self::opened(state, side, vault),
        Err(error) => {
            let host = cipher.parent().map(Path::to_path_buf);
            let shown = host.filter(|host| state.panel(side).dir() == host.as_path());
            let effects = shown.map(|host| state.navigate(side, host, Some(name.clone()))).into_iter().collect();
            state.notice = Some(describe(error, &name));
            effects
        }
    };
    state.overlay = Some(Overlay::VaultKey(Box::new(VaultKey::new(name, key))));
    effects
}

pub(crate) fn failed(state: &mut AppState, side: Side, cipher: PathBuf, error: VaultError) {
    state.creating_vaults.retain(|creating| *creating != cipher);
    match error {
        VaultError::WrongPassword => ask(state, side, cipher, true),
        VaultError::NoTool(installer) => crate::install::offer(state, Tool::Gocryptfs, installer),
        error => state.notice = Some(describe(error, &vault_name(&cipher))),
    }
}

pub(crate) fn closed(state: &mut AppState, root: &Path, error: Option<VaultError>) -> Vec<Effect> {
    let name = vault_name(root);
    if let Some(error) = error {
        state.notice = Some(describe(error, &name));
        return Vec::new();
    }
    state.notice = Some(tr!(vault_closed, name = name));
    Side::ALL.into_iter().filter_map(|side| leave_closed(state, side, root)).collect()
}

fn describe(error: VaultError, name: &str) -> String {
    match error {
        VaultError::WrongPassword => tr!(vault_password_wrong, name = name),
        VaultError::NoTool(_) => tr!(vault_no_gocryptfs).to_owned(),
        VaultError::NoFuse(Platform::MacOs) => tr!(vault_no_fuse_mac).to_owned(),
        VaultError::NoFuse(Platform::Linux) => tr!(vault_no_fuse_linux).to_owned(),
        VaultError::Exists => tr!(vault_exists, name = name),
        VaultError::Busy => tr!(vault_busy, name = name),
        VaultError::TooOld(version) => tr!(vault_gocryptfs_too_old, version = version),
        VaultError::NotMounted => tr!(vault_not_mounted, name = name),
        VaultError::KeyNotKept => tr!(vault_key_not_kept, name = name),
        VaultError::Failed(error) => tr!(vault_failed, name = name, error = error),
    }
}

fn leave_closed(state: &mut AppState, side: Side, root: &Path) -> Option<Effect> {
    let panel = state.panel(side);
    let vault = panel.vault.clone().filter(|vault| vault.root == root);
    if vault.is_none() && !panel.destination().starts_with(root) {
        return None;
    }
    let (host, focus) = match vault {
        Some(vault) => (vault.host(), Some(vault.name())),
        None => (root.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf), None),
    };
    state.panel_mut(side).vault = None;
    Some(state.navigate(side, host, focus))
}

pub(crate) fn at_root(state: &AppState, side: Side) -> bool {
    let panel = state.panel(side);
    panel.vault.as_ref().is_some_and(|vault| panel.dir() == vault.root)
}

pub(crate) fn leave(state: &mut AppState, side: Side) -> Option<Effect> {
    let vault = state.panel_mut(side).vault.take()?;
    Some(state.navigate(side, vault.host(), Some(vault.name())))
}

pub(crate) fn close(state: &mut AppState) -> Vec<Effect> {
    match state.active_panel().vault.clone() {
        Some(vault) => {
            state.notice = Some(tr!(vault_closing, name = vault.name()));
            vec![Effect::CloseVault { root: vault.root }]
        }
        None => {
            state.notice = Some(tr!(vault_none).to_owned());
            Vec::new()
        }
    }
}

pub(crate) fn create(state: &mut AppState) -> Vec<Effect> {
    let side = state.active;
    if state.panel(side).foreign() || state.panel(side).volume.is_some() {
        state.notice = Some(tr!(vault_here_only).to_owned());
        return Vec::new();
    }
    show_prompt(state, tr!(vault_create_title), tr!(vault_name_label).to_owned(), PromptPurpose::VaultName { side });
    Vec::new()
}

pub(crate) fn on_prompt(state: &mut AppState, prompt: &mut Prompt, key: &KeyEvent) -> Option<Vec<Effect>> {
    let enter = match key.code {
        KeyCode::Esc => false,
        KeyCode::Enter if !prompt.input.text().trim().is_empty() => true,
        KeyCode::Enter => return Some(Vec::new()),
        _ => return None,
    };
    let text = prompt.input.text().to_owned();
    let purpose = prompt.purpose.clone();
    crate::overlays::close_secret(state, prompt);
    if !enter {
        return Some(Vec::new());
    }
    Some(match purpose {
        PromptPurpose::VaultPassword { side, cipher } => {
            state.notice = Some(tr!(vault_opening, name = vault_name(&cipher)));
            vec![Effect::OpenVault { side, cipher, password: Password::new(&text) }]
        }
        PromptPurpose::VaultName { side } => named(state, side, text.trim()),
        PromptPurpose::VaultNewPassword { side, cipher } => {
            let label = tr!(vault_repeat_label).to_owned();
            let first = Password::new(&text);
            show_prompt(state, tr!(vault_create_title), label, PromptPurpose::VaultRepeat { side, cipher, first });
            Vec::new()
        }
        PromptPurpose::VaultRepeat { side, cipher, first } if first.as_str() == text => {
            state.notice = Some(tr!(vault_creating, name = vault_name(&cipher)));
            state.creating_vaults.push(cipher.clone());
            vec![Effect::CreateVault { side, cipher, password: first }]
        }
        PromptPurpose::VaultRepeat { side, cipher, .. } => {
            let label = tr!(vault_passwords_differ, name = vault_name(&cipher));
            show_prompt(state, tr!(vault_create_title), label, PromptPurpose::VaultNewPassword { side, cipher });
            Vec::new()
        }
        _ => Vec::new(),
    })
}

pub(crate) fn on_key(state: &mut AppState, mut shown: VaultKey, key: &KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter if shown.confirmed() => {
            shown.typed.wipe();
            state.overlay = None;
            state.notice = None;
        }
        KeyCode::Enter | KeyCode::Esc => state.notice = Some(tr!(vault_key_type_last).to_owned()),
        _ => {
            shown.typed.apply(key);
            state.overlay = Some(Overlay::VaultKey(Box::new(shown)));
        }
    }
    Vec::new()
}

pub(crate) fn key_shown_or_coming(state: &AppState) -> bool {
    matches!(state.overlay, Some(Overlay::VaultKey(_))) || !state.creating_vaults.is_empty()
}

fn named(state: &mut AppState, side: Side, name: &str) -> Vec<Effect> {
    if !plain_name(name) {
        state.notice = Some(tr!(vault_bad_name).to_owned());
        return Vec::new();
    }
    if state.panel(side).entries().iter().any(|entry| entry.name == name) {
        state.notice = Some(tr!(vault_exists, name = name));
        return Vec::new();
    }
    let cipher = state.panel(side).dir().join(name);
    let label = tr!(vault_new_password_label, name = name);
    show_prompt(state, tr!(vault_create_title), label, PromptPurpose::VaultNewPassword { side, cipher });
    Vec::new()
}

fn plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!((components.next(), components.next()), (Some(Component::Normal(_)), None)) && !name.contains('/')
}

fn ask(state: &mut AppState, side: Side, cipher: PathBuf, wrong: bool) {
    let name = vault_name(&cipher);
    if state.overlay.is_some() {
        let (menu, entry) = (tr!(encryption_menu_title), tr!(vault_open));
        state.notice = Some(tr!(vault_needs_password, name = name, menu = menu, entry = entry));
        return;
    }
    let label = if wrong { tr!(vault_password_wrong, name = name) } else { tr!(vault_password_for, name = name) };
    state.notice = None;
    show_prompt(state, tr!(vault_password_title), label, PromptPurpose::VaultPassword { side, cipher });
}

fn show_prompt(state: &mut AppState, title: &'static str, label: String, purpose: PromptPurpose) {
    state.overlay = Some(Overlay::Prompt(Prompt {
        title,
        label,
        input: LineInput::default(),
        purpose,
        option_focused: false,
        typed: None,
    }));
}

pub fn vault_name(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}
