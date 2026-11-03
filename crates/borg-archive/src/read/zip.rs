//! Zip archives, through their central directory, which lists every entry without reading data.

use std::{
    fs::File,
    io::BufReader,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use zip::{ExtraField, ZipArchive, result::ZipError};

use super::{Flow, Visit};
use crate::{Error, Format, Index, Item, ItemKind, index::ENTRY_LIMIT, name::clean};

const PERMISSIONS: u32 = 0o777;

pub(crate) fn index(file: File, bytes: u64) -> Result<Index, Error> {
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(error)?;
    if zip.len() > ENTRY_LIMIT {
        return Err(Error::TooMany);
    }
    if zip.has_overlapping_files().map_err(error)? {
        return Err(Error::Overlapping);
    }
    let mut items = Vec::with_capacity(zip.len());
    let mut left_out = 0;
    for slot in 0..zip.len() {
        let entry = zip.by_index_raw(slot).map_err(error)?;
        let raw = entry.name().map_err(error)?.into_owned();
        let Some(path) = clean(&raw, true) else {
            left_out += 1;
            continue;
        };
        let kind = if entry.is_dir() {
            ItemKind::Folder
        } else if entry.is_symlink() {
            ItemKind::Symlink
        } else {
            ItemKind::File
        };
        let mut item = Item::new(path, kind, raw, slot);
        item.size = if kind == ItemKind::File { entry.size() } else { 0 };
        item.mode = entry.unix_mode().map_or(item.mode, |mode| mode & PERMISSIONS);
        item.encrypted = entry.encrypted();
        item.modified = entry
            .extra_data_fields()
            .find_map(|field| match field {
                ExtraField::ExtendedTimestamp(stamp) => stamp.mod_time(),
                _ => None,
            })
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds.into())))
            .or_else(|| entry.last_modified().and_then(local_time));
        items.push(item);
    }
    Ok(Index::new(Format::Zip, bytes, items, left_out))
}

/// A zip time, which is the local time of the machine that made the archive.
fn local_time(time: zip::DateTime) -> Option<SystemTime> {
    let civil = jiff::civil::DateTime::new(
        time.year().try_into().ok()?,
        time.month().try_into().ok()?,
        time.day().try_into().ok()?,
        time.hour().try_into().ok()?,
        time.minute().try_into().ok()?,
        time.second().try_into().ok()?,
        0,
    )
    .ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

pub(crate) fn each(file: File, password: Option<&str>, wanted: &[&Item], visit: Visit) -> Result<(), Error> {
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(error)?;
    for item in wanted {
        let Some(slot) = item.slot else { continue };
        let mut entry = match (item.encrypted, password) {
            (true, None) => return Err(Error::NeedsPassword),
            (true, Some(password)) => zip.by_index_decrypt(slot, password.as_bytes()),
            (false, _) => zip.by_index(slot),
        }
        .map_err(error)?;
        if visit(item, &mut entry)? == Flow::Stop {
            break;
        }
    }
    Ok(())
}

fn error(error: ZipError) -> Error {
    match error {
        ZipError::Io(error) => super::stream::damaged(error),
        ZipError::InvalidPassword => Error::WrongPassword,
        ZipError::UnsupportedArchive(reason) if reason == ZipError::PASSWORD_REQUIRED => Error::NeedsPassword,
        other => Error::Damaged(other.to_string()),
    }
}
