//! Tests of the activity monitor, by what they exercise, and the helpers they share.

use super::*;
use crate::{AppState, Effect};
use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
use crossterm::event::{KeyCode, KeyEvent};
use std::{path::PathBuf, time::Instant};
mod map;
mod processes;
mod search;
mod snapshots;

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        120,
        32,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.settings.preferences.git_plugin = false;
    (state, now)
}
fn process(pid: u32, start: u64, ram: u64) -> ProcessInfo {
    ProcessInfo {
        key: ProcessKey { pid, start },
        parent: None,
        name: format!("worker-{pid}"),
        executable: None,
        cpu: Some(100),
        resident: Some(ram),
        virtual_bytes: Some(ram * 2),
        read: Some(0),
        written: Some(0),
        protection: None,
    }
}
fn sample(state: &mut AppState, processes: Vec<ProcessInfo>, now: Instant) {
    state.activity.request(ActivityRequest::Sample);
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(state, id, Ok(ActivityData::Sample(SystemSample { processes, ..Default::default() })), now);
}
fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    route_key(state, &code.into(), state.epoch)
}
