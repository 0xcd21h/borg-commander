use std::{
    fs::File,
    io::BufWriter,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::{
    Delay, DynamicImage, Frame,
    codecs::gif::{GifEncoder, Repeat},
};

use crate::{Session, Snapshot};

const GIF_SPEED: i32 = 10;

pub struct Recorder {
    interval: Duration,
    takes: Vec<(Snapshot, Instant)>,
}

impl Recorder {
    pub fn new(interval: Duration) -> Self {
        Self { interval, takes: Vec::new() }
    }

    pub fn record(&mut self, session: &mut Session, duration: Duration) -> Result<()> {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            session.pump(self.interval)?;
            let snapshot = session.snapshot();
            let repeated = self.takes.last().is_some_and(|(last, _)| last.same_screen(&snapshot));
            if !repeated {
                self.takes.push((snapshot, Instant::now()));
            }
        }
        Ok(())
    }

    pub fn frames(&self) -> usize {
        self.takes.len()
    }

    pub fn save_gif(&self, path: &Path, tail: Duration) -> Result<()> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut encoder = GifEncoder::new_with_speed(BufWriter::new(file), GIF_SPEED);
        encoder.set_repeat(Repeat::Infinite)?;
        let ends = self.takes.iter().skip(1).map(|(_, at)| *at).map(Some).chain(std::iter::once(None));
        for ((snapshot, start), end) in self.takes.iter().zip(ends) {
            let shown = end.map_or(tail, |end| end.saturating_duration_since(*start));
            let image = DynamicImage::ImageRgb8(snapshot.raster()?).to_rgba8();
            encoder.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_saturating_duration(shown)))?;
        }
        Ok(())
    }
}
