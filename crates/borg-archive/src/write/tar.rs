//! Tar, compressed or not: the archive is read through once and written again, each kept entry
//! with its header and extended records, so long names, owners and times stay. A plain tar that
//! only gains entries is copied and the new entries follow the last one.

use std::{
    ffi::OsStr,
    fs::File,
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    os::unix::ffi::OsStrExt,
    path::Path,
    time::UNIX_EPOCH,
};

use tar::{Builder, EntryType, Header};

use super::plan::{Added, Adding, Plan};
use crate::{
    Compression, Error,
    meter::{Counted, Meter},
    read::{damaged, decoder},
};

const BLOCK: u64 = 512;
const PAX_PATH: &str = "path";
const PAX_LINK: &str = "linkpath";
const PAX_SIZE: &str = "size";
/// Records Borg writes itself, from the entry's path, target and size.
const REWRITTEN: [&str; 3] = [PAX_PATH, PAX_LINK, PAX_SIZE];
const GZIP_LEVEL: u32 = 6;
const BZIP2_LEVEL: u32 = 6;
const XZ_PRESET: u32 = 6;
const ZSTD_LEVEL: i32 = 3;

/// Writes the archive at `real`, changed as `plan` says, into `out`.
pub(crate) fn rewrite(
    real: &Path,
    compression: Option<Compression>,
    plan: &Plan,
    out: File,
    meter: &Meter<'_, '_>,
) -> Result<File, Error> {
    let source = Counted { inner: BufReader::new(File::open(real)?), meter };
    let reader: Box<dyn Read + '_> = match compression {
        Some(compression) => decoder(compression, source)?,
        None => Box::new(source),
    };
    let mut builder = Builder::new(encoder(compression, BufWriter::new(out))?);
    let mut archive = tar::Archive::new(reader);
    for (slot, entry) in archive.entries().map_err(damaged)?.enumerate() {
        let entry = entry.map_err(damaged)?;
        match plan.kept.get(&slot) {
            Some(kept) if kept.path == kept.item.path => copy(&mut builder, entry, None, link(plan, &kept.item))?,
            Some(kept) => copy(&mut builder, entry, Some(kept.stored().into_bytes()), link(plan, &kept.item))?,
            None if !plan.known.contains(&slot) => copy(&mut builder, entry, None, None)?,
            None => continue,
        }
        meter.entry();
    }
    add(&mut builder, plan, meter)?;
    let encoder = builder.into_inner()?;
    let out = encoder.finish()?;
    Ok(out.into_inner().map_err(io::IntoInnerError::into_error)?)
}

/// Adds the plan's new entries to `out`, a copy of a plain tar archive, after its last entry.
pub(crate) fn append(mut out: File, plan: &Plan, meter: &Meter<'_, '_>) -> Result<File, Error> {
    out.seek(SeekFrom::Start(0))?;
    let end = end(&mut out)?;
    out.set_len(end)?;
    out.seek(SeekFrom::Start(end))?;
    let mut builder = Builder::new(BufWriter::new(out));
    add(&mut builder, plan, meter)?;
    Ok(builder.into_inner()?.into_inner().map_err(io::IntoInnerError::into_error)?)
}

/// Where the last entry of a plain tar archive ends.
fn end(file: &mut File) -> Result<u64, Error> {
    let mut archive = tar::Archive::new(BufReader::new(file));
    let mut end = 0;
    for entry in archive.entries_with_seek().map_err(damaged)? {
        let entry = entry.map_err(damaged)?;
        let stored = if entry.header().entry_type() == EntryType::GNUSparse {
            entry.header().entry_size().map_err(damaged)?
        } else {
            entry.size()
        };
        end = entry.raw_file_position() + stored.div_ceil(BLOCK) * BLOCK;
    }
    Ok(end)
}

/// The target a kept link names once folders it points into have moved.
fn link(plan: &Plan, item: &crate::Item) -> Option<Vec<u8>> {
    let target = item.link.as_deref().filter(|_| item.kind == crate::ItemKind::HardLink)?;
    let moved = plan.moved(target);
    (moved != target).then(|| moved.into_bytes())
}

