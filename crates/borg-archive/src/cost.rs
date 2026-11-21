//! How long compressing and decompressing take, to tell beforehand how long a job will run.

use std::time::Duration;

use crate::{Compression, Format};

const MEGABYTE: u64 = 1_000_000;

/// Bytes of unpacked data a codec handles each second on one core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Codec {
    pub pack: u64,
    pub unpack: u64,
}

const DEFLATE: Codec = Codec { pack: 40 * MEGABYTE, unpack: 250 * MEGABYTE };
const BZIP2: Codec = Codec { pack: 12 * MEGABYTE, unpack: 40 * MEGABYTE };
const LZMA: Codec = Codec { pack: 3 * MEGABYTE, unpack: 80 * MEGABYTE };
const ZSTD: Codec = Codec { pack: 250 * MEGABYTE, unpack: 900 * MEGABYTE };

/// The codec a format usually packs its data with; a plain tar has none.
pub(crate) fn codec(format: Format) -> Option<Codec> {
    match format {
        Format::Zip => Some(DEFLATE),
        Format::SevenZip | Format::Rar => Some(LZMA),
        Format::Tar(compression) => compression.map(of),
        Format::Compressed(compression) => Some(of(compression)),
    }
}

fn of(compression: Compression) -> Codec {
    match compression {
        Compression::Gzip => DEFLATE,
        Compression::Bzip2 => BZIP2,
        Compression::Xz => LZMA,
        Compression::Zstd => ZSTD,
    }
}

pub(crate) fn packing(codec: Option<Codec>, bytes: u64) -> Duration {
    codec.map_or(Duration::ZERO, |codec| seconds(bytes, codec.pack))
}

pub(crate) fn unpacking(codec: Option<Codec>, bytes: u64) -> Duration {
    codec.map_or(Duration::ZERO, |codec| seconds(bytes, codec.unpack))
}

fn seconds(bytes: u64, rate: u64) -> Duration {
    Duration::from_secs_f64(bytes as f64 / rate as f64)
}
