//! Reading and writing files for the built-in editor.

use std::{
    fs, io,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use borg_core::{EditorFile, FileStamp, OpenError, SaveError};

/// Larger files open in the viewer, which reads them a page at a time.
const EDIT_LIMIT: u64 = 32 * 1024 * 1024;
const NEW_FILE_MODE: u32 = 0o644;

fn stamp(meta: &fs::Metadata) -> FileStamp {
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
/// a file with other hard links is written in place, so they all see the change.
pub fn write(path: &Path, bytes: &[u8], expected: FileStamp, force: bool) -> Result<FileStamp, SaveError> {
    let failed = |error: io::Error| SaveError::Failed(error.to_string());
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let current = fs::metadata(&target).ok();
    if !force && current.as_ref().map(stamp) != Some(expected) {
        return Err(SaveError::Changed);
    }
    if current.as_ref().is_some_and(|meta| meta.nlink() > 1) {
        let mut file = fs::OpenOptions::new().write(true).truncate(true).open(&target).map_err(failed)?;
        file.write_all(bytes).and_then(|()| file.sync_all()).map_err(failed)?;
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
    }
}
