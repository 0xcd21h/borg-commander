//! What an archive holds: its entries as a tree of folders, with the totals each folder holds.

use std::{collections::HashMap, time::SystemTime};

use crate::{Format, name};

/// Archives with more entries than this are not listed.
pub const ENTRY_LIMIT: usize = 1_000_000;

const FILE_MODE: u32 = 0o644;
const FOLDER_MODE: u32 = 0o755;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Folder,
    Symlink,
    /// A second name for a file earlier in the archive.
    HardLink,
}

/// One entry of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Where it sits inside the archive, such as `docs/guide.md`.
    pub path: String,
    pub kind: ItemKind,
    /// Its unpacked bytes; for a folder, everything beneath it.
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Permission bits.
    pub mode: u32,
    /// A symlink's target as stored, or the archive path a hard link names. Zip and 7z
    /// archives keep a symlink's target as its data, which unpacking reads.
    pub link: Option<String>,
    pub encrypted: bool,
    /// Files, links and folders beneath a folder.
    pub inside: Totals,
    /// The name the archive itself uses, which tools are given back.
    pub(crate) raw: String,
    /// Its place in the archive; `None` for a folder only the paths beneath it imply.
    pub(crate) slot: Option<usize>,
}

/// How many entries and bytes something holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    /// Files and links.
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
}

impl Item {
    pub(crate) fn new(path: String, kind: ItemKind, raw: String, slot: usize) -> Self {
        Self {
            path,
            kind,
            size: 0,
            modified: None,
            mode: if kind == ItemKind::Folder { FOLDER_MODE } else { FILE_MODE },
            link: None,
            encrypted: false,
            inside: Totals::default(),
            raw,
            slot: Some(slot),
        }
    }

    fn implied(path: String) -> Self {
        Self { slot: None, ..Self::new(path, ItemKind::Folder, String::new(), 0) }
    }

    pub fn is_folder(&self) -> bool {
        self.kind == ItemKind::Folder
    }

    pub fn name(&self) -> &str {
        name::base(&self.path)
    }

    /// Whether unpacking reads this entry's data from the archive.
    pub(crate) fn carries_data(&self) -> bool {
        self.kind == ItemKind::File || (self.kind == ItemKind::Symlink && self.link.is_none())
    }
}

/// An archive's entries, ready to list folder by folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub format: Format,
    /// The archive file's own size.
    pub bytes: u64,
    /// Entries whose names lead outside the archive's folder, which Borg leaves out.
    pub left_out: usize,
    items: Vec<Item>,
    paths: HashMap<String, usize>,
    children: HashMap<String, Vec<usize>>,
    /// Places of entries a later entry of the same path hides.
    shadowed: Vec<usize>,
}

impl Index {
    /// Builds the tree from the archive's entries in archive order. A later entry with the same
    /// path replaces an earlier one, as unpacking would; folders the paths imply are added.
    pub(crate) fn new(format: Format, bytes: u64, entries: Vec<Item>, left_out: usize) -> Self {
        let mut index = Self {
            format,
            bytes,
            left_out,
            items: Vec::new(),
            paths: HashMap::new(),
            children: HashMap::new(),
            shadowed: Vec::new(),
        };
        for item in entries {
            index.imply(name::parent(&item.path).to_owned());
            index.insert(item);
        }
        index.total();
        index
    }

    fn insert(&mut self, item: Item) {
        match self.paths.get(&item.path) {
            Some(&at) if !(self.items[at].is_folder() && item.is_folder()) || self.items[at].slot.is_none() => {
                self.shadowed.extend(self.items[at].slot);
                self.items[at] = item;
            }
            Some(_) => self.shadowed.extend(item.slot),
            None => {
                let at = self.items.len();
                self.children.entry(name::parent(&item.path).to_owned()).or_default().push(at);
                self.paths.insert(item.path.clone(), at);
                self.items.push(item);
            }
        }
    }

