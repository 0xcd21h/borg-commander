use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    hash::{Hash, Hasher},
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, OnceLock},
};

use image::{DynamicImage, ImageFormat, ImageReader, RgbaImage, imageops::FilterType};

use crate::GfxError;

const CHANNELS: usize = 4;
/// Enough of a file's start to tell which picture format it holds.
const HEAD: usize = 32;
const SUPPORTED: [ImageFormat; 6] =
    [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Gif, ImageFormat::WebP, ImageFormat::Bmp, ImageFormat::Ico];

struct Inner {
    id: u64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    png: OnceLock<Result<Vec<u8>, String>>,
}

#[derive(Clone)]
pub struct Picture(Arc<Inner>);

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picture")
            .field("id", &self.0.id)
            .field("width", &self.0.width)
            .field("height", &self.0.height)
            .finish()
    }
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for Picture {}

impl Picture {
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, GfxError> {
        let expected =
            usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| GfxError::TooLarge)? * CHANNELS;
        if rgba.len() != expected {
            return Err(GfxError::Malformed { expected, actual: rgba.len() });
        }
        let mut hasher = DefaultHasher::new();
        (width, height, &rgba).hash(&mut hasher);
        Ok(Self(Arc::new(Inner { id: hasher.finish(), width, height, rgba, png: OnceLock::new() })))
    }

    pub fn id(&self) -> u64 {
        self.0.id
    }

    pub fn width(&self) -> u32 {
        self.0.width
    }

    pub fn height(&self) -> u32 {
        self.0.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.0.rgba
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; CHANNELS] {
        let index = (usize::try_from(y).unwrap_or(0) * usize::try_from(self.0.width).unwrap_or(0)
            + usize::try_from(x).unwrap_or(0))
            * CHANNELS;
        let mut pixel = [0; CHANNELS];
        if let Some(source) = self.0.rgba.get(index..index + CHANNELS) {
            pixel.copy_from_slice(source);
        }
        pixel
    }

    pub fn png(&self) -> Result<&[u8], GfxError> {
        self.0
            .png
            .get_or_init(|| encode_png(self).map_err(|e| e.to_string()))
            .as_deref()
            .map_err(|e| GfxError::Encode(e.clone()))
    }

    pub fn resized(&self, width: u32, height: u32) -> Result<Self, GfxError> {
        if (width, height) == (self.width(), self.height()) {
            return Ok(self.clone());
        }
        let image = self.image()?;
        let scaled = DynamicImage::ImageRgba8(image).resize_exact(width.max(1), height.max(1), FilterType::CatmullRom);
        let scaled = scaled.to_rgba8();
        Self::from_rgba(scaled.width(), scaled.height(), scaled.into_raw())
    }

    fn image(&self) -> Result<RgbaImage, GfxError> {
        RgbaImage::from_raw(self.width(), self.height(), self.0.rgba.clone()).ok_or(GfxError::TooLarge)
    }
}

fn encode_png(picture: &Picture) -> Result<Vec<u8>, GfxError> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(picture.image()?).write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub picture: Picture,
    pub original: (u32, u32),
    pub format: &'static str,
}

pub fn is_picture(path: &Path) -> bool {
    ImageFormat::from_path(path).is_ok_and(|format| SUPPORTED.contains(&format))
}

/// Whether the file starts as a picture Borg shows does, whatever its name says.
pub fn holds_picture(path: &Path) -> bool {
    let mut head = Vec::with_capacity(HEAD);
    let read = std::fs::File::open(path).and_then(|file| file.take(HEAD as u64).read_to_end(&mut head));
    read.is_ok() && image::guess_format(&head).is_ok_and(|format| SUPPORTED.contains(&format))
}

pub fn decode(path: &Path, bounds: (u32, u32)) -> Result<Decoded, GfxError> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let format = reader.format().filter(|format| SUPPORTED.contains(format)).ok_or(GfxError::Unsupported)?;
    let image = reader.decode()?;
    let original = (image.width(), image.height());
    let (max_w, max_h) = bounds;
    let fitted = if original.0 > max_w || original.1 > max_h {
        image.resize(max_w.max(1), max_h.max(1), FilterType::CatmullRom)
    } else {
        image
    };
    let rgba = fitted.to_rgba8();
    let picture = Picture::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())?;
    Ok(Decoded { picture, original, format: format.extensions_str().first().copied().unwrap_or_default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn checker(size: u32) -> Picture {
        let rgba = (0..size * size)
            .flat_map(|i| if (i % size + i / size).is_multiple_of(2) { [255, 255, 255, 255] } else { [0, 0, 0, 255] })
            .collect();
        Picture::from_rgba(size, size, rgba).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn identical_pixels_share_an_id() {
        assert_eq!(checker(4).id(), checker(4).id());
        assert_ne!(checker(4).id(), checker(6).id());
        assert_eq!(checker(4).pixel(1, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn rejects_mismatched_buffers() {
        assert!(matches!(Picture::from_rgba(2, 2, vec![0; 3]), Err(GfxError::Malformed { .. })));
    }

    #[test]
    fn encodes_png_and_decodes_with_a_size_bound() {
        let dir = std::env::temp_dir().join(format!("borg-gfx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let path = dir.join("checker.png");
        std::fs::write(&path, checker(40).png().unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));

        assert!(is_picture(&path) && holds_picture(&path));
        let notes = dir.join("notes.png");
        std::fs::write(&notes, "not a picture").unwrap_or_else(|e| panic!("{e}"));
        assert!(is_picture(&notes) && !holds_picture(&notes), "the name says picture, the bytes do not");
        let decoded = decode(&path, (10, 20)).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(decoded.original, (40, 40));
        assert_eq!((decoded.picture.width(), decoded.picture.height()), (10, 10));
        assert_eq!(decoded.format, "png");
    }
}
