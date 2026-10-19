use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    Linear,
    OutCubic,
    InOutCubic,
}

impl Easing {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::OutCubic => 1.0 - (1.0 - t).powi(3),
            Self::InOutCubic if t < 0.5 => 4.0 * t.powi(3),
            Self::InOutCubic => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Fade {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    easing: Easing,
}

impl Fade {
    pub fn settled(value: f32, now: Instant) -> Self {
        Self { from: value, to: value, start: now, duration: Duration::ZERO, easing: Easing::Linear }
    }

    pub fn toward(&self, to: f32, now: Instant, duration: Duration, easing: Easing) -> Self {
        Self { from: self.value(now), to, start: now, duration, easing }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let progress = now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        self.from + (self.to - self.from) * self.easing.apply(progress)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_hits_endpoints() {
        for easing in [Easing::Linear, Easing::OutCubic, Easing::InOutCubic] {
            assert!(easing.apply(0.0).abs() < f32::EPSILON);
            assert!((easing.apply(1.0) - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn fade_interpolates_and_settles() {
        let start = Instant::now();
        let duration = Duration::from_millis(100);
        let fade = Fade::settled(1.0, start).toward(0.0, start, duration, Easing::Linear);

        assert!((fade.value(start + duration / 2) - 0.5).abs() < 0.01);
        assert!(fade.is_animating(start + duration / 2));
        assert!(fade.value(start + duration * 2).abs() < f32::EPSILON);
        assert!(!fade.is_animating(start + duration));
    }

    #[test]
    fn retargeting_starts_from_current_value() {
        let start = Instant::now();
        let duration = Duration::from_millis(100);
        let half = start + duration / 2;
        let fade = Fade::settled(1.0, start).toward(0.0, start, duration, Easing::Linear);
        let back = fade.toward(1.0, half, duration, Easing::Linear);

        assert!((back.value(half) - 0.5).abs() < 0.01);
    }
}
