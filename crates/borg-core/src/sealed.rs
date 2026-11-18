use std::path::{Path, PathBuf};

use borg_crypt::TEXT_LIMIT;
use borg_crypt::sealed::{Error, Format};
use borg_i18n::tr;

use crate::{AppState, EditorSlot, Effect, LineInput, Overlay, Password, Prompt, PromptPurpose};

#[cfg(test)]
mod tests;

const AGE_NAME: &str = "age";
const PTE_NAME: &str = "Paranoia Text Encryption";
const BYTES_PER_KIB: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    View,
    Edit(EditorSlot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedFile {
    pub path: PathBuf,
    pub format: Format,
    pub then: Then,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsealError {
    Sealed(Error),
    NotText,
    Failed(String),
}

pub fn format_name(format: Format) -> &'static str {
    match format {
        Format::Age => AGE_NAME,
        Format::Pte => PTE_NAME,
    }
}

pub fn describe(error: &UnsealError, path: &Path) -> String {
    let name = display_name(path);
    match error {
        UnsealError::Sealed(Error::WrongPassword) => tr!(sealed_password_wrong, name = name),
        UnsealError::Sealed(Error::Damaged) => tr!(sealed_damaged, name = name),
        UnsealError::Sealed(Error::Unsupported) => tr!(sealed_unsupported, name = name),
        UnsealError::Sealed(Error::NotForPassword) => tr!(sealed_not_for_password, name = name),
        UnsealError::Sealed(Error::TooLong) => tr!(sealed_too_long, name = name, kib = TEXT_LIMIT / BYTES_PER_KIB),
        UnsealError::Sealed(Error::NeedsMemory) => tr!(sealed_needs_memory, name = name),
        UnsealError::NotText => tr!(sealed_not_text, name = name),
        UnsealError::Failed(error) => tr!(sealed_open_failed, name = name, error = error),
    }
}

pub(crate) fn locked(state: &mut AppState, file: SealedFile, wrong: bool) {
    if !waiting(state, &file) {
        return;
    }
    let prompting = matches!(&state.overlay, Some(Overlay::Prompt(prompt)) if matches!(prompt.purpose, PromptPurpose::FilePassword(_)));
    let name = display_name(&file.path);
    if state.overlay.is_some() && !prompting {
        abandon(state, &file);
        state.notice = Some(tr!(sealed_needs_password, name = name));
        return;
    }
    let label = if wrong {
        tr!(sealed_password_wrong, name = name)
    } else {
        tr!(sealed_password_for, name = name, format = format_name(file.format))
    };
    state.notice = None;
    state.overlay = Some(Overlay::Prompt(Prompt {
        title: tr!(sealed_password_title),
        label,
        input: LineInput::default(),
        purpose: PromptPurpose::FilePassword(Box::new(file)),
        option_focused: false,
        typed: None,
    }));
}

pub(crate) fn unlock(state: &mut AppState, file: SealedFile, password: &str) -> Vec<Effect> {
    state.notice = Some(tr!(sealed_opening, name = display_name(&file.path)));
    vec![Effect::Unseal { file, password: Password::new(password) }]
}

pub(crate) fn failed(state: &mut AppState, file: SealedFile, error: UnsealError) {
    if error == UnsealError::Sealed(Error::WrongPassword) {
        return locked(state, file, true);
    }
    if !waiting(state, &file) {
        return;
    }
    state.notice = Some(describe(&error, &file.path));
    abandon(state, &file);
}

pub(crate) fn abandon(state: &mut AppState, file: &SealedFile) {
    match file.then {
        Then::View => {
            if state.opening.as_ref() == Some(&file.path) {
                state.opening = None;
                state.landing = None;
            }
        }
        Then::Edit(slot) => {
            state.beside.take_if(|(side, _)| slot == EditorSlot::Panel(*side));
            state.landing.take_if(|landing| landing.path() == file.path);
        }
    }
}

fn waiting(state: &AppState, file: &SealedFile) -> bool {
    match file.then {
        Then::View => state.opening.as_ref() == Some(&file.path),
        Then::Edit(_) => true,
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}
