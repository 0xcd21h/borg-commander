//! 7z archives. Their header lists every entry; a solid archive packs many files into one block,
//! which reads from its start.

use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sevenz_rust2::{Archive, ArchiveEntry, ArchiveReader, EncoderMethod, Password};

use super::{Flow, Visit};
use crate::{Error, Format, Index, Item, ItemKind, index::ENTRY_LIMIT, name::clean};

const UNIX_EXTENSION: u32 = 0x8000;
const UNIX_MODE_SHIFT: u32 = 16;
const FILE_TYPE: u32 = 0o170000;
const SYMLINK_TYPE: u32 = 0o120000;
const PERMISSIONS: u32 = 0o777;
const TICKS_PER_SECOND: u64 = 10_000_000;
const NANOS_PER_TICK: u64 = 100;
/// Seconds from 1601, where Windows file times start, to 1970.
const WINDOWS_TO_UNIX: u64 = 11_644_473_600;

fn password(password: Option<&str>) -> Password {
    password.map_or_else(Password::empty, Password::new)
}

pub(crate) fn index(file: File, bytes: u64, given: Option<&str>) -> Result<Index, Error> {
    let archive = Archive::read(&mut BufReader::new(file), &password(given)).map_err(|e| error(e, given))?;
    if archive.files.len() > ENTRY_LIMIT {
        return Err(Error::TooMany);
    }
    let encrypted = archive
        .blocks
        .iter()
        .any(|block| block.coders.iter().any(|coder| coder.encoder_method_id() == EncoderMethod::ID_AES256_SHA256));
    let mut items = Vec::with_capacity(archive.files.len());
    let mut left_out = 0;
    for (slot, entry) in archive.files.iter().enumerate().filter(|(_, entry)| !entry.is_anti_item) {
        let Some(path) = clean(&entry.name, true) else {
            left_out += 1;
            continue;
        };
        let mode = unix_mode(entry);
        let kind = if entry.is_directory {
            ItemKind::Folder
        } else if mode.is_some_and(|mode| mode & FILE_TYPE == SYMLINK_TYPE) {
            ItemKind::Symlink
        } else {
            ItemKind::File
        };
        let mut item = Item::new(path, kind, entry.name.clone(), slot);
        item.size = if kind == ItemKind::File { entry.size } else { 0 };
        item.mode = mode.map_or(item.mode, |mode| mode & PERMISSIONS);
        item.encrypted = encrypted && entry.has_stream;
        item.modified =
            entry.has_last_modified_date.then(|| system_time(u64::from(entry.last_modified_date))).flatten();
        items.push(item);
    }
    Ok(Index::new(Format::SevenZip, bytes, items, left_out))
}

/// The Unix mode 7-Zip and p7zip keep in the upper half of the Windows attributes.
fn unix_mode(entry: &ArchiveEntry) -> Option<u32> {
    let attributes = entry.windows_attributes;
    (entry.has_windows_attributes && attributes & UNIX_EXTENSION != 0).then_some(attributes >> UNIX_MODE_SHIFT)
}

fn system_time(ticks: u64) -> Option<SystemTime> {
    let seconds = (ticks / TICKS_PER_SECOND).checked_sub(WINDOWS_TO_UNIX)?;
    let nanos = (ticks % TICKS_PER_SECOND) * NANOS_PER_TICK;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds) + Duration::from_nanos(nanos))
}

pub(crate) fn each(file: File, given: Option<&str>, wanted: &[&Item], visit: Visit) -> Result<(), Error> {
    if given.is_none() && wanted.iter().any(|item| item.encrypted) {
        return Err(Error::NeedsPassword);
    }
    let mut reader = ArchiveReader::new(BufReader::new(file), password(given)).map_err(|e| error(e, given))?;
    let mut left: HashMap<&str, &Item> = wanted.iter().map(|item| (item.raw.as_str(), *item)).collect();
    let mut failure = None;
    let result = reader.for_each_entries(|entry, data| {
        let Some(item) = left.remove(entry.name.as_str()) else { return Ok(true) };
        match visit(item, data) {
            Ok(Flow::Next) => Ok(!left.is_empty()),
            Ok(Flow::Stop) => Ok(false),
            Err(error) => {
                failure = Some(error);
                Ok(false)
            }
        }
    });
    if let Some(failure) = failure {
        return Err(failure);
    }
    result.map_err(|e| error(e, given))
}

fn error(error: sevenz_rust2::Error, given: Option<&str>) -> Error {
    match error {
        sevenz_rust2::Error::PasswordRequired if given.is_none() => Error::NeedsPassword,
        sevenz_rust2::Error::PasswordRequired | sevenz_rust2::Error::MaybeBadPassword(_) => Error::WrongPassword,
        sevenz_rust2::Error::BadSignature(_) => Error::Unknown,
        sevenz_rust2::Error::Io(error, _) | sevenz_rust2::Error::FileOpen(error, _) => super::stream::damaged(error),
        other => Error::Damaged(other.to_string()),
    }
}
