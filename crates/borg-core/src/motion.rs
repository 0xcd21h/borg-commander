use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{AppState, Easing, Fade, Listing, Side};

const SETTLED: f32 = 0.0;
const SCROLL_JUMP: usize = 1;
/// Interference flashes over empty folders about this often, at its own moment each time.
const GLITCH_PERIOD: f32 = 12.0;
const GLITCH_SHORTEST: f32 = 0.3;
const GLITCH_SPREAD: f32 = 0.35;

fn unit(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// When the flash of period `index` starts, and how long it lasts, in seconds.
fn burst(index: u64) -> (f32, f32) {
    let start = (index as f32 + 0.2 + unit(index) * 0.6) * GLITCH_PERIOD;
    (start, GLITCH_SHORTEST + unit(index ^ 0x5bd1) * GLITCH_SPREAD)
}

/// The flash over empty folders `t` seconds after Borg started: how far through it is, from
/// 0 to 1, or `None` between flashes. The same `t` always gives the same answer.
pub fn empty_glitch(t: f32) -> Option<f32> {
    let (start, length) = burst((t.max(0.0) / GLITCH_PERIOD) as u64);
    (start..start + length).contains(&t).then(|| (t - start) / length)
}

/// When the next flash starts, in seconds after Borg started.
fn next_empty_glitch(t: f32) -> f32 {
    let index = (t.max(0.0) / GLITCH_PERIOD) as u64;
    let (start, _) = burst(index);
    if t < start { start } else { burst(index + 1).0 }
}

fn empty_shown(state: &AppState) -> bool {
    Side::ALL.iter().any(|side| {
        let panel = state.panel(*side);
        panel.listing() == &Listing::Ready && panel.entries().is_empty()
    })
}

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
    let t = now.saturating_duration_since(state.epoch).as_secs_f32();
    state.glitch_wake = empty_shown(state).then(|| state.epoch + Duration::from_secs_f32(next_empty_glitch(t)));
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
    let spinning = state.busy.any_shown(now);
    let working = !state.jobs.is_empty();
    let empty_art =
        empty_shown(state) && empty_glitch(now.saturating_duration_since(state.epoch).as_secs_f32()).is_some();
    let rain = state.rain_until.is_some_and(|until| now < until);
    let about = matches!(state.overlay, Some(crate::Overlay::About(_)));
    let decorative = empty_art || rain || about;
    spinning || working || decorative || state.screensaver || state.intro.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_folders_flash_rarely_briefly_and_the_same_way_each_time() {
        let samples: Vec<f32> = (0..20_000).map(|step| step as f32 * 0.03).collect();
        let lit = samples.iter().filter(|t| empty_glitch(**t).is_some()).count();
        assert!(lit * 100 < samples.len() * 6, "flashing {lit} of {} samples", samples.len());
        assert!(lit > 0);
        for t in [0.0, 5.0, 33.3, 100.0] {
            let next = next_empty_glitch(t);
            assert!(next > t && next - t < 2.0 * GLITCH_PERIOD);
            assert!(empty_glitch(next + 0.01).is_some(), "a flash starts at {next}");
            assert_eq!(empty_glitch(t), empty_glitch(t), "the same moment, the same picture");
        }
    }

    #[test]
    fn only_shown_empty_folders_wake_borg_for_the_next_flash() {
        let now = Instant::now();
        let startup = crate::Startup::at("/work".into());
        let appearance =
            crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0);
        let (mut state, effects) = AppState::new(startup, 80, 24, crate::Settings::default(), appearance, now);
        assert_eq!(state.glitch_wake, None);
        for effect in effects {
            let crate::Effect::List { side, generation, .. } = effect else { continue };
            crate::update(&mut state, crate::Msg::Listed { side, generation, error: None }, now);
        }
        let wake = state.glitch_wake.expect("an empty folder waits for its next flash");
        assert!(wake > now && !ambient(&state, now));
        assert!(ambient(&state, wake + Duration::from_millis(10)), "frames run during the flash");
    }
}
