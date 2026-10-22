use std::{
    fs::File,
    io::BufWriter,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use gif::{DisposalMethod, Encoder, Frame, Repeat};
use image::{DynamicImage, GenericImageView, RgbaImage};

use crate::{Session, Snapshot};

const GIF_SPEED: i32 = 10;
const ALPHA: usize = 3;
const TRANSPARENT: u8 = 0;
const CENTISECOND_MS: u128 = 10;

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
            self.take(session.snapshot(), Instant::now());
        }
        Ok(())
    }

    /// Adds a frame shown from `at`, for screens rendered without a session. A frame that
    /// repeats the previous screen only extends it.
    pub fn take(&mut self, snapshot: Snapshot, at: Instant) {
        let repeated = self.takes.last().is_some_and(|(last, _)| last.same_screen(&snapshot));
        if !repeated {
            self.takes.push((snapshot, at));
        }
    }

    pub fn snapshots(&self) -> impl Iterator<Item = &Snapshot> {
        self.takes.iter().map(|(snapshot, _)| snapshot)
    }

    pub fn frames(&self) -> usize {
        self.takes.len()
    }

    pub fn save_gif(&self, path: &Path, tail: Duration) -> Result<()> {
        let Some((first, _)) = self.takes.first() else { return Ok(()) };
        let canvas = first.raster()?;
        let size = |value: u32| u16::try_from(value).context("frame too large for a GIF");
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut encoder = Encoder::new(BufWriter::new(file), size(canvas.width())?, size(canvas.height())?, &[])?;
        encoder.set_repeat(Repeat::Infinite)?;

        let ends = self.takes.iter().skip(1).map(|(_, at)| *at).map(Some).chain(std::iter::once(None));
        let mut previous: Option<RgbaImage> = None;
        for ((snapshot, start), end) in self.takes.iter().zip(ends) {
            let shown = end.map_or(tail, |end| end.saturating_duration_since(*start));
            let image = DynamicImage::ImageRgb8(snapshot.raster()?).to_rgba8();
            let (left, top, width, height) =
                previous.as_ref().map_or((0, 0, image.width(), image.height()), |before| changed(before, &image));
            let mut patch = image.view(left, top, width, height).to_image();
            if let Some(before) = &previous {
                patch.enumerate_pixels_mut().for_each(|(x, y, pixel)| {
                    if before.get_pixel(left + x, top + y) == pixel {
                        pixel.0[ALPHA] = TRANSPARENT;
                    }
                });
            }
            let mut patch = patch.into_raw();
            let mut frame = Frame::from_rgba_speed(size(width)?, size(height)?, &mut patch, GIF_SPEED);
            frame.left = size(left)?;
            frame.top = size(top)?;
            frame.delay = u16::try_from(shown.as_millis() / CENTISECOND_MS).unwrap_or(u16::MAX);
            frame.dispose = DisposalMethod::Keep;
            encoder.write_frame(&frame)?;
            previous = Some(image);
        }
        Ok(())
    }
}

fn changed(before: &RgbaImage, after: &RgbaImage) -> (u32, u32, u32, u32) {
    let differs = |x: u32, y: u32| before.get_pixel(x, y) != after.get_pixel(x, y);
    let rows: Vec<u32> = (0..after.height()).filter(|y| (0..after.width()).any(|x| differs(x, *y))).collect();
    let (Some(top), Some(bottom)) = (rows.first().copied(), rows.last().copied()) else { return (0, 0, 1, 1) };
    let columns = |x: &u32| (top..=bottom).any(|y| differs(*x, y));
    let left = (0..after.width()).find(columns).unwrap_or(0);
    let right = (0..after.width()).rev().find(columns).unwrap_or(left);
    (left, top, right - left + 1, bottom - top + 1)
}
