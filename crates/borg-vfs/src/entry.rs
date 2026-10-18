use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink { to_dir: bool },
    Other,
}

impl EntryKind {
    pub fn is_dir(self) -> bool {
        matches!(self, Self::Dir | Self::Symlink { to_dir: true })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub executable: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }

    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}
