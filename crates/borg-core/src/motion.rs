use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{AppState, Easing, Fade, Listing, Side};

const SETTLED: f32 = 0.0;
const ARRIVED: f32 = 1.0;
const SCROLL_JUMP: usize = 1;
const GLIDE_SHARE: f32 = 0.6;
pub const EMPTY_ART_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
    Forward,
    Back,
}

#[derive(Debug, Clone, Copy)]
pub struct Slide {
    pub direction: SlideDirection,
    pub progress: Fade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Smooth,
    Ambient,
}

#[derive(Debug, Clone, Copy)]
pub struct PanelMotion {
    pub slide: Option<Slide>,
    pub scroll: Fade,
    pub cursor: Fade,
    pub arrived: Instant,
}

impl PanelMotion {
    pub fn settled(now: Instant) -> Self {
        Self { slide: None, scroll: Fade::settled(SETTLED, now), cursor: Fade::settled(SETTLED, now), arrived: now }
    }

    pub fn slide(&self, now: Instant) -> Option<(SlideDirection, f32)> {
        self.slide
            .filter(|slide| slide.progress.is_animating(now))
            .map(|slide| (slide.direction, slide.progress.value(now)))
    }

    pub fn scroll(&self, now: Instant, actual: usize) -> usize {
        if self.scroll.is_animating(now) { self.scroll.value(now).round().max(0.0) as usize } else { actual }
    }

    pub fn cursor(&self, now: Instant, actual: usize) -> f32 {
        if self.cursor.is_animating(now) { self.cursor.value(now) } else { actual as f32 }
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.slide(now).is_some() || self.scroll.is_animating(now) || self.cursor.is_animating(now)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Before {
    dir: PathBuf,
    scroll: usize,
    cursor: usize,
}

pub(crate) fn before(state: &AppState) -> [Before; 2] {
    Side::ALL.map(|side| {
        let panel = state.panel(side);
        Before { dir: panel.dir().to_path_buf(), scroll: panel.scroll(), cursor: panel.cursor() }
    })
}

pub(crate) fn animate(state: &mut AppState, before: &[Before; 2], now: Instant) {
    let duration = state.motion().fade;
    let glide = duration.mul_f32(GLIDE_SHARE);
    for side in Side::ALL {
        let previous = &before[side.index()];
        let panel = state.panel(side);
        let (dir, scroll, cursor) = (panel.dir().to_path_buf(), panel.scroll(), panel.cursor());
        let motion = &mut state.motions[side.index()];

        if dir != previous.dir {
            let direction = if dir.starts_with(&previous.dir) {
                Some(SlideDirection::Forward)
            } else if previous.dir.starts_with(&dir) {
                Some(SlideDirection::Back)
            } else {
                None
            };
            motion.slide = direction.map(|direction| Slide {
                direction,
                progress: Fade::settled(SETTLED, now).toward(ARRIVED, now, duration, Easing::OutCubic),
            });
            if motion.slide(now).is_none() {
                state.panels[side.index()].forget_previous();
            }
            let motion = &mut state.motions[side.index()];
            motion.scroll = Fade::settled(scroll as f32, now);
            motion.cursor = Fade::settled(cursor as f32, now);
            motion.arrived = now;
            continue;
        }
        if motion.slide(now).is_none() {
            state.panels[side.index()].forget_previous();
        }
        let motion = &mut state.motions[side.index()];
        if cursor != previous.cursor {
            let from = motion.cursor(now, previous.cursor);
            motion.cursor = Fade::settled(from, now).toward(cursor as f32, now, glide, Easing::OutCubic);
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
