//! Reading and writing files for the built-in editor.

use std::{
    fs, io,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use borg_core::{EditorFile, FileStamp, OpenError, SaveError};

/// Larger files open in the viewer, which reads them a page at a time.
const EDIT_LIMIT: u64 = 32 * 1024 * 1024;
const NEW_FILE_MODE: u32 = 0o644;
const BACKUP_MODE: u32 = 0o600;
const BACKUP_TAG: &str = "borg-kept";
const BACKUP_TRIES: u32 = 100;

type Fill<'a> = &'a mut dyn FnMut(&mut fs::File, &[u8]) -> io::Result<()>;

pub(crate) fn stamp(meta: &fs::Metadata) -> FileStamp {
    FileStamp { len: meta.len(), modified: meta.modified().ok(), inode: meta.ino() }
}

pub fn read(path: &Path) -> Result<EditorFile, OpenError> {
    let failed = |error: io::Error| OpenError::Failed(path.into(), error.to_string());
    let meta = fs::metadata(path).map_err(failed)?;
    if meta.len() > EDIT_LIMIT {
        return Err(OpenError::TooLarge(path.into()));
    }
    let bytes = fs::read(path).map_err(failed)?;
    if bytes.contains(&0) {
        return Err(OpenError::NotText(path.into()));
    }
    let text = String::from_utf8(bytes).map_err(|_| OpenError::NotText(path.into()))?;
    Ok(EditorFile { path: path.into(), text, stamp: stamp(&meta) })
}

/// Writes `bytes` to the file `path` names, if it is still the one read as `expected` or the
/// write is `force`d. A symlink keeps pointing at the file, which keeps its permissions and,
/// where allowed, its owner. A new file replaces the old one only once completely written;
/// a file with other hard links is written in place, so they all see the change, with its
/// old text kept beside it until the new text is on the disk.
pub fn write(path: &Path, bytes: &[u8], expected: FileStamp, force: bool) -> Result<FileStamp, SaveError> {
    write_with(path, bytes, expected, force, &mut |file, bytes| file.write_all(bytes))
}

fn write_with(
    path: &Path,
    bytes: &[u8],
    expected: FileStamp,
    force: bool,
    fill: Fill<'_>,
) -> Result<FileStamp, SaveError> {
    let failed = |error: io::Error| SaveError::Failed(error.to_string());
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let current = fs::metadata(&target).ok();
    if !force && current.as_ref().map(stamp) != Some(expected) {
        return Err(SaveError::Changed);
    }
    if current.as_ref().is_some_and(|meta| meta.nlink() > 1) {
        in_place(&target, bytes, fill)?;
    } else {
        let dir = target.parent().ok_or_else(|| SaveError::Failed("no folder to write in".into()))?;
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        let staging = dir.join(format!(".{name}.borg-save-{}", std::process::id()));
        let mode = current.as_ref().map_or(NEW_FILE_MODE, |meta| meta.permissions().mode() & 0o7777);
        let _ = fs::remove_file(&staging);
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&staging)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.set_permissions(fs::Permissions::from_mode(mode))?;
                file.sync_all()
            })
            .and_then(|()| {
                if let Some(meta) = &current {
                    // Only an administrator may give a file away; anyone else keeps their own.
                    let _ = std::os::unix::fs::chown(&staging, Some(meta.uid()), Some(meta.gid()));
                }
                fs::rename(&staging, &target)
            });
        if let Err(error) = written {
            let _ = fs::remove_file(&staging);
            return Err(failed(error));
        }
    }
    fs::metadata(&target).map(|meta| stamp(&meta)).map_err(failed)
}

fn in_place(target: &Path, bytes: &[u8], fill: Fill<'_>) -> Result<(), SaveError> {
    let failed = |error: io::Error| SaveError::Failed(error.to_string());
    let old = fs::read(target).map_err(failed)?;
    let backup = keep(target, &old, fill).map_err(failed)?;
    let Err(error) = overwrite(target, bytes, fill) else {
        let _ = fs::remove_file(&backup);
        return Ok(());
    };
    if overwrite(target, &old, fill).is_err() {
        return Err(SaveError::KeptOld { error: error.to_string(), backup });
    }
    let _ = fs::remove_file(&backup);
    Err(failed(error))
}

