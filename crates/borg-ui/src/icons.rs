use borg_vfs::{Entry, EntryKind};

use crate::kinds::{self, Kind};

const PARENT: char = '\u{f148}';
const FOLDER: char = '\u{f07b}';
const FOLDER_LINK: char = '\u{f482}';
const FILE: char = '\u{f15b}';
const FILE_LINK: char = '\u{f481}';
const EXECUTABLE: char = '\u{f489}';
const SPECIAL: char = '\u{f128}';
const CODE: char = '\u{f121}';
const IMAGE: char = '\u{f1c5}';
const ARCHIVE: char = '\u{f1c6}';
const DOCUMENT: char = '\u{f15c}';
const MEDIA: char = '\u{f1c8}';
const CONFIG: char = '\u{e615}';
const EXTENSION_SEPARATOR: char = '.';

const BY_NAME: [(&str, char); 10] = [
    (".git", '\u{e5fb}'),
    (".github", '\u{e5fd}'),
    ("node_modules", '\u{e5fa}'),
    (".gitignore", '\u{e702}'),
    (".gitattributes", '\u{e702}'),
    ("dockerfile", '\u{f308}'),
    ("makefile", '\u{e779}'),
    ("cargo.toml", '\u{e7a8}'),
    ("cargo.lock", '\u{e7a8}'),
    ("license", '\u{f0219}'),
];

const BY_EXTENSION: [(&str, char); 30] = [
    ("rs", '\u{e7a8}'),
    ("py", '\u{e606}'),
    ("js", '\u{e74e}'),
    ("mjs", '\u{e74e}'),
    ("ts", '\u{e628}'),
    ("tsx", '\u{e7ba}'),
    ("jsx", '\u{e7ba}'),
    ("go", '\u{e627}'),
    ("c", '\u{e61e}'),
    ("h", '\u{e61e}'),
    ("cpp", '\u{e61d}'),
    ("java", '\u{e738}'),
    ("kt", '\u{e634}'),
    ("swift", '\u{e755}'),
    ("rb", '\u{e739}'),
    ("php", '\u{e73d}'),
    ("lua", '\u{e620}'),
    ("html", '\u{e736}'),
    ("css", '\u{e749}'),
    ("sh", '\u{f489}'),
    ("zsh", '\u{f489}'),
    ("fish", '\u{f489}'),
    ("md", '\u{e73e}'),
    ("json", '\u{e60b}'),
    ("toml", '\u{e615}'),
    ("yaml", '\u{e615}'),
    ("yml", '\u{e615}'),
    ("lock", '\u{f023}'),
    ("pdf", '\u{f1c1}'),
    ("log", '\u{f18d}'),
];

pub fn parent() -> char {
    PARENT
}

fn lookup<const N: usize>(table: &[(&str, char); N], key: &str) -> Option<char> {
    table.iter().find(|(name, _)| *name == key).map(|(_, icon)| *icon)
}

fn by_kind(kind: Kind) -> char {
    match kind {
        Kind::Code => CODE,
        Kind::Image => IMAGE,
        Kind::Archive => ARCHIVE,
        Kind::Document => DOCUMENT,
        Kind::Media => MEDIA,
        Kind::Config => CONFIG,
    }
}

pub fn icon(entry: &Entry) -> char {
    let lower = entry.name.to_ascii_lowercase();
    let named = lookup(&BY_NAME, &lower);
    match entry.kind {
        EntryKind::Dir => named.unwrap_or(FOLDER),
        EntryKind::Symlink { to_dir: true } => FOLDER_LINK,
        EntryKind::Symlink { to_dir: false } => FILE_LINK,
        EntryKind::Other => SPECIAL,
        EntryKind::File => named
            .or_else(|| lower.rsplit_once(EXTENSION_SEPARATOR).and_then(|(_, ext)| lookup(&BY_EXTENSION, ext)))
            .or_else(|| entry.executable.then_some(EXECUTABLE))
            .or_else(|| kinds::classify(&entry.name).map(by_kind))
            .unwrap_or(FILE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, executable: bool) -> Entry {
        Entry { name: name.into(), kind, size: 0, modified: None, executable, permissions: 0 }
    }

    #[test]
    fn picks_icons_by_name_extension_and_kind() {
        assert_eq!(icon(&entry("main.rs", EntryKind::File, false)), '\u{e7a8}');
        assert_eq!(icon(&entry("Cargo.toml", EntryKind::File, false)), '\u{e7a8}');
        assert_eq!(icon(&entry(".git", EntryKind::Dir, false)), '\u{e5fb}');
        assert_eq!(icon(&entry("src", EntryKind::Dir, false)), FOLDER);
        assert_eq!(icon(&entry("photo.webp", EntryKind::File, false)), IMAGE);
        assert_eq!(icon(&entry("run", EntryKind::File, true)), EXECUTABLE);
        assert_eq!(icon(&entry("notes", EntryKind::File, false)), FILE);
        assert_eq!(icon(&entry("latest", EntryKind::Symlink { to_dir: false }, false)), FILE_LINK);
    }
}
