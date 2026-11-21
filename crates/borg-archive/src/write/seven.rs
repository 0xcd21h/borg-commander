//! 7z: a block whose entries all stay is copied as it is stored, renamed entries included. A
//! block that loses an entry is unpacked into a spool file and packed again without it, and new
//! entries go into one new solid block, encrypted with AES-256 when the archive has a password.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    os::unix::fs::FileExt,
    path::Path,
};

use sevenz_rust2::{
    Archive, ArchiveEntry, ArchiveWriter, BlockDecoder, EncoderConfiguration, EncoderMethod, NtTime, Password,
    SourceReader, encoder_options::AesEncoderOptions,
};

use super::plan::{Added, Adding, Plan};
use crate::{
    Error,
    meter::{Counted, Meter},
};

const UNIX_EXTENSION: u32 = 0x8000;
const UNIX_MODE_SHIFT: u32 = 16;
const ATTRIBUTE_DIRECTORY: u32 = 0x10;
const ATTRIBUTE_ARCHIVE: u32 = 0x20;
const REGULAR_TYPE: u32 = 0o100000;
const FOLDER_TYPE: u32 = 0o040000;
const SYMLINK_TYPE: u32 = 0o120000;
const THREADS: u32 = 1;

type Reader<'r> = Box<dyn Read + 'r>;

/// Writes the archive at `real`, changed as `plan` says, into `out`.
pub(crate) fn rewrite(
    real: &Path,
    plan: &Plan,
    password: Option<&str>,
    out: File,
    meter: &Meter<'_, '_>,
) -> Result<File, Error> {
    let given = password.map_or_else(Password::empty, Password::new);
    let mut source = Counted { inner: BufReader::new(File::open(real)?), meter };
    let archive = Archive::read(&mut source, &given).map_err(error)?;
    let mut writer = ArchiveWriter::new(out).map_err(error)?;
    if password.is_some() {
        writer.set_content_methods(vec![
            EncoderConfiguration::from(AesEncoderOptions::new(given.clone())),
            EncoderMethod::LZMA2.into(),
        ]);
        writer.set_encrypt_header(header_encrypted(real)?);
    }
    let names = names(&archive, plan);
    let spool_folder = real.parent().unwrap_or(real);
    for (block, kind) in blocks(&archive, plan, &names).into_iter().enumerate() {
        match kind {
            Block::Dropped => continue,
            Block::Raw => {
                writer
                    .push_raw_block(&archive, block, &mut source, |entry| {
                        names.get(entry.name.as_str()).cloned().unwrap_or_else(|| entry.name.clone())
                    })
                    .map_err(error)?;
            }
            Block::Repacked => {
                let spool = tempfile::tempfile_in(spool_folder)?;
                repack(&mut writer, &archive, block, &given, &mut source, plan, &spool)?;
            }
        }
        for _ in members(&archive, block) {
            meter.entry();
        }
    }
    for (slot, entry) in archive.files.iter().enumerate() {
        if archive.stream_map.file_block_index[slot].is_none() && !entry.is_anti_item && is_kept(plan, slot) {
            let name = stored(plan, slot, entry);
            writer.push_archive_entry::<File>(ArchiveEntry { name, ..entry.clone() }, None).map_err(error)?;
            meter.entry();
        }
    }
    add(&mut writer, plan, meter)?;
    Ok(writer.finish()?)
}

/// The unpacked bytes of the blocks the change packs again.
pub(crate) fn touched(real: &Path, plan: &Plan, password: Option<&str>) -> Result<u64, Error> {
    let given = password.map_or_else(Password::empty, Password::new);
    let archive = Archive::read(&mut BufReader::new(File::open(real)?), &given).map_err(error)?;
    let names = names(&archive, plan);
    let repacked = blocks(&archive, plan, &names).into_iter().enumerate().filter(|(_, kind)| *kind == Block::Repacked);
    Ok(repacked.flat_map(|(block, _)| members(&archive, block)).map(|slot| archive.files[slot].size).sum())
}

/// What becomes of a block of the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// The change keeps none of its entries.
    Dropped,
    /// Copied as it is stored.
    Raw,
    /// Unpacked and packed again with the entries the change keeps.
    Repacked,
}

fn blocks(archive: &Archive, plan: &Plan, names: &HashMap<String, String>) -> Vec<Block> {
    (0..archive.blocks.len())
        .map(|block| {
            let members = members(archive, block);
            let kept = members.iter().filter(|slot| is_kept(plan, **slot)).count();
            let unique = members.iter().all(|slot| names.contains_key(archive.files[*slot].name.as_str()));
            match kept {
                0 => Block::Dropped,
                _ if kept == members.len() && unique => Block::Raw,
                _ => Block::Repacked,
            }
        })
        .collect()
}

/// Whether the archive in `file` lists with `password`.
pub(crate) fn verify(file: &mut File, password: Option<&str>) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0))?;
    let given = password.map_or_else(Password::empty, Password::new);
    Archive::read(&mut BufReader::new(&mut *file), &given).map(drop).map_err(error)
}

/// Whether the archive hides its list of entries behind its password too.
fn header_encrypted(real: &Path) -> Result<bool, Error> {
    match Archive::read(&mut BufReader::new(File::open(real)?), &Password::empty()) {
        Err(sevenz_rust2::Error::PasswordRequired) => Ok(true),
        _ => Ok(false),
    }
}

/// An entry stays when the plan keeps it, or when the listing left it out.
fn is_kept(plan: &Plan, slot: usize) -> bool {
    plan.kept.contains_key(&slot) || !plan.known.contains(&slot)
}

fn stored(plan: &Plan, slot: usize, entry: &ArchiveEntry) -> String {
    plan.kept.get(&slot).map_or_else(|| entry.name.clone(), super::plan::Kept::stored)
}