fn keep(target: &Path, old: &[u8], fill: Fill<'_>) -> io::Result<PathBuf> {
    let dir = target.parent().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let base = format!(".{name}.{BACKUP_TAG}-{}", std::process::id());
    for attempt in 0..BACKUP_TRIES {
        let backup = dir.join(if attempt == 0 { base.clone() } else { format!("{base}-{attempt}") });
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).mode(BACKUP_MODE).open(&backup) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            opened => opened?,
        };
        let written =
            fill(&mut file, old).and_then(|()| file.sync_all()).and_then(|()| fs::File::open(dir)?.sync_all());
        if let Err(error) = written {
            drop(file);
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        return Ok(backup);
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

fn overwrite(target: &Path, bytes: &[u8], fill: Fill<'_>) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).truncate(true).open(target)?;
    fill(&mut file, bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_replace_atomically_keep_modes_and_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("script.sh");
        fs::write(&file, "echo one\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).unwrap();
        let link = dir.path().join("link.sh");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let opened = read(&link).unwrap();
        let stamp = write(&link, b"echo two\n", opened.stamp, false).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");
        assert_eq!(fs::read_to_string(&file).unwrap(), "echo two\n");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o750);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2, "no staging file is left behind");
        assert_eq!(write(&file, b"stale", opened.stamp, false), Err(SaveError::Changed));
        assert!(write(&file, b"echo three\n", stamp, false).is_ok());
    }

    #[test]
    fn hard_linked_files_are_written_in_place_and_binary_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "old").unwrap();
        fs::hard_link(&file, dir.path().join("b.txt")).unwrap();
        let opened = read(&file).unwrap();
        write(&file, b"new", opened.stamp, false).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("b.txt")).unwrap(), "new");
        fs::write(dir.path().join("bin"), [0x7f, 0, 1]).unwrap();
        assert!(matches!(read(&dir.path().join("bin")), Err(OpenError::NotText(_))));
        fs::write(dir.path().join("latin1"), [b'c', 0xe9]).unwrap();
        assert!(matches!(read(&dir.path().join("latin1")), Err(OpenError::NotText(_))));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4, "no backup is left behind");
    }

    fn linked() -> (tempfile::TempDir, PathBuf, FileStamp) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        fs::write(&file, "old text").unwrap();
        fs::hard_link(&file, dir.path().join("twin.txt")).unwrap();
        let stamp = read(&file).unwrap().stamp;
        (dir, file, stamp)
    }

    fn failing_at(calls: &'static [u32]) -> impl FnMut(&mut fs::File, &[u8]) -> io::Result<()> {
        let mut call = 0;
        move |file, bytes| {
            call += 1;
            if !calls.contains(&call) {
                return file.write_all(bytes);
            }
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(io::Error::other("the disk is full"))
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_hard_linked_file_that_fails_halfway_gets_its_old_text_back() {
        let (dir, file, stamp) = linked();
        let result = write_with(&file, b"new text", stamp, false, &mut failing_at(&[2]));
        assert_eq!(result, Err(SaveError::Failed("the disk is full".into())));
        assert_eq!(fs::read_to_string(dir.path().join("twin.txt")).unwrap(), "old text");
        assert_eq!(names(dir.path()), ["notes.txt", "twin.txt"]);
    }

    #[test]
    fn a_hard_linked_file_stays_untouched_when_its_backup_cannot_be_written() {
        let (dir, file, stamp) = linked();
        let result = write_with(&file, b"new text", stamp, false, &mut failing_at(&[1]));
        assert_eq!(result, Err(SaveError::Failed("the disk is full".into())));
        assert_eq!(fs::read_to_string(&file).unwrap(), "old text");
        assert_eq!(names(dir.path()), ["notes.txt", "twin.txt"]);
    }

    #[test]
    fn a_hard_linked_file_that_cannot_be_restored_keeps_its_backup() {
        let (dir, file, stamp) = linked();
        let result = write_with(&file, b"new text", stamp, false, &mut failing_at(&[2, 3]));
        let Err(SaveError::KeptOld { error, backup }) = result else { panic!("{result:?}") };
        assert_eq!(error, "the disk is full");
        let folder = fs::canonicalize(dir.path()).unwrap();
        assert_eq!(backup, folder.join(format!(".notes.txt.{BACKUP_TAG}-{}", std::process::id())));
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old text");
        assert_eq!(fs::metadata(&backup).unwrap().permissions().mode() & 0o777, BACKUP_MODE);
        let stamp = read(&file).unwrap().stamp;
        write_with(&file, b"new text", stamp, false, &mut failing_at(&[])).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("twin.txt")).unwrap(), "new text");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old text", "a later save leaves the kept text alone");
        assert_eq!(names(dir.path()).len(), 3);
    }
}
