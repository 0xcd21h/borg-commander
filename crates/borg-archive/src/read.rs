//! Reading an archive: listing what it holds, and handing out the data of chosen entries in
//! the order the archive stores them.

use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::{
    Compression, Error, Format, Index, Item, ItemKind, Tool,
    error::halted,
    format::{Container, SNIFF_BYTES, is_tar_header, sniff},
};

mod seven;
mod stream;
pub(crate) mod tool;
mod zip;

pub(crate) use stream::damaged;

/// Reports how far a read got: bytes of the archive file read so far and the file's size.
/// Returning false stops the read.
pub type Watch<'a> = &'a mut dyn FnMut(u64, u64) -> bool;

/// Whether to go on to the next entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next,
    Stop,
}

/// Receives each chosen entry with a reader for its data.
pub type Visit<'a> = &'a mut dyn FnMut(&Item, &mut dyn Read) -> Result<Flow, Error>;

/// An archive on disk, with what reading it needs.
#[derive(Debug, Clone, Copy)]
pub struct Archive<'a> {
    pub path: &'a Path,
    pub password: Option<&'a str>,
    /// The installed program that reads RAR archives.
    pub tool: Option<&'a Tool>,
}

/// How many bytes pass between two reports to a `Watch`.
const WATCH_STEP: u64 = 1 << 20;
const ZSTD_WINDOW_LOG_MAX: u32 = 31;

impl Archive<'_> {
    /// Lists the archive.
    pub fn index(&self, watch: Watch) -> Result<Index, Error> {
        let mut file = File::open(self.path)?;
        let bytes = file.metadata()?.len();
        match format(&mut file)? {
            Format::Zip => zip::index(file, bytes),
            Format::SevenZip => seven::index(file, bytes, self.password),
            Format::Rar => tool::index(self.tool.ok_or(Error::NoTool)?, self.path, bytes, self.password),
            Format::Tar(None) => stream::index_tar_seeking(BufReader::new(Watched::new(file, bytes, watch)), bytes),
            Format::Tar(Some(compression)) => {
                let reader = decoder(compression, BufReader::new(Watched::new(file, bytes, watch)))?;
                stream::index_tar(reader, Format::Tar(Some(compression)), bytes)
            }
            Format::Compressed(compression) => {
                let name = self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let modified = file.metadata()?.modified().ok();
                let reader = decoder(compression, BufReader::new(Watched::new(file, bytes, watch)))?;
                stream::index_compressed(reader, compression, bytes, &name, modified)
            }
        }
    }

    /// Whether the password opens the encrypted entries, by reading the smallest of them through.
    pub fn check_password(&self, index: &Index) -> Result<(), Error> {
        let Some(item) = index.smallest_encrypted() else { return Ok(()) };
        self.each(index, &[item], &mut |_, reader| {
            io::copy(reader, &mut io::sink()).map_err(|_| Error::WrongPassword)?;
            Ok(Flow::Stop)
        })
    }

    /// Copies the data of the file at `path` into `into`, refusing more than the archive gives it.
    pub fn copy_out(&self, index: &Index, path: &str, into: &mut dyn Write) -> Result<u64, Error> {
        let item = index.item(path).filter(|item| item.kind == ItemKind::File).ok_or(Error::Unknown)?;
        let mut copied = 0;
        self.each(index, &[item], &mut |item, reader| {
            copied = io::copy(&mut reader.take(item.size + 1), into).map_err(|error| read_failure(item, error))?;
            Ok(Flow::Stop)
        })?;
        match copied.cmp(&item.size) {
            std::cmp::Ordering::Greater => {
                Err(Error::Damaged(borg_i18n::tr!(archive_entry_too_long, path = item.path)))
            }
            std::cmp::Ordering::Less => Err(Error::Damaged(borg_i18n::tr!(archive_entry_short, path = item.path))),
            std::cmp::Ordering::Equal => Ok(copied),
        }
    }

    /// Hands `visit` the data of each of `wanted`, entries of this archive's `index` that
    /// carry data, in the order the archive stores them.
    pub(crate) fn each(&self, index: &Index, wanted: &[&Item], visit: Visit) -> Result<(), Error> {
        if wanted.is_empty() {
            return Ok(());
        }
        let file = File::open(self.path)?;
        match index.format {
            Format::Zip => zip::each(file, self.password, wanted, visit),
            Format::SevenZip => seven::each(file, self.password, wanted, visit),
            Format::Rar => tool::each(self.tool.ok_or(Error::NoTool)?, self.path, self.password, wanted, visit),
            Format::Tar(None) => stream::each_tar(BufReader::new(file), wanted, visit),
            Format::Tar(Some(compression)) => {
                stream::each_tar(decoder(compression, BufReader::new(file))?, wanted, visit)
            }
            Format::Compressed(compression) => {
                let item = wanted[0];
                visit(item, &mut decoder(compression, BufReader::new(file))?).map(drop)
            }
        }
    }
}

