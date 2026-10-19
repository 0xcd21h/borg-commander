use std::{
    collections::BTreeMap,
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub index: char,
    pub worktree: char,
}

impl Status {
    pub fn label(self) -> String {
        format!("{}{}", self.index, self.worktree)
    }

    pub fn conflict(self) -> bool {
        self.index == 'U' || self.worktree == 'U' || matches!((self.index, self.worktree), ('A', 'A') | ('D', 'D'))
    }

    fn merge(self, other: Self) -> Self {
        if self.index == '?' && self.worktree == '?' {
            return other;
        }
        if other.index == '?' && other.worktree == '?' {
            return self;
        }
        if self.conflict() {
            return self;
        }
        if other.conflict() {
            return other;
        }
        let merge = |a, b| {
            if a == ' ' {
                b
            } else if b == ' ' || a == b {
                a
            } else {
                'M'
            }
        };
        let combined = Self { index: merge(self.index, other.index), worktree: merge(self.worktree, other.worktree) };
        if combined.conflict() { Self { index: 'M', worktree: 'M' } } else { combined }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
    pub branch: String,
    pub changes: BTreeMap<PathBuf, Status>,
    directory: PathBuf,
    prefix: PathBuf,
}

impl Repository {
    pub fn parse(root: PathBuf, bytes: &[u8]) -> Result<Self> {
        let mut repository = Self {
            directory: root.clone(),
            prefix: PathBuf::new(),
            root,
            branch: String::new(),
            changes: BTreeMap::new(),
        };
        let mut records = bytes.split(|b| *b == 0).filter(|record| !record.is_empty());
        while let Some(record) = records.next() {
            if let Some(header) = record.strip_prefix(b"## ") {
                let header = String::from_utf8_lossy(header);
                let header = header
                    .strip_prefix("No commits yet on ")
                    .or_else(|| header.strip_prefix("Initial commit on "))
                    .unwrap_or(&header);
                repository.branch = header.split("...").next().unwrap_or(header).to_owned();
                continue;
            }
            ensure!(record.len() > 3 && record[2] == b' ', "Invalid Git status record");
            let status = Status { index: record[0] as char, worktree: record[1] as char };
            let path = PathBuf::from(OsStr::from_bytes(&record[3..]));
            repository.insert(path, status);
            if matches!(status.index, 'R' | 'C') || matches!(status.worktree, 'R' | 'C') {
                let original = records.next().ok_or_else(|| anyhow::anyhow!("Missing Git rename source"))?;
                repository.insert(PathBuf::from(OsStr::from_bytes(original)), status);
            }
        }
        Ok(repository)
    }

    fn insert(&mut self, path: PathBuf, status: Status) {
        for parent in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
            self.changes
                .entry(parent.to_path_buf())
                .and_modify(|previous| *previous = previous.merge(status))
                .or_insert(status);
        }
    }

    pub fn status(&self, path: &Path) -> Option<Status> {
        let relative = path
            .strip_prefix(&self.directory)
            .map(|path| self.prefix.join(path))
            .or_else(|_| path.strip_prefix(&self.root).map(PathBuf::from))
            .ok()?;
        self.changes.get(&relative).copied()
    }

    pub(crate) fn at_directory(mut self, directory: PathBuf, prefix: PathBuf) -> Self {
        self.directory = directory;
        self.prefix = prefix;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Status,
    Diff,
    StagedDiff,
    Log,
    Stage,
    Unstage,
}

impl Tool {
    pub fn changes_index(self) -> bool {
        matches!(self, Self::Stage | Self::Unstage)
    }
    pub fn needs_selection(self) -> bool {
        matches!(self, Self::Diff | Self::StagedDiff | Self::Stage | Self::Unstage)
    }
}

#[derive(Debug)]
pub enum Output {
    Document { name: &'static str, text: Vec<u8> },
    IndexUpdated,
}
