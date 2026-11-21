//! Tar archives, compressed or not, and single compressed files: read from the start each time,
//! since a compressed stream cannot jump to an entry.

use std::{
    collections::HashMap,
    io::{self, Read, Seek},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tar::EntryType;

use super::{Flow, Visit};
use crate::{Compression, Error, Format, Index, Item, ItemKind, format::inner_name, index::ENTRY_LIMIT, name::clean};

const PERMISSIONS: u32 = 0o777;

/// Lists a tar archive that can skip over entries' data.
pub(crate) fn index_tar_seeking(reader: impl Read + Seek, bytes: u64) -> Result<Index, Error> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries_with_seek().map_err(damaged)?;
    collect(entries, Format::Tar(None), bytes)
}

/// Lists a tar archive read through its compression.
pub(crate) fn index_tar(reader: impl Read, format: Format, bytes: u64) -> Result<Index, Error> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(damaged)?;
    collect(entries, format, bytes)
}

fn collect<R: Read>(entries: tar::Entries<'_, R>, format: Format, bytes: u64) -> Result<Index, Error> {
    let mut items = Vec::new();
    let mut left_out = 0;
    for (slot, entry) in entries.enumerate() {
        let entry = entry.map_err(damaged)?;
        if items.len() >= ENTRY_LIMIT {
            return Err(Error::TooMany);
        }
        let Some(kind) = kind(entry.header().entry_type()) else {
            left_out += usize::from(special(entry.header().entry_type()));
            continue;
        };
        let raw = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let link = entry.link_name_bytes().map(|link| String::from_utf8_lossy(&link).into_owned());
        let (Some(path), link) = (clean(&raw, false), link) else {
            left_out += 1;
            continue;
        };
        let link = match kind {
            ItemKind::HardLink => match link.as_deref().and_then(|link| clean(link, false)) {
                Some(target) => Some(target),
                None => {
                    left_out += 1;
                    continue;
                }
            },
            _ => link,
        };
        let header = entry.header();
        let mut item = Item::new(path, kind, raw, slot);
        item.size = if kind == ItemKind::File { entry.size() } else { 0 };
        item.modified = header.mtime().ok().and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)));
        item.mode = header.mode().map_or(item.mode, |mode| mode & PERMISSIONS);
        item.link = link;
        items.push(item);
    }
    Ok(Index::new(format, bytes, items, left_out))
}

fn kind(entry_type: EntryType) -> Option<ItemKind> {
    match entry_type {
        EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => Some(ItemKind::File),
        EntryType::Directory => Some(ItemKind::Folder),
        EntryType::Symlink => Some(ItemKind::Symlink),
        EntryType::Link => Some(ItemKind::HardLink),
        _ => None,
    }
}

/// Device files and pipes, which Borg leaves out; other entry types only carry metadata.
fn special(entry_type: EntryType) -> bool {
    matches!(entry_type, EntryType::Char | EntryType::Block | EntryType::Fifo)
}

/// Lists a single compressed file, reading it through to learn its size.
pub(crate) fn index_compressed(
    mut reader: impl Read,
    compression: Compression,
    bytes: u64,
    name: &str,
    modified: Option<SystemTime>,
) -> Result<Index, Error> {
    let size = io::copy(&mut reader, &mut io::sink()).map_err(damaged)?;
    let inner = inner_name(name);
    let mut item = Item::new(inner.clone(), ItemKind::File, inner, 0);
    item.size = size;
    item.modified = modified;
    Ok(Index::new(Format::Compressed(compression), bytes, vec![item], 0))
}

/// Hands `visit` each wanted entry of the tar archive `reader` reads, stopping after the last.
pub(crate) fn each_tar(reader: impl Read, wanted: &[&Item], visit: Visit) -> Result<(), Error> {
    let mut left: HashMap<usize, &Item> =
        wanted.iter().filter_map(|item| item.slot.map(|slot| (slot, *item))).collect();
    let mut archive = tar::Archive::new(reader);
    for (slot, entry) in archive.entries().map_err(damaged)?.enumerate() {
        let mut entry = entry.map_err(damaged)?;
        let Some(item) = left.remove(&slot) else { continue };
        if visit(item, &mut entry)? == Flow::Stop || left.is_empty() {
            break;
        }
    }
    Ok(())
}

/// A read error that means the archive is broken, unless the read was stopped.
pub(crate) fn damaged(error: io::Error) -> Error {
    match Error::from(error) {
        Error::Io(error) => Error::Damaged(error.to_string()),
        other => other,
    }
}
