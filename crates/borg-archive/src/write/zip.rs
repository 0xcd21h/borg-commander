//! Zip: entries the change keeps are copied as they are stored, encrypted ones included, and new
//! entries are compressed with Deflate, encrypted with AES-256 when the archive has a password.

use std::{
    fs::File,
    io::{self, BufReader, Seek, SeekFrom, Write},
    path::Path,
    time::SystemTime,
};

use zip::{AesMode, CompressionMethod, DateTime, ZipArchive, ZipWriter, write::FileOptions};

use super::plan::{Added, Adding, Plan};
use crate::{
    Error,
    meter::{Counted, Meter},
};

/// Entries this large need the zip64 extension.
const ZIP64_THRESHOLD: u64 = 0xFFFF_FFFF;

/// Writes the archive at `real`, changed as `plan` says, into `out`.
pub(crate) fn rewrite(
    real: &Path,
    plan: &Plan,
    password: Option<&str>,
    out: File,
    meter: &Meter<'_, '_>,
) -> Result<File, Error> {
    let source = Counted { inner: BufReader::new(File::open(real)?), meter };
    let mut zip = ZipArchive::new(source).map_err(error)?;
    let mut writer = ZipWriter::new(out);
    for slot in 0..zip.len() {
        let entry = zip.by_index_raw(slot).map_err(error)?;
        match plan.kept.get(&slot) {
            Some(kept) => writer.raw_copy_file_rename(entry, kept.stored()).map_err(error)?,
            None if !plan.known.contains(&slot) => writer.raw_copy_file(entry).map_err(error)?,
            None => continue,
        }
        meter.entry();
    }
    add(&mut writer, plan, password, meter)?;
    writer.finish().map_err(error)
}

/// Adds the plan's new entries to `out`, a copy of the archive.
pub(crate) fn append(mut out: File, plan: &Plan, password: Option<&str>, meter: &Meter<'_, '_>) -> Result<File, Error> {
    out.seek(SeekFrom::Start(0))?;
    let mut writer = ZipWriter::new_append(out).map_err(error)?;
    add(&mut writer, plan, password, meter)?;
    writer.finish().map_err(error)
}

/// Whether the archive in `file` lists.
pub(crate) fn verify(file: &mut File) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut zip = ZipArchive::new(BufReader::new(&mut *file)).map_err(error)?;
    if zip.has_overlapping_files().map_err(error)? {
        return Err(Error::Overlapping);
    }
    Ok(())
}

fn add<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    plan: &Plan,
    password: Option<&str>,
    meter: &Meter<'_, '_>,
) -> Result<(), Error> {
    for added in &plan.added {
        let options = options(added, password);
        match &added.adding {
            Adding::Folder => writer.add_directory(&added.path, options).map_err(error)?,
            Adding::Symlink(target) => writer.add_symlink(&added.path, target, options).map_err(error)?,
            Adding::File { source, size } => {
                writer.start_file(&added.path, options.large_file(*size >= ZIP64_THRESHOLD)).map_err(error)?;
                io::copy(&mut Counted { inner: File::open(source)?, meter }, writer)?;
            }
        }
        meter.entry();
    }
    Ok(())
}

fn options<'k>(added: &Added, password: Option<&'k str>) -> FileOptions<'k, 'static, ()> {
    let options: FileOptions<'k, 'static, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated).unix_permissions(added.mode);
    let options = match added.modified.and_then(dos_time) {
        Some(time) => options.last_modified_time(time),
        None => options,
    };
    match password {
        Some(password) => options.with_aes_encryption(AesMode::Aes256, password),
        None => options,
    }
}

/// A zip time, which is the local time of the machine that makes the archive.
fn dos_time(time: SystemTime) -> Option<DateTime> {
    let zoned = jiff::Timestamp::try_from(time).ok()?.to_zoned(jiff::tz::TimeZone::system());
    DateTime::from_date_and_time(
        zoned.year().try_into().ok()?,
        zoned.month().try_into().ok()?,
        zoned.day().try_into().ok()?,
        zoned.hour().try_into().ok()?,
        zoned.minute().try_into().ok()?,
        zoned.second().try_into().ok()?,
    )
    .ok()
}

fn error(error: zip::result::ZipError) -> Error {
    match error {
        zip::result::ZipError::Io(error) => Error::from(error),
        zip::result::ZipError::InvalidPassword => Error::WrongPassword,
        other => Error::Damaged(other.to_string()),
    }
}