/// The archive `file` holds, by its first bytes and, for a compressed stream, the first bytes
/// inside it. Leaves the file at its start.
fn format(file: &mut File) -> Result<Format, Error> {
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    Read::by_ref(file).take(SNIFF_BYTES as u64).read_to_end(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    let format = match sniff(&head).ok_or(Error::Unknown)? {
        Container::Zip => Format::Zip,
        Container::SevenZip => Format::SevenZip,
        Container::Rar => Format::Rar,
        Container::Tar => Format::Tar(None),
        Container::Stream(compression) => {
            let mut inner = Vec::with_capacity(SNIFF_BYTES);
            let reader = decoder(compression, BufReader::new(&mut *file))?;
            reader.take(SNIFF_BYTES as u64).read_to_end(&mut inner).map_err(stream::damaged)?;
            file.seek(SeekFrom::Start(0))?;
            if is_tar_header(&inner) { Format::Tar(Some(compression)) } else { Format::Compressed(compression) }
        }
    };
    Ok(format)
}

/// What a failed read of an entry's data means: an encrypted entry that does not decode most
/// likely has the wrong password.
pub(crate) fn read_failure(item: &Item, error: io::Error) -> Error {
    match stream::damaged(error) {
        Error::Damaged(_) if item.encrypted => Error::WrongPassword,
        other => other,
    }
}

/// Reads through the compression.
pub(crate) fn decoder<'a>(compression: Compression, reader: impl Read + 'a) -> Result<Box<dyn Read + 'a>, Error> {
    Ok(match compression {
        Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        Compression::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(reader)),
        Compression::Xz => Box::new(lzma_rust2::XzReader::new(reader, true)),
        Compression::Zstd => {
            let mut decoder = zstd::stream::read::Decoder::new(reader)?;
            decoder.window_log_max(ZSTD_WINDOW_LOG_MAX)?;
            Box::new(decoder)
        }
    })
}

/// A reader that reports to a `Watch` how much of the archive file it read.
pub(crate) struct Watched<'a, R> {
    inner: R,
    at: u64,
    reported: u64,
    total: u64,
    watch: Watch<'a>,
}

impl<'a, R> Watched<'a, R> {
    pub(crate) fn new(inner: R, total: u64, watch: Watch<'a>) -> Self {
        Self { inner, at: 0, reported: 0, total, watch }
    }

    fn report(&mut self) -> io::Result<()> {
        if self.at.abs_diff(self.reported) < WATCH_STEP {
            return Ok(());
        }
        self.reported = self.at;
        if (self.watch)(self.at, self.total) { Ok(()) } else { Err(halted()) }
    }
}

impl<R: Read> Read for Watched<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.at += read as u64;
        self.report()?;
        Ok(read)
    }
}

impl<R: Seek> Seek for Watched<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.at = self.inner.seek(pos)?;
        self.report()?;
        Ok(self.at)
    }
}