    fn imply(&mut self, folder: String) {
        if folder.is_empty() || self.paths.contains_key(&folder) {
            return;
        }
        self.imply(name::parent(&folder).to_owned());
        self.insert(Item::implied(folder));
    }

    /// Adds up what each folder holds, deepest folders first.
    fn total(&mut self) {
        let mut folders: Vec<usize> = (0..self.items.len()).filter(|&at| self.items[at].is_folder()).collect();
        folders.sort_by_key(|&at| std::cmp::Reverse(self.items[at].path.matches('/').count()));
        for at in folders {
            let inside = self.children.get(&self.items[at].path).map_or_else(Totals::default, |children| {
                children.iter().map(|&child| self.contribution(child)).fold(Totals::default(), add)
            });
            let folder = &mut self.items[at];
            folder.inside = inside;
            folder.size = inside.bytes;
        }
    }

    fn contribution(&self, at: usize) -> Totals {
        let item = &self.items[at];
        match item.kind {
            ItemKind::Folder => add(item.inside, Totals { folders: 1, ..Totals::default() }),
            _ => Totals { files: 1, bytes: item.size, folders: 0 },
        }
    }

    /// The entries directly inside `folder`, `""` for the top; `None` when there is no such folder.
    pub fn list(&self, folder: &str) -> Option<impl Iterator<Item = &Item>> {
        if !folder.is_empty() && !self.item(folder).is_some_and(Item::is_folder) {
            return None;
        }
        let children = self.children.get(folder).map_or(&[][..], Vec::as_slice);
        Some(children.iter().map(|&at| &self.items[at]))
    }

    pub fn item(&self, path: &str) -> Option<&Item> {
        self.paths.get(path).map(|&at| &self.items[at])
    }

    /// Every entry, in archive order, folders the paths imply among them.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }

    /// Places of entries that later entries of the same path hide.
    pub(crate) fn shadowed(&self) -> &[usize] {
        &self.shadowed
    }

    /// Everything the archive holds.
    pub fn totals(&self) -> Totals {
        self.children.get("").into_iter().flatten().map(|&at| self.contribution(at)).fold(Totals::default(), add)
    }

    /// Whether any entry needs a password.
    pub fn encrypted(&self) -> bool {
        self.items.iter().any(|item| item.encrypted)
    }

    pub(crate) fn smallest_encrypted(&self) -> Option<&Item> {
        self.items.iter().filter(|item| item.encrypted && item.kind == ItemKind::File).min_by_key(|item| item.size)
    }

    /// The entries at `paths` and everything beneath them, each once: folders by depth, then
    /// the rest in archive order, the order the archive is read in.
    pub fn selection(&self, paths: &[String]) -> Vec<&Item> {
        let wanted: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();
        let chosen_path = |path: &str| {
            wanted.contains("")
                || std::iter::successors(Some(path), |path| Some(name::parent(path)))
                    .take_while(|path| !path.is_empty())
                    .any(|path| wanted.contains(path))
        };
        let mut chosen: Vec<&Item> = self.items.iter().filter(|item| chosen_path(&item.path)).collect();
        chosen.sort_by_key(|item| match item.kind {
            ItemKind::Folder => (0, item.path.matches('/').count()),
            _ => (1, item.slot.unwrap_or_default()),
        });
        chosen
    }

    /// What `selection` holds altogether.
    pub fn selected_totals(selection: &[&Item]) -> Totals {
        selection.iter().fold(Totals::default(), |totals, item| match item.kind {
            ItemKind::Folder => Totals { folders: totals.folders + 1, ..totals },
            _ => Totals { files: totals.files + 1, bytes: totals.bytes + item.size, ..totals },
        })
    }
}

fn add(a: Totals, b: Totals) -> Totals {
    Totals { files: a.files + b.files, folders: a.folders + b.folders, bytes: a.bytes.saturating_add(b.bytes) }
}
