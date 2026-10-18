use std::{
    fs, io,
    ops::Add,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::Totals;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File(u64),
    Link(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub source: PathBuf,
    pub relative: PathBuf,
    pub kind: Kind,
    pub mode: u32,
    pub modified: Option<SystemTime>,
    pub device: u64,
}

impl Item {
    fn of(source: PathBuf, relative: PathBuf) -> io::Result<Self> {
        let meta = fs::symlink_metadata(&source)?;
        let kind = if meta.file_type().is_symlink() {
            Kind::Link(fs::read_link(&source)?)
        } else if meta.is_dir() {
            Kind::Dir
        } else {
            Kind::File(meta.len())
        };
        Ok(Self {
            source,
            relative,
            kind,
            mode: meta.permissions().mode(),
            modified: meta.modified().ok(),
            device: meta.dev(),
        })
    }

    pub fn totals(&self) -> Totals {
        match self.kind {
            Kind::File(size) => Totals::file(size),
            Kind::Link(_) => Totals::file(0),
            Kind::Dir => Totals::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Root {
    pub source: PathBuf,
    pub items: Vec<Item>,
}

impl Root {
    pub fn totals(&self) -> Totals {
        self.items.iter().map(Item::totals).fold(Totals::default(), Totals::add)
    }
}

pub(crate) fn scan(source: &Path, seen: &mut dyn FnMut(Totals)) -> io::Result<Root> {
    let mut items = Vec::new();
    let mut pending = vec![(source.to_path_buf(), PathBuf::new())];
    let mut found = Totals::default();
    while let Some((path, relative)) = pending.pop() {
        let item = Item::of(path, relative)?;
        if item.kind == Kind::Dir {
            let mut children: Vec<PathBuf> =
                fs::read_dir(&item.source)?.map(|entry| entry.map(|entry| entry.path())).collect::<io::Result<_>>()?;
            children.sort();
            pending.extend(children.into_iter().rev().map(|child| {
                let name = child.file_name().map(PathBuf::from).unwrap_or_default();
                let relative = item.relative.join(name);
                (child, relative)
            }));
        }
        found = found.add(item.totals());
        seen(found);
        items.push(item);
    }
    Ok(Root { source: source.to_path_buf(), items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_depth_first_with_parents_before_children() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = dir.path().join("tree");
        fs::create_dir_all(root.join("b")).unwrap_or_else(|e| panic!("{e}"));
        fs::write(root.join("a.txt"), "hello").unwrap_or_else(|e| panic!("{e}"));
        fs::write(root.join("b/c.txt"), "hi").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap_or_else(|e| panic!("{e}"));

        let mut last = Totals::default();
        let plan = scan(&root, &mut |found| last = found).unwrap_or_else(|e| panic!("{e}"));
        let order: Vec<String> = plan.items.iter().map(|item| item.relative.display().to_string()).collect();
        assert_eq!(order, ["", "a.txt", "b", "b/c.txt", "link"]);
        assert_eq!(plan.totals(), Totals { files: 3, bytes: 7 });
        assert_eq!(last, plan.totals());
        assert_eq!(plan.items[4].kind, Kind::Link(PathBuf::from("a.txt")));
    }
}
