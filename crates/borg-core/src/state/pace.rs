//! When the app next has something to do: animations, the screensaver, timers and settling overlays.

use super::{AppState, INTRO, Leaving, OPAQUE, Overlay, TRANSPARENT};
use crate::{Easing, Fade, Pace, Panel};
use std::time::Instant;

impl AppState {
    pub fn pace(&self, now: Instant) -> Option<Pace> {
        let smooth = self.fade.is_animating(now)
            || self.split.is_animating(now)
            || self.appearance.is_animating(now)
            || self.overlay_fade.is_animating(now)
            || self.leaving.as_ref().is_some_and(|leaving| leaving.fade.is_animating(now))
            || self.motions.iter().any(|motion| motion.is_animating(now));
        if smooth {
            Some(Pace::Smooth)
        } else if crate::motion::ambient(self, now) {
            Some(Pace::Ambient)
        } else {
            None
        }
    }

    pub fn raining(&self, now: Instant) -> bool {
        self.rain_until.is_some_and(|until| now < until)
    }

    pub fn can_screensave(&self) -> bool {
        !self.shell.busy()
            && self.intro.is_none()
            && self.jobs.is_empty()
            && self.overlay.is_none()
            && self.viewer.is_none()
            && self.activity.side.is_none()
            && !self.docker.is_open()
            && self.sizing.is_none()
    }

    pub fn wake_at(&self) -> Option<Instant> {
        let reveal = self.operation.as_ref().filter(|op| !op.revealed).map(|op| op.reveal_at);
        let prefix = self.escaped.map(|at| at + crate::update::ESCAPE_WINDOW);
        let intro = self.intro.map(|start| start + INTRO);
        let rest = (self.settings.preferences.screensaver && !self.screensaver && self.can_screensave())
            .then_some(self.idle_since + self.settings.screensaver_after);
        let rain = self.rain_until;
        [
            reveal,
            rest,
            rain,
            prefix,
            intro,
            self.busy.reveal_at(),
            self.glitch_wake,
            crate::plugins::wake_at(self),
            crate::activity::wake_at(self),
            crate::docker::wake_at(self),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn settle(&mut self) {
        self.panels.iter_mut().for_each(Panel::settle);
        self.last_results.iter_mut().flatten().for_each(Panel::settle);
        self.result_history.iter_mut().flatten().for_each(Panel::settle);
    }

    pub(crate) fn sync_overlay(&mut self, previous: Option<Overlay>, now: Instant) {
        let kind = |overlay: Option<&Overlay>| overlay.map(std::mem::discriminant);
        if self.leaving.as_ref().is_some_and(|leaving| !leaving.fade.is_animating(now)) {
            self.leaving = None;
        }
        if kind(previous.as_ref()) == kind(self.overlay.as_ref()) {
            return;
        }
        let duration = self.motion().fade;
        let shown = self.overlay_fade.value(now);
        self.leaving = previous.filter(|_| !duration.is_zero()).map(|overlay| Leaving {
            overlay,
            fade: Fade::settled(shown, now).toward(TRANSPARENT, now, duration, Easing::OutCubic),
        });
        let start = Fade::settled(TRANSPARENT, now);
        self.overlay_fade =
            if self.overlay.is_some() { start.toward(OPAQUE, now, duration, Easing::OutCubic) } else { start };
    }
}
