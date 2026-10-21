use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderUsage {
    pub direct_bytes: u64,
    pub direct_files: u64,
    pub measured: Option<SystemTime>,
    pub cached: bool,
    pub files: u64,
    pub folders: u64,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub usage: Option<FolderUsage>,
    pub source: Option<PathBuf>,
    pub missing: bool,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub executable: bool,
    pub permissions: u32,
}

impl Entry {
    pub fn identity(&self) -> &Path {
        self.source.as_deref().unwrap_or_else(|| Path::new(&self.name))
    }

    pub fn path(&self, dir: &Path) -> PathBuf {
        self.source.clone().unwrap_or_else(|| dir.join(&self.name))
    }

    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            source: Some(path),
            missing: true,
            usage: None,
            kind: EntryKind::Other,
            size: 0,
            modified: None,
            executable: false,
            permissions: 0,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }

    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}
