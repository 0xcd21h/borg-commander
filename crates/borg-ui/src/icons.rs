use std::{collections::HashMap, str::FromStr, sync::OnceLock};

use borg_vfs::{Entry, EntryKind};
use ratatui::style::Color;
use serde::Deserialize;

const TABLE: &str = include_str!("../icons/devicons.toml");
const PARENT: &str = "\u{f148}";
const FOLDER: &str = "\u{f07b}";
const FOLDER_LINK: &str = "\u{f482}";
const FILE: &str = "\u{f15b}";
const FILE_LINK: &str = "\u{f481}";
const EXECUTABLE: &str = "\u{f489}";
const SPECIAL: &str = "\u{f128}";
const EXTENSION_SEPARATOR: char = '.';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon {
    pub glyph: &'static str,
    pub color: Option<Color>,
}

impl Icon {
    const fn plain(glyph: &'static str) -> Self {
        Self { glyph, color: None }
    }
}

#[derive(Deserialize)]
struct Rule {
    name: String,
    text: String,
    fg: String,
}

#[derive(Default, Deserialize)]
struct Table {
    dirs: Vec<Rule>,
    files: Vec<Rule>,
    exts: Vec<Rule>,
}

struct Stored {
    glyph: String,
    color: Option<Color>,
}

impl Stored {
    fn icon(&'static self) -> Icon {
        Icon { glyph: &self.glyph, color: self.color }
    }
}

struct IconSet {
    dirs: HashMap<String, Stored>,
    files: HashMap<String, Stored>,
    exts: HashMap<String, Stored>,
}

fn index(rules: Vec<Rule>) -> HashMap<String, Stored> {
    rules
        .into_iter()
        .map(|rule| (rule.name.to_lowercase(), Stored { glyph: rule.text, color: Color::from_str(&rule.fg).ok() }))
        .collect()
}

fn set() -> &'static IconSet {
    static SET: OnceLock<IconSet> = OnceLock::new();
    SET.get_or_init(|| {
        let table: Table = toml::from_str(TABLE).unwrap_or_default();
        IconSet { dirs: index(table.dirs), files: index(table.files), exts: index(table.exts) }
    })
}

fn by_extension(set: &'static IconSet, name: &str) -> Option<Icon> {
    name.char_indices()
        .skip(1)
        .filter(|(_, c)| *c == EXTENSION_SEPARATOR)
        .find_map(|(at, _)| set.exts.get(&name[at + EXTENSION_SEPARATOR.len_utf8()..]).map(Stored::icon))
}

pub fn parent() -> Icon {
    Icon::plain(PARENT)
}

pub fn icon(entry: &Entry) -> Icon {
    let set = set();
    let name = entry.name.to_lowercase();
    match entry.kind {
        EntryKind::Dir => set.dirs.get(&name).map_or(Icon::plain(FOLDER), Stored::icon),
        EntryKind::Symlink { to_dir: true } => Icon::plain(FOLDER_LINK),
        EntryKind::Symlink { to_dir: false } => Icon::plain(FILE_LINK),
        EntryKind::Other => Icon::plain(SPECIAL),
        EntryKind::File => set
            .files
            .get(&name)
            .map(Stored::icon)
            .or_else(|| by_extension(set, &name))
            .unwrap_or(Icon::plain(if entry.executable { EXECUTABLE } else { FILE })),
    }
}

pub fn portable(entry: &Entry) -> Icon {
    Icon::plain(match entry.kind {
        EntryKind::Dir => "/",
        EntryKind::Symlink { .. } => "@",
        EntryKind::Other => "?",
        EntryKind::File if entry.executable => "*",
        EntryKind::File => "·",
    })
}

pub fn portable_parent() -> Icon {
    Icon::plain("↑")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, executable: bool) -> Entry {
        Entry { name: name.into(), kind, size: 0, modified: None, executable, permissions: 0 }
    }

    #[test]
    fn the_embedded_table_loads() {
        let set = set();
        assert!(set.dirs.len() > 10 && set.files.len() > 100 && set.exts.len() > 100);
    }

    #[test]
    fn icons_carry_their_own_colours() {
        let rust = icon(&entry("main.rs", EntryKind::File, false));
        assert_eq!(rust.color, Some(Color::Rgb(0xde, 0xa5, 0x84)));
        let typings = icon(&entry("next-env.d.ts", EntryKind::File, false));
        assert_ne!(typings, icon(&entry("app.ts", EntryKind::File, false)));
        assert_eq!(
            icon(&entry("Dockerfile", EntryKind::File, false)),
            icon(&entry("dockerfile", EntryKind::File, false))
        );
        assert_eq!(icon(&entry(".git", EntryKind::Dir, false)).color, Some(Color::Rgb(0x00, 0xbc, 0xd4)));
        assert_eq!(icon(&entry("src", EntryKind::Dir, false)), Icon::plain(FOLDER));
        assert_eq!(icon(&entry("run", EntryKind::File, true)), Icon::plain(EXECUTABLE));
        assert_eq!(icon(&entry("notes", EntryKind::File, false)), Icon::plain(FILE));
    }
}
