//! The names of a folder's subfolders, for the picker above the command line.

use std::{fs, path::Path};

/// Subfolders read at most, so a huge folder answers quickly.
const LIMIT: usize = 10_000;

/// The names of the folders in `dir`, links to folders included, or none when it cannot be read.
/// Names that are not UTF-8 are left out, since the picker types them.
pub fn read(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| match entry.file_type() {
            Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .take(LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_and_links_to_them_but_no_files() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("src"))?;
        fs::create_dir(root.path().join(".git"))?;
        fs::write(root.path().join("README.md"), "")?;
        std::os::unix::fs::symlink(root.path().join("src"), root.path().join("source"))?;
        std::os::unix::fs::symlink(root.path().join("README.md"), root.path().join("readme"))?;
        let mut names = read(root.path());
        names.sort();
        assert_eq!(names, [".git", "source", "src"]);
        assert!(read(&root.path().join("missing")).is_empty());
        Ok(())
    }
}
