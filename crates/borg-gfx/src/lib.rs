mod caps;
mod encode;
mod fit;
mod graphics;
mod halfblock;
mod picture;
mod shape;

pub use caps::{Capabilities, CellSize, Hints, Preference, Protocol, Replies, TerminalColors, probe};
pub use fit::{Fit, fit};
pub use graphics::{Anchor, Graphics, Output, Placement};
pub use halfblock::draw_halfblocks;
pub use picture::{Decoded, Picture, decode, is_picture};
pub use shape::{Axis, Card, Fill, Glow, Rgba, Shadow, Shape, Stroke, draw};

#[derive(Debug, thiserror::Error)]
pub enum GfxError {
    #[error("unsupported image format")]
    Unsupported,
    #[error("image too large")]
    TooLarge,
    #[error("pixel buffer has {actual} bytes, expected {expected}")]
    Malformed { expected: usize, actual: usize },
    #[error("encoding image: {0}")]
    Encode(String),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