/// The name each kept entry gets, by the name it has; a name two entries share is left out, so
/// their blocks are packed again entry by entry.
fn names(archive: &Archive, plan: &Plan) -> HashMap<String, String> {
    let mut names: HashMap<String, Option<String>> = HashMap::new();
    for (slot, entry) in archive.files.iter().enumerate().filter(|(slot, _)| is_kept(plan, *slot)) {
        names
            .entry(entry.name.clone())
            .and_modify(|name| *name = None)
            .or_insert_with(|| Some(stored(plan, slot, entry)));
    }
    names.into_iter().filter_map(|(old, new)| new.map(|new| (old, new))).collect()
}

/// The entries with data in `block`.
fn members(archive: &Archive, block: usize) -> Vec<usize> {
    let map = &archive.stream_map;
    (0..archive.files.len())
        .filter(|slot| map.file_block_index[*slot] == Some(block) && archive.files[*slot].has_stream)
        .collect()
}

/// Unpacks the entries of `block` the plan keeps into `spool` and packs them again.
fn repack<R: Read + Seek>(
    writer: &mut ArchiveWriter<File>,
    archive: &Archive,
    block: usize,
    given: &Password,
    source: &mut R,
    plan: &Plan,
    spool: &File,
) -> Result<(), Error> {
    let first = archive.stream_map.block_first_file_index[block];
    let mut slot = first;
    let mut at = 0;
    let mut kept: Vec<(ArchiveEntry, u64, u64)> = Vec::new();
    let mut failure = None;
    let decoder = BlockDecoder::new(THREADS, block, archive, given, source);
    decoder
        .for_each_entries(&mut |entry, data| {
            let this = slot;
            slot += 1;
            if !entry.has_stream || !is_kept(plan, this) {
                return Ok(true);
            }
            let mut written = 0;
            let mut buffer = [0; 1 << 16];
            loop {
                let read = data.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                if let Err(error) = spool.write_all_at(&buffer[..read], at + written) {
                    failure = Some(error);
                    return Ok(false);
                }
                written += read as u64;
            }
            kept.push((ArchiveEntry { name: stored(plan, this, entry), ..entry.clone() }, at, written));
            at += written;
            Ok(true)
        })
        .map_err(error)?;
    if let Some(failure) = failure {
        return Err(failure.into());
    }
    let (entries, readers): (Vec<ArchiveEntry>, Vec<SourceReader<Reader<'_>>>) = kept
        .into_iter()
        .map(|(entry, at, length)| {
            let reader: Reader<'_> = Box::new(Slice { file: spool, at, left: length });
            (entry, SourceReader::new(reader))
        })
        .unzip();
    if !entries.is_empty() {
        writer.push_archive_entries(entries, readers).map_err(error)?;
    }
    Ok(())
}

/// Part of a file, read by position.
struct Slice<'f> {
    file: &'f File,
    at: u64,
    left: u64,
}

impl Read for Slice<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let wanted = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let read = self.file.read_at(&mut buf[..wanted], self.at)?;
        self.at += read as u64;
        self.left -= read as u64;
        Ok(read)
    }
}

fn add(writer: &mut ArchiveWriter<File>, plan: &Plan, meter: &Meter<'_, '_>) -> Result<(), Error> {
    let mut entries = Vec::new();
    let mut readers: Vec<SourceReader<Reader<'_>>> = Vec::new();
    for added in &plan.added {
        let entry = entry(added);
        let reader: Option<Reader<'_>> = match &added.adding {
            Adding::Folder => None,
            Adding::File { source, .. } => {
                let file = File::open(source)?;
                let empty = file.metadata()?.len() == 0;
                (!empty).then(|| Box::new(Counted { inner: file, meter }) as Reader<'_>)
            }
            Adding::Symlink(target) => Some(Box::new(io::Cursor::new(target.clone().into_bytes()))),
        };
        match reader {
            Some(reader) => {
                entries.push(entry);
                readers.push(SourceReader::new(reader));
            }
            None => {
                writer.push_archive_entry::<File>(entry, None).map_err(error)?;
            }
        }
        meter.entry();
    }
    if !entries.is_empty() {
        writer.push_archive_entries(entries, readers).map_err(error)?;
    }
    Ok(())
}

fn entry(added: &Added) -> ArchiveEntry {
    let (mut entry, kind, attribute) = match added.adding {
        Adding::Folder => (ArchiveEntry::new_directory(&added.path), FOLDER_TYPE, ATTRIBUTE_DIRECTORY),
        Adding::Symlink(_) => (ArchiveEntry::new_file(&added.path), SYMLINK_TYPE, ATTRIBUTE_ARCHIVE),
        Adding::File { .. } => (ArchiveEntry::new_file(&added.path), REGULAR_TYPE, ATTRIBUTE_ARCHIVE),
    };
    entry.has_windows_attributes = true;
    entry.windows_attributes = UNIX_EXTENSION | ((kind | added.mode) << UNIX_MODE_SHIFT) | attribute;
    if let Some(time) = added.modified.and_then(|time| NtTime::try_from(time).ok()) {
        entry.last_modified_date = time;
        entry.has_last_modified_date = true;
    }
    entry
}

fn error(error: sevenz_rust2::Error) -> Error {
    match error {
        sevenz_rust2::Error::PasswordRequired => Error::NeedsPassword,
        sevenz_rust2::Error::MaybeBadPassword(_) => Error::WrongPassword,
        sevenz_rust2::Error::Io(error, _) | sevenz_rust2::Error::FileOpen(error, _) => Error::from(error),
        other => Error::Damaged(other.to_string()),
    }
}
