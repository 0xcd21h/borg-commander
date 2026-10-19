use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{AppState, Easing, Fade, Listing, Side};

const SETTLED: f32 = 0.0;
const SCROLL_JUMP: usize = 1;
pub const EMPTY_ART_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Smooth,
    Ambient,
}

#[derive(Debug, Clone, Copy)]
pub struct PanelMotion {
    pub scroll: Fade,
    pub arrived: Instant,
}

impl PanelMotion {
    pub fn settled(now: Instant) -> Self {
        Self { scroll: Fade::settled(SETTLED, now), arrived: now }
    }

    pub fn scroll(&self, now: Instant, actual: usize) -> usize {
        if self.scroll.is_animating(now) { self.scroll.value(now).round().max(0.0) as usize } else { actual }
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.scroll.is_animating(now)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Before {
    dir: PathBuf,
    scroll: usize,
}

pub(crate) fn before(state: &AppState) -> [Before; 2] {
    Side::ALL.map(|side| {
        let panel = state.panel(side);
        Before { dir: panel.dir().to_path_buf(), scroll: panel.scroll() }
    })
}

pub(crate) fn animate(state: &mut AppState, before: &[Before; 2], now: Instant) {
    let duration = state.motion().fade;
    for side in Side::ALL {
        let previous = &before[side.index()];
        let panel = state.panel(side);
        let (dir, scroll) = (panel.dir().to_path_buf(), panel.scroll());
        let motion = &mut state.motions[side.index()];

        if dir != previous.dir {
            motion.scroll = Fade::settled(scroll as f32, now);
            motion.arrived = now;
            continue;
        }
        if scroll != previous.scroll
            && (scroll.abs_diff(previous.scroll) > SCROLL_JUMP || motion.scroll.is_animating(now))
        {
            let from = motion.scroll(now, previous.scroll) as f32;
            motion.scroll = Fade::settled(from, now).toward(scroll as f32, now, duration, Easing::OutCubic);
        }
    }
}

pub(crate) fn ambient(state: &AppState, now: Instant) -> bool {
    let loading = state.panels.iter().any(|panel| panel.listing() == &Listing::Loading);
    let working = (state.operation.is_some() && state.shell.running) || !state.jobs.is_empty();
    let empty_art = state.settings.preferences.delight
        && Side::ALL.iter().any(|side| {
            let panel = state.panel(*side);
            let fresh = now < state.motions[side.index()].arrived + EMPTY_ART_WINDOW;
            fresh && panel.listing() == &Listing::Ready && panel.entries().is_empty()
        });
    let rain = state.rain_until.is_some_and(|until| now < until);
    let about = matches!(state.overlay, Some(crate::Overlay::About(_)));
    let decorative = state.settings.preferences.animations && (empty_art || rain || about);
    loading || working || decorative || state.screensaver
}
