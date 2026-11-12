//! The disk use list's keys and clicks, measuring, and typing a cleanup command at the host
//! shell's prompt.

use super::{Disk, DiskRow, NoCleanup};
use crate::{
    AppState, Console, Effect, View,
    docker::{Docker, DockerRequest, FILTER_LIMIT, PAGE},
};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

pub(crate) fn measure(docker: &mut Docker) -> Vec<Effect> {
    docker.request(DockerRequest::DiskUse)
}

/// D: disk use in place of the containers, measured the first time, or the containers again.
pub(crate) fn toggle(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.docker.is_open() && state.docker.disk.open {
        state.docker.disk.open = false;
        return Vec::new();
    }
    let mut effects = if state.docker.is_open() { Vec::new() } else { crate::docker::open(state, now) };
    let docker = &mut state.docker;
    if !docker.is_open() {
        return effects;
    }
    docker.disk.open = true;
    docker.details_focused = false;
    docker.filtering = false;
    if docker.disk.loading.is_none() && (docker.disk.usage.is_none() || docker.disk.stale) {
        effects.extend(measure(docker));
    }
    effects
}

/// The next command the host shell finishes after a cleanup command was typed is taken to be
/// that one, so disk use is measured again.
pub(crate) fn command_ended(state: &mut AppState) {
    let disk = &mut state.docker.disk;
    if state.console == Console::Host && std::mem::take(&mut disk.typed) {
        disk.stale = true;
    }
}

/// Types the selected row's cleanup command at the host shell's prompt without pressing Enter,
/// as the agent's typing does, and shows the console with it.
fn clean(state: &mut AppState) -> Vec<Effect> {
    let line = match state.docker.disk.cleanup() {
        Ok(line) => line,
        Err(reason) => {
            state.notice = match reason {
                NoCleanup::InUse => Some(tr!(docker_disk_in_use).into()),
                NoCleanup::CacheRecord => Some(tr!(docker_disk_cache_together).into()),
                NoCleanup::ControlCharacter => Some(tr!(docker_disk_control_character).into()),
                NoCleanup::Empty => None,
            };
            return Vec::new();
        }
    };
    if state.console != Console::Host {
        state.notice = Some(tr!(docker_disk_host_console).into());
        return Vec::new();
    }
    if !crate::update::shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).into());
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&line.line()).into_owned();
    state.shell.return_view = state.view;
    state.shell.mark_typed();
    state.view = View::Shell;
    state.docker.disk.typed = true;
    vec![Effect::Write(borg_term::encode_paste(&text, state.shell.observed.mode))]
}

/// Enter on a volume: its files in the panel, read only, where Borg can read them on this
/// machine; otherwise why it cannot.
fn fold(disk: &mut Disk, open: bool) {
    match disk.row() {
        Some(DiskRow::Kind(group)) => {
            let kind = disk.groups[group].kind;
            if open && disk.unfolded(kind) {
                disk.move_by(1);
            } else if open {
                disk.unfolded.insert(kind);
            } else {
                disk.unfolded.remove(&kind);
            }
        }
        Some(DiskRow::Item(group, _)) if !open => {
            if let Some(row) = disk.rows.iter().position(|&row| row == DiskRow::Kind(group)) {
                disk.select(row);
            }
        }
        _ => {}
    }
    disk.rebuild();
}

fn filter_key(disk: &mut Disk, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            if disk.filter.chars().count() < FILTER_LIMIT {
                disk.filter.push(c);
            }
        }
        KeyCode::Backspace => {
            disk.filter.pop();
        }
        KeyCode::Enter => disk.filtering = false,
        KeyCode::Esc => {
            disk.filtering = false;
            disk.filter.clear();
        }
        KeyCode::Up | KeyCode::Down => {
            disk.move_by(if key.code == KeyCode::Up { -1 } else { 1 });
            return true;
        }
        _ => return false,
    }
    disk.rebuild();
    true
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let disk = &mut state.docker.disk;
    if disk.filtering && filter_key(disk, key) {
        return Vec::new();
    }
    match key.code {
        KeyCode::Up => disk.move_by(-1),
        KeyCode::Down => disk.move_by(1),
        KeyCode::PageUp => disk.move_by(-(PAGE as isize)),
        KeyCode::PageDown => disk.move_by(PAGE as isize),
        KeyCode::Home => disk.select(0),
        KeyCode::End => disk.select(usize::MAX),
        KeyCode::Left => fold(disk, false),
        KeyCode::Right => fold(disk, true),
        KeyCode::Enter => {
            if let Some(group) = disk.group().filter(|_| matches!(disk.row(), Some(DiskRow::Kind(_)))) {
                let open = !disk.unfolded(group.kind);
                fold(disk, open);
            } else if disk.volume().is_some() {
                return crate::docker::volume::browse(state);
            }
        }
        KeyCode::Char('/') => disk.filtering = true,
        KeyCode::Insert | KeyCode::Char(' ') => {
            disk.toggle_mark();
            disk.move_by(1);
        }
        KeyCode::Char('*') => disk.invert_marks(),
        KeyCode::F(8) | KeyCode::Delete => return clean(state),
        KeyCode::Char('r' | 'R') if disk.loading.is_none() => {
            disk.contents.clear();
            return measure(&mut state.docker);
        }
        KeyCode::Char('d' | 'D') => disk.open = false,
        KeyCode::Esc => {
            if disk.filter.is_empty() {
                disk.open = false;
            } else {
                disk.filter.clear();
                disk.rebuild();
            }
        }
        _ => {}
    }
    Vec::new()
}

pub(crate) fn paste(disk: &mut Disk, text: &str) {
    if disk.filtering {
        disk.filter.extend(text.chars().filter(|c| !c.is_control()).take(FILTER_LIMIT));
        disk.rebuild();
    }
}

/// A click on a row selects it; a double click folds or unfolds a kind.
pub(crate) fn click(state: &mut AppState, index: usize, open: bool) -> Vec<Effect> {
    let disk = &mut state.docker.disk;
    if index >= disk.rows.len() {
        return Vec::new();
    }
    disk.filtering = false;
    disk.select(index);
    if open { on_key(state, &KeyCode::Enter.into()) } else { Vec::new() }
}
