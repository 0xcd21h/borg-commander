//! Which kind of archive a file is: by its first bytes, and by the name panels open on Enter.

/// How a stream of bytes is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

/// The kinds of archive Borg reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    SevenZip,
    Rar,
    Tar(Option<Compression>),
    /// One compressed file, such as `notes.txt.gz`.
    Compressed(Compression),
}

impl Format {
    /// The format's name as people write it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Tar(None) => "tar",
            Self::Tar(Some(Compression::Gzip)) => "tar.gz",
            Self::Tar(Some(Compression::Bzip2)) => "tar.bz2",
            Self::Tar(Some(Compression::Xz)) => "tar.xz",
            Self::Tar(Some(Compression::Zstd)) => "tar.zst",
            Self::Compressed(Compression::Gzip) => "gz",
            Self::Compressed(Compression::Bzip2) => "bz2",
            Self::Compressed(Compression::Xz) => "xz",
            Self::Compressed(Compression::Zstd) => "zst",
        }
    }
}

/// What the first bytes of a file say it is, before looking inside a compressed stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Container {
    Zip,
    SevenZip,
    Rar,
    Tar,
    Stream(Compression),
}

/// How many bytes `sniff` needs to recognize every format, a tar header included.
pub(crate) const SNIFF_BYTES: usize = 512;

const SIGNATURES: [(&[u8], Container); 10] = [
    (b"PK\x03\x04", Container::Zip),
    (b"PK\x05\x06", Container::Zip),
    (b"PK\x07\x08", Container::Zip),
    (b"7z\xBC\xAF\x27\x1C", Container::SevenZip),
    (b"Rar!\x1A\x07\x00", Container::Rar),
    (b"Rar!\x1A\x07\x01\x00", Container::Rar),
    (b"\x1F\x8B", Container::Stream(Compression::Gzip)),
    (b"BZh", Container::Stream(Compression::Bzip2)),
    (b"\xFD7zXZ\x00", Container::Stream(Compression::Xz)),
    (b"\x28\xB5\x2F\xFD", Container::Stream(Compression::Zstd)),
];

const TAR_MAGIC: &[u8] = b"ustar";
const TAR_MAGIC_AT: usize = 257;
const TAR_CHECKSUM: std::ops::Range<usize> = 148..156;
const TAR_HEADER: usize = 512;
const CHECKSUM_SPACE: u8 = b' ';
const OCTAL_RADIX: u32 = 8;

/// The archive the bytes at the start of a file begin, if any.
pub(crate) fn sniff(head: &[u8]) -> Option<Container> {
    SIGNATURES
        .iter()
        .find(|(signature, _)| head.starts_with(signature))
        .map(|(_, container)| *container)
        .or_else(|| is_tar_header(head).then_some(Container::Tar))
}

/// Whether `head` starts with a tar header: the POSIX magic, or an old header whose checksum adds up.
pub(crate) fn is_tar_header(head: &[u8]) -> bool {
    let Some(header) = head.get(..TAR_HEADER) else { return false };
    if header[TAR_MAGIC_AT..].starts_with(TAR_MAGIC) {
        return true;
    }
    let stored = std::str::from_utf8(&header[TAR_CHECKSUM])
        .ok()
        .and_then(|text| u32::from_str_radix(text.trim_matches(|c: char| c == ' ' || c == '\0'), OCTAL_RADIX).ok());
    let sum: u32 = header
        .iter()
        .enumerate()
        .map(|(at, byte)| u32::from(if TAR_CHECKSUM.contains(&at) { CHECKSUM_SPACE } else { *byte }))
        .sum();
    header.iter().any(|byte| *byte != 0) && stored == Some(sum)
}

/// Name endings of the archives a panel opens on Enter, longest first.
const SUFFIXES: [&str; 17] = [
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".tbz", ".txz", ".tzst", ".tar", ".zip", ".7z",
    ".rar", ".gz", ".bz2", ".xz", ".zst",
];

/// The name ending that marks `name` as an archive, matched without regard to case.
pub fn suffix(name: &str) -> Option<&'static str> {
    SUFFIXES.into_iter().find(|suffix| {
        let start = name.len().saturating_sub(suffix.len());
        start > 0 && name.is_char_boundary(start) && name[start..].eq_ignore_ascii_case(suffix)
    })
}

/// Whether Enter opens a file of this name as a folder.
pub fn named(name: &str) -> bool {
    suffix(name).is_some()
}

/// The name of the file inside a single compressed file: its own name without the ending.
pub(crate) fn inner_name(name: &str) -> String {
    match suffix(name) {
        Some(suffix) => name[..name.len() - suffix.len()].to_owned(),
        None => name.to_owned(),
    }
}
