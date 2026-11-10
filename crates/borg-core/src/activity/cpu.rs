//! CPU use averaged over the last few seconds, so lists sorted by it hold still.

use std::time::{Duration, Instant};

/// How long CPU use takes to fade from the average: use this long ago counts about a third as
/// much as use now, like the decayed CPU figure of BSD's `top`.
pub const CPU_WINDOW: Duration = Duration::from_secs(3);
/// The step CPU figures are shown, sorted and judged idle by, in hundredths of a percent.
pub const CPU_STEP: u32 = 10;

/// CPU use in hundredths of a percent of one core, averaged with weights that fade over
/// [`CPU_WINDOW`], so each reading counts for the time it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuAverage {
    value: f64,
    at: Instant,
}

impl CpuAverage {
    /// The average after a reading of `cpu` at `now`, which starts it when there is none.
    pub fn next(previous: Option<Self>, cpu: u32, now: Instant) -> Self {
        let reading = f64::from(cpu);
        let value = previous.map_or(reading, |previous| {
            let elapsed = now.saturating_duration_since(previous.at).as_secs_f64();
            let kept = (-elapsed / CPU_WINDOW.as_secs_f64()).exp();
            reading + (previous.value - reading) * kept
        });
        Self { value, at: now }
    }

    /// The average rounded to [`CPU_STEP`], in hundredths of a percent.
    pub fn shown(&self) -> u32 {
        ((self.value / f64::from(CPU_STEP)).round() as u32).saturating_mul(CPU_STEP)
    }
}
