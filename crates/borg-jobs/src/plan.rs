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
    Mount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mounts {
    Cross,
    Stop,
}

pub fn mount_point(path: &Path, device: u64) -> bool {
    path.parent().and_then(|parent| fs::metadata(parent).ok()).is_some_and(|parent| parent.dev() != device)
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
            Kind::Dir | Kind::Mount => Totals::default(),
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

pub(crate) fn scan(source: &Path, mounts: Mounts, seen: &mut dyn FnMut(Totals)) -> io::Result<Root> {
    let mut items = Vec::new();
    let mut pending = vec![(source.to_path_buf(), PathBuf::new())];
    let mut found = Totals::default();
    let mut home = None;
    while let Some((path, relative)) = pending.pop() {
        let mut item = Item::of(path, relative)?;
        let device = *home.get_or_insert(item.device);
        let mounted_root =
            item.relative.as_os_str().is_empty() && item.kind == Kind::Dir && mount_point(&item.source, item.device);
        if mounts == Mounts::Stop && (item.device != device || mounted_root) {
            item.kind = Kind::Mount;
        }
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

    const DEVICES: &str = "/dev";
    #[cfg(target_os = "linux")]
    const KERNEL_FILESYSTEMS: &str = "/sys/fs";

    #[test]
    fn scans_depth_first_with_parents_before_children() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = dir.path().join("tree");
        fs::create_dir_all(root.join("b")).unwrap_or_else(|e| panic!("{e}"));
        fs::write(root.join("a.txt"), "hello").unwrap_or_else(|e| panic!("{e}"));
        fs::write(root.join("b/c.txt"), "hi").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap_or_else(|e| panic!("{e}"));

        let mut last = Totals::default();
        let plan = scan(&root, Mounts::Stop, &mut |found| last = found).unwrap_or_else(|e| panic!("{e}"));
        let order: Vec<String> = plan.items.iter().map(|item| item.relative.display().to_string()).collect();
        assert_eq!(order, ["", "a.txt", "b", "b/c.txt", "link"]);
        assert_eq!(plan.totals(), Totals { files: 3, bytes: 7 });
        assert_eq!(last, plan.totals());
        assert_eq!(plan.items[4].kind, Kind::Link(PathBuf::from("a.txt")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_scan_that_stops_at_mounts_never_lists_what_is_mounted_inside() {
        let parent = Path::new(KERNEL_FILESYSTEMS);
        let Ok(home) = fs::metadata(parent).map(|meta| meta.dev()) else { return };
        let mounted: Vec<PathBuf> = fs::read_dir(parent)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.dev() != home))
            .collect();
        if mounted.is_empty() {
            return;
        }
        let Ok(kept) = scan(parent, Mounts::Stop, &mut |_| {}) else { return };
        for mount in &mounted {
            let relative = mount.strip_prefix(parent).unwrap_or_else(|e| panic!("{e}"));
            let shown = kept.items.iter().find(|item| item.relative == relative);
            assert_eq!(shown.map(|item| &item.kind), Some(&Kind::Mount), "{}", mount.display());
            assert!(!kept.items.iter().any(|item| item.relative.starts_with(relative) && item.relative != relative));
        }
    }

    #[test]
    fn a_source_that_is_a_mount_point_is_kept_whole() {
        let devices = Path::new(DEVICES);
        let Ok(meta) = fs::metadata(devices) else { return };
        if !mount_point(devices, meta.dev()) {
            return;
        }
        let kept = scan(devices, Mounts::Stop, &mut |_| {}).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(kept.items.iter().map(|item| &item.kind).collect::<Vec<_>>(), [&Kind::Mount]);
    }

    #[test]
    fn a_folder_on_its_own_filesystem_is_a_mount_point_and_one_inside_its_parent_is_not() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let inner = dir.path().join("inner");
        fs::create_dir(&inner).unwrap_or_else(|e| panic!("{e}"));
        let device = fs::metadata(&inner).unwrap_or_else(|e| panic!("{e}")).dev();
        assert!(!mount_point(&inner, device));
        assert!(mount_point(&inner, device.wrapping_add(1)));
        let plan = scan(&inner, Mounts::Stop, &mut |_| {}).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(plan.items[0].kind, Kind::Dir);
    }
}
