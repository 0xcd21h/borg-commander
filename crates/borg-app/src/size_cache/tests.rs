use super::*;
use borg_core::{RESULT_LIMIT, SearchProgress, SearchRequest};
use borg_vfs::{Entry, EntryKind, FolderUsage};
use rusqlite::Connection;
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};
fn entry(path: &Path, bytes: u64, folders: u64) -> Entry {
    let mut e = Entry::unavailable(path.into());
    e.missing = false;
    e.kind = EntryKind::Dir;
    e.size = bytes;
    e.usage = Some(FolderUsage { files: 1, folders, ..Default::default() });
    e
}
fn save(cache: &SizeCache, root: &Path, bytes: u64) {
    let m = fs::symlink_metadata(root).unwrap();
    cache.save(root, true, (m.dev(), m.ino()), &[entry(root, bytes, 0)], &SearchProgress::default(), cache.epoch());
}
#[test]
fn complete_index_retains_late_siblings_and_loads_only_expanded_branches() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    let cache = SizeCache::at(Some(temp.path().join("cache")));
    let meta = fs::symlink_metadata(&root).unwrap();
    let store = cache.begin(&root, true, (meta.dev(), meta.ino()), cache.epoch()).unwrap();
    let large = root.join("dependencies");
    store.put(&entry(&large, 30_000, RESULT_LIMIT as u64 + 1)).unwrap();
    for i in 0..RESULT_LIMIT + 1 {
        store.put(&entry(&large.join(format!("dependency-{i:05}")), 1, 0)).unwrap();
    }
    store.put(&entry(&root.join("apps"), 8_000, 5)).unwrap();
    store.put(&entry(&root.join("src"), 100, 0)).unwrap();
    let mut total = entry(&root, 38_107, RESULT_LIMIT as u64 + 4);
    total.usage.as_mut().unwrap().direct_bytes = 7;
    total.usage.as_mut().unwrap().direct_files = 1;
    store.put(&total).unwrap();
    store.finish(&SearchProgress::default()).unwrap();
    let (tree, progress) = cache.tree(&root, true).unwrap();
    assert!(!progress.limited);
    assert_eq!(tree.len(), 4);
    assert_eq!(tree[0].size, 38_107);
    assert!(tree.iter().any(|e| e.name == "apps"));
    assert!(tree.iter().any(|e| e.name == "src"));
    let totals = cache.totals(&root);
    assert_eq!(totals.len(), 3);
    assert_eq!(totals[&root.join("apps")].0, 8_000);
    let (children, limited) = cache.children(&root, true, &large).unwrap();
    assert!(limited);
    assert_eq!(children.len(), RESULT_LIMIT);
    let connection = Connection::open(file(
        cache.0.dir.as_ref().unwrap(),
        &Key { root: root.as_os_str().as_bytes().to_vec(), hidden: true },
    ))
    .unwrap();
    let count: i64 = connection.query_row("SELECT count(*) FROM folders", [], |r| r.get(0)).unwrap();
    assert_eq!(count, RESULT_LIMIT as i64 + 5);
    let request = SearchRequest {
        skip: None,
        root: root.clone(),
        query: "apps".into(),
        hidden: true,
        options: borg_core::SearchOptions { kind: borg_core::SearchKind::FolderTotals, ..Default::default() },
    };
    let (flat, _) = cache.flat(&request, || false).unwrap();
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].size, 8_000);
    assert_eq!(tree[0].usage.as_ref().unwrap().direct_bytes, 7);
}
#[test]
fn cancelled_generation_and_file_jobs_preserve_or_invalidate_the_correct_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let other = temp.path().join("other");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&other).unwrap();
    let cache = SizeCache::at(Some(temp.path().join("cache")));
    save(&cache, &root, 10);
    save(&cache, &other, 20);
    let meta = fs::symlink_metadata(&root).unwrap();
    let unfinished = cache.begin(&root, true, (meta.dev(), meta.ino()), cache.epoch()).unwrap();
    unfinished.put(&entry(&root, 999, 0)).unwrap();
    drop(unfinished);
    assert_eq!(cache.tree(&root, true).unwrap().0[0].size, 10);
    assert!(
        !fs::read_dir(cache.0.dir.as_ref().unwrap()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".scan"))
    );
    let old = cache.epoch();
    cache.begin_changes();
    let during = cache.epoch();
    assert!(cache.tree(&root, true).is_none());
    cache.invalidate(&[root.join("file")]);
    for epoch in [old, during] {
        cache.save(&root, true, (meta.dev(), meta.ino()), &[entry(&root, 99, 0)], &SearchProgress::default(), epoch);
    }
    assert!(cache.tree(&root, true).is_none());
    assert_eq!(cache.tree(&other, true).unwrap().0[0].size, 20);
}
#[test]
fn cached_observations_keep_policy_identity_and_corruption_checks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    let base = temp.path().join("cache");
    let cache = SizeCache::at(Some(base.clone()));
    save(&cache, &root, 10);
    assert!(cache.tree(&root, false).is_none());
    assert!(cache.tree(&root, true).unwrap().1.cached);
    fs::write(root.join("external"), "changed").unwrap();
    assert_eq!(cache.tree(&root, true).unwrap().0[0].size, 10);
    fs::rename(&root, temp.path().join("old")).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(cache.tree(&root, true).is_none());
    save(&cache, &root, 20);
    let key = Key { root: root.as_os_str().as_bytes().to_vec(), hidden: true };
    fs::write(file(&base, &key), b"broken SQLite").unwrap();
    assert!(cache.tree(&root, true).is_none());
}