/// Writes `entry` again, under `path` and naming `link` when they are given.
fn copy<W: Write, R: Read>(
    builder: &mut Builder<W>,
    mut entry: tar::Entry<'_, R>,
    path: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
) -> Result<(), Error> {
    let mut records: Vec<(String, Vec<u8>)> = match entry.pax_extensions().map_err(damaged)? {
        Some(extensions) => extensions
            .filter_map(Result::ok)
            .filter_map(|record| Some((record.key().ok()?.to_owned(), record.value_bytes().to_vec())))
            .filter(|(key, _)| !REWRITTEN.contains(&key.as_str()))
            .collect(),
        None => Vec::new(),
    };
    let mut header = match entry.header().entry_type() {
        EntryType::GNUSparse => expanded(entry.header()),
        _ => entry.header().clone(),
    };
    header.set_size(entry.size());
    let stored_path = entry.path_bytes().into_owned();
    match path {
        Some(path) if header.set_path(OsStr::from_bytes(&path)).is_err() => records.push((PAX_PATH.to_owned(), path)),
        Some(_) => {}
        None if *header.path_bytes() != *stored_path => records.push((PAX_PATH.to_owned(), stored_path)),
        None => {}
    }
    let stored_link = entry.link_name_bytes().map(|link| link.into_owned());
    match (link, stored_link) {
        (Some(link), _) if header.set_link_name(OsStr::from_bytes(&link)).is_err() => {
            records.push((PAX_LINK.to_owned(), link));
        }
        (Some(_), _) | (None, None) => {}
        (None, Some(stored)) if header.link_name_bytes().as_deref() != Some(stored.as_slice()) => {
            records.push((PAX_LINK.to_owned(), stored));
        }
        (None, Some(_)) => {}
    }
    builder.append_pax_extensions(records.iter().map(|(key, value)| (key.as_str(), value.as_slice())))?;
    header.set_cksum();
    builder.append(&header, &mut entry)?;
    Ok(())
}

/// A plain file header for a sparse file, whose data reads with its holes filled in.
fn expanded(sparse: &Header) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    let _ = header.set_path(OsStr::from_bytes(&sparse.path_bytes()));
    sparse.mode().ok().into_iter().for_each(|mode| header.set_mode(mode));
    sparse.mtime().ok().into_iter().for_each(|mtime| header.set_mtime(mtime));
    sparse.uid().ok().into_iter().for_each(|uid| header.set_uid(uid));
    sparse.gid().ok().into_iter().for_each(|gid| header.set_gid(gid));
    if let Ok(Some(user)) = sparse.username() {
        let _ = header.set_username(user);
    }
    if let Ok(Some(group)) = sparse.groupname() {
        let _ = header.set_groupname(group);
    }
    header
}

fn add<W: Write>(builder: &mut Builder<W>, plan: &Plan, meter: &Meter<'_, '_>) -> Result<(), Error> {
    for added in &plan.added {
        let mut header = header(added);
        match &added.adding {
            Adding::Folder => builder.append_data(&mut header, &added.path, io::empty())?,
            Adding::Symlink(target) => builder.append_link(&mut header, &added.path, target)?,
            Adding::File { source, .. } => {
                builder.append_data(&mut header, &added.path, Counted { inner: File::open(source)?, meter })?;
            }
        }
        meter.entry();
    }
    Ok(())
}

fn header(added: &Added) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(match added.adding {
        Adding::Folder => EntryType::Directory,
        Adding::Symlink(_) => EntryType::Symlink,
        Adding::File { .. } => EntryType::Regular,
    });
    header.set_size(added.size());
    header.set_mode(added.mode);
    let seconds = added.modified.and_then(|time| time.duration_since(UNIX_EPOCH).ok());
    header.set_mtime(seconds.map_or(0, |seconds| seconds.as_secs()));
    header
}

/// Writes through the compression the archive had.
enum Encoder<W: Write> {
    Plain(W),
    Gzip(flate2::write::GzEncoder<W>),
    Bzip2(bzip2::write::BzEncoder<W>),
    Xz(lzma_rust2::XzWriter<W>),
    Zstd(zstd::stream::write::Encoder<'static, W>),
}

fn encoder<W: Write>(compression: Option<Compression>, inner: W) -> io::Result<Encoder<W>> {
    Ok(match compression {
        None => Encoder::Plain(inner),
        Some(Compression::Gzip) => {
            Encoder::Gzip(flate2::write::GzEncoder::new(inner, flate2::Compression::new(GZIP_LEVEL)))
        }
        Some(Compression::Bzip2) => {
            Encoder::Bzip2(bzip2::write::BzEncoder::new(inner, bzip2::Compression::new(BZIP2_LEVEL)))
        }
        Some(Compression::Xz) => {
            Encoder::Xz(lzma_rust2::XzWriter::new(inner, lzma_rust2::XzOptions::with_preset(XZ_PRESET))?)
        }
        Some(Compression::Zstd) => Encoder::Zstd(zstd::stream::write::Encoder::new(inner, ZSTD_LEVEL)?),
    })
}

impl<W: Write> Encoder<W> {
    fn finish(self) -> io::Result<W> {
        match self {
            Self::Plain(inner) => Ok(inner),
            Self::Gzip(encoder) => encoder.finish(),
            Self::Bzip2(encoder) => encoder.finish(),
            Self::Xz(encoder) => encoder.finish(),
            Self::Zstd(encoder) => encoder.finish(),
        }
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(inner) => inner.write(buf),
            Self::Gzip(encoder) => encoder.write(buf),
            Self::Bzip2(encoder) => encoder.write(buf),
            Self::Xz(encoder) => encoder.write(buf),
            Self::Zstd(encoder) => encoder.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(inner) => inner.flush(),
            Self::Gzip(encoder) => encoder.flush(),
            Self::Bzip2(encoder) => encoder.flush(),
            Self::Xz(encoder) => encoder.flush(),
            Self::Zstd(encoder) => encoder.flush(),
        }
    }
}
