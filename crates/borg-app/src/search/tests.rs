use super::sizes::{Directory, provisional, revisits};
use super::*;
use borg_core::{Msg, SearchKind, SearchRequest, Side};
use borg_vfs::Entry;
use std::os::unix::fs::symlink;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::mpsc;

#[tokio::test]
async fn folder_totals_find_many_small_files_and_filter_only_after_measuring() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("project/node_modules/.cache")).unwrap();
    fs::create_dir_all(root.path().join("small")).unwrap();
    for index in 0..300 {
        fs::write(root.path().join(format!("project/node_modules/.cache/{index}")), "1234567890").unwrap();
    }
    fs::write(root.path().join("small/file"), "x").unwrap();
    symlink(root.path(), root.path().join("project/node_modules/loop")).unwrap();
    let mut worker = Search::with_cache(crate::size_cache::SizeCache::at(None));
    worker.search(
        Side::Left,
        9,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "node_modules".into(),
            hidden: true,
            options: borg_core::SearchOptions {
                kind: SearchKind::FolderTotals,
                minimum: 2000,
                minimum_files: 200,
                ..Default::default()
            },
        },
    );
    let mut found = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), worker.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries, .. } => found.extend(entries),
            Msg::Listed { error, .. } => {
                assert!(error.is_none());
                break;
            }
            _ => {}
        }
    }
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "node_modules");
    assert_eq!(found[0].size, 3000);
    assert_eq!(found[0].usage.as_ref().unwrap().files, 300);
    assert!(!found[0].usage.as_ref().unwrap().partial);
    worker.search(
        Side::Left,
        10,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "node_modules".into(),
            hidden: false,
            options: borg_core::SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
        },
    );
    let mut partial = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), worker.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries, .. } => partial.extend(entries),
            Msg::Listed { .. } => break,
            _ => {}
        }
    }
    assert_eq!(partial[0].size, 0);
    assert!(partial[0].usage.as_ref().unwrap().partial);
}

#[tokio::test]
async fn recursive_search_preserves_sources_and_does_not_follow_symlink_loops() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("one/nested")).unwrap();
    fs::create_dir(root.path().join("two")).unwrap();
    fs::create_dir(root.path().join(".hidden")).unwrap();
    for path in ["one/config.toml", "two/config.toml", ".hidden/config.toml", "one/nested/other.txt"] {
        fs::write(root.path().join(path), "hello").unwrap();
    }
    symlink(root.path(), root.path().join("one/loop")).unwrap();
    let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
    search.search(
        Side::Left,
        7,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "*.toml".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    let mut found = Vec::new();
    let progress = loop {
        match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries, .. } => found.extend(entries),
            Msg::SearchProgress { progress, .. } => break progress,
            _ => {}
        }
    };
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].source, found[1].source);
    assert_eq!(found[0].name, found[1].name);
    assert_eq!(progress.errors, 0);
    assert_eq!(progress.skipped, 1);
    assert!(!progress.limited);
}

#[tokio::test]
async fn matches_near_the_search_folder_come_first_and_nothing_is_counted_twice() {
    let root = tempfile::tempdir().unwrap();
    let deep = root.path().join("a/b/c/d/e");
    fs::create_dir_all(&deep).unwrap();
    fs::create_dir(root.path().join("Downloads")).unwrap();
    fs::write(deep.join("регламент 2019.txt"), "old").unwrap();
    fs::write(root.path().join("Downloads/Регламент передачи ключей.pdf"), "new").unwrap();
    let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
    search.search(
        Side::Left,
        7,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "*регламент*".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    let mut found = Vec::new();
    let progress = loop {
        match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries, .. } => found.extend(entries.into_iter().map(|entry| entry.name)),
            Msg::SearchProgress { progress, .. } => break progress,
            _ => {}
        }
    };
    assert_eq!(found, ["Регламент передачи ключей.pdf", "регламент 2019.txt"]);
    assert_eq!(progress.visited, 8, "five folders, Downloads and two files, each once");
    assert_eq!(progress.errors, 0);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn other_apps_data_is_left_out_from_above_and_read_when_searched_in_its_own_folder() {
    let root = tempfile::tempdir().unwrap();
    let groups = root.path().join("Library/Group Containers");
    for folder in ["Library/Group Containers/TEAM.group.chat/Downloads", "Library/Containers/org.example.notes/Data"] {
        fs::create_dir_all(root.path().join(folder)).unwrap();
        fs::write(root.path().join(folder).join("report.txt"), "kept by an app").unwrap();
    }
    fs::write(root.path().join("Library/report.txt"), "mine").unwrap();
    async fn found(search: &mut Search, root: &std::path::Path, id: u64) -> (Vec<std::path::PathBuf>, usize) {
        let (skip, query, hidden, options) = (None, "report".into(), false, Default::default());
        search.search(Side::Left, id, SearchRequest { skip, root: root.into(), query, hidden, options });
        let mut found = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries.iter().map(|entry| entry.identity().to_path_buf())),
                Msg::SearchProgress { progress, .. } => return (found, progress.excluded),
                _ => {}
            }
        }
    }
    let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
    let (found_above, excluded) = found(&mut search, root.path(), 1).await;
    assert_eq!(found_above, [root.path().join("Library/report.txt")]);
    assert_eq!(excluded, 2, "both folders of apps' data are left out, once each");
    let (found_inside, _) = found(&mut search, &groups, 2).await;
    assert_eq!(found_inside, [groups.join("TEAM.group.chat/Downloads/report.txt")]);
}

#[tokio::test]
async fn name_searches_leave_out_generated_ignored_and_configured_paths_unless_asked() {
    let root = tempfile::tempdir().unwrap();
    let files = [
        "app/main.rs",
        "app/node_modules/dep/index.rs",
        "app/Cargo.toml",
        "app/target/debug/build.rs",
        "notes/target/plan.rs",
        "cache/CACHEDIR.TAG",
        "cache/blob.rs",
        "gen/out.rs",
        "app/ignored.rs",
        "big/huge.rs",
    ];
    for file in files {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    fs::write(root.path().join("cache/CACHEDIR.TAG"), borg_core::CACHEDIR_SIGNATURE).unwrap();
    fs::write(root.path().join("app/.gitignore"), "ignored.rs\n").unwrap();
    fs::write(root.path().join(".borgignore"), "gen/\n").unwrap();
    let base = root.path().to_path_buf();
    let run = |query: &str, skip: Option<borg_core::SearchSettings>| {
        let base = base.clone();
        let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
        let request =
            SearchRequest { root: base.clone(), query: query.into(), hidden: false, options: Default::default(), skip };
        async move {
            search.search(Side::Left, 1, request);
            let mut found = Vec::new();
            loop {
                match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                    Msg::Batch { entries, .. } => found.extend(entries),
                    Msg::Listed { .. } => break,
                    _ => {}
                }
            }
            let mut paths: Vec<String> =
                found.iter().map(|entry| entry.identity().strip_prefix(&base).unwrap().display().to_string()).collect();
            paths.sort();
            paths
        }
    };
    let settings = borg_core::SearchSettings { exclude: vec!["big".into()], ..Default::default() };
    assert_eq!(run("ext:rs", Some(settings.clone())).await, ["app/main.rs", "notes/target/plan.rs"]);
    assert_eq!(
        run("node_modules | target", Some(settings.clone())).await,
        ["app/node_modules", "app/target", "notes/target"]
    );
    assert_eq!(run("ext:rs", None).await.len(), 8, "nothing is left out without exclusions");
}

#[tokio::test(flavor = "multi_thread")]
async fn indexed_folders_answer_from_their_index_and_searching_everything_walks() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    for file in ["src/main.rs", "src/lib.rs", "docs/main.md"] {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    let indexes = crate::index::Indexes::new(Some(cache.path().into()), Duration::from_secs(600));
    let (messages, mut built) = tokio::sync::mpsc::unbounded_channel();
    indexes.connect(messages);
    indexes.set_roots(vec![root.path().into()]);
    indexes.refresh(root.path().into(), borg_core::SearchSettings::default());
    let indexed = tokio::time::timeout(Duration::from_secs(5), built.recv()).await.unwrap();
    assert!(matches!(indexed, Some(Msg::Indexed { entries: 5, error: None, .. })), "{indexed:?}");
    let mut search = Search::with_stores(crate::size_cache::SizeCache::at(None), indexes);
    for (skip, from_index) in [(Some(borg_core::SearchSettings::default()), true), (None, false)] {
        let request = SearchRequest {
            root: root.path().join("src"),
            query: "main".into(),
            hidden: false,
            options: Default::default(),
            skip,
        };
        search.search(Side::Left, 1, request);
        let mut found = Vec::new();
        let progress = loop {
            match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
                Msg::Batch { entries, .. } => found.extend(entries),
                Msg::SearchProgress { progress, .. } => break progress,
                _ => {}
            }
        };
        assert_eq!(found.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), ["main.rs"]);
        assert_eq!(progress.indexed.is_some(), from_index);
    }
}

#[tokio::test]
async fn refresh_keeps_missing_references_and_raw_filenames() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(target_os = "linux")]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"raw-\xff.txt".to_vec())
    };
    #[cfg(not(target_os = "linux"))]
    let name = std::ffi::OsString::from("日本語.txt");
    let path = root.path().join(name);
    fs::write(&path, "hello").unwrap();
    let missing = root.path().join("missing");
    let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
    search.refresh(Side::Right, 3, vec![Entry::unavailable(path.clone()), Entry::unavailable(missing.clone())]);
    let mut found = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries, .. } => found.extend(entries),
            Msg::Listed { .. } => break,
            _ => {}
        }
    }
    assert_eq!(found[0].identity(), path);
    assert!(!found[0].missing);
    assert_eq!(found[1].identity(), missing);
    assert!(found[1].missing);
}

#[tokio::test]
async fn replacing_a_search_cancels_backpressure_without_blocking_the_next_request() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..1500 {
        fs::write(root.path().join(format!("file-{index}")), "x").unwrap();
    }
    let mut search = Search::with_cache(crate::size_cache::SizeCache::at(None));
    search.search(
        Side::Left,
        1,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "*".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    let _ = tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap();
    search.search(
        Side::Left,
        2,
        SearchRequest {
            skip: None,
            root: root.path().into(),
            query: "file-1499".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    loop {
        if let Msg::Listed { generation: 2, .. } =
            tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap()
        {
            break;
        }
    }
}
async fn measured_tree(
    search: &mut Search,
    root: &std::path::Path,
    refresh: bool,
) -> (Vec<Entry>, borg_core::SearchProgress) {
    measured_request(
        search,
        SearchRequest {
            skip: None,
            root: root.into(),
            query: "*".into(),
            hidden: true,
            options: borg_core::SearchOptions { kind: SearchKind::FolderTree, refresh, ..Default::default() },
        },
    )
    .await
}

async fn measured_request(search: &mut Search, request: SearchRequest) -> (Vec<Entry>, borg_core::SearchProgress) {
    search.search(Side::Left, 1, request);
    let mut entries = std::collections::BTreeMap::new();
    let mut progress = Default::default();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), search.incoming.recv()).await.unwrap().unwrap() {
            Msg::Batch { entries: batch, .. } => {
                for entry in batch {
                    entries.insert(entry.identity().to_path_buf(), entry);
                }
            }
            Msg::SearchProgress { progress: update, .. } => progress = update,
            Msg::Listed { error, .. } => {
                assert!(error.is_none());
                break;
            }
            _ => {}
        }
    }
    (entries.into_values().collect(), progress)
}

#[tokio::test]
async fn tree_measures_root_reuses_saved_scan_and_refreshes_nested_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("cache/nested")).unwrap();
    fs::write(root.join("cache/nested/data"), "12345").unwrap();
    fs::write(root.join("top"), "123").unwrap();
    symlink(&root, root.join("cache/loop")).unwrap();
    let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
    let mut search = Search::with_cache(cache.clone());
    let (entries, progress) = measured_tree(&mut search, &root, false).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(cache.children(&root, true, &root.join("cache")).unwrap().0[0].name, "nested");
    assert_eq!(entries[0].identity(), root);
    assert_eq!(entries[0].size, 8);
    assert_eq!(entries[0].usage.as_ref().unwrap().files, 2);
    assert_eq!(entries[0].usage.as_ref().unwrap().folders, 2);
    assert!(!progress.cached);
    fs::write(root.join("cache/nested/data"), "1234567890").unwrap();
    let (saved, progress) = measured_tree(&mut search, &root, false).await;
    assert!(progress.cached);
    assert_eq!(saved[0].size, 8);
    assert_eq!(cache.totals(&root)[&root.join("cache")].0, 5);
    let (fresh, progress) = measured_tree(&mut search, &root, true).await;
    assert!(!progress.cached);
    assert_eq!(fresh[0].size, 13);
    assert_eq!(cache.totals(&root)[&root.join("cache")].0, 10);
}
#[tokio::test]
async fn cancelled_tree_refresh_never_replaces_the_previous_saved_scan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("data"), "old").unwrap();
    let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
    let mut search = Search::with_cache(cache.clone());
    measured_tree(&mut search, &root, false).await;
    for index in 0..500 {
        fs::create_dir(root.join(index.to_string())).unwrap();
    }
    fs::write(root.join("data"), "updated").unwrap();
    let request = SearchRequest {
        skip: None,
        root: root.clone(),
        query: "*".into(),
        hidden: true,
        options: borg_core::SearchOptions { kind: SearchKind::FolderTree, refresh: true, ..Default::default() },
    };
    let (messages, mut incoming) = mpsc::channel(1);
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let cancel_worker = cancelled.clone();
    let worker_cache = cache.clone();
    let job = tokio::task::spawn_blocking(move || {
        let mut stream = Output {
            side: Side::Left,
            generation: 2,
            messages: &messages,
            cancelled: &cancel_worker,
            batch: Vec::new(),
            background: false,
        };
        scan_sizes(&request, &mut stream, &worker_cache);
    });
    assert!(matches!(incoming.recv().await, Some(Msg::Batch { .. })));
    cancelled.store(true, Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(5), job).await.unwrap().unwrap();
    assert_eq!(cache.load(&root, true).unwrap().0[0].size, 3);
}
#[tokio::test]
async fn space_analysis_reuses_complete_snapshots_and_applies_filters_to_saved_totals() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("cache/nested")).unwrap();
    fs::write(root.join("cache/nested/data"), "12345").unwrap();
    let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
    let mut search = Search::with_cache(cache);
    measured_tree(&mut search, &root, false).await;
    fs::write(root.join("cache/nested/data"), "changed outside Borg").unwrap();
    let request = SearchRequest {
        skip: None,
        root: root.clone(),
        query: "cache".into(),
        hidden: true,
        options: borg_core::SearchOptions {
            kind: SearchKind::FolderTotals,
            minimum: 5,
            minimum_files: 1,
            ..Default::default()
        },
    };
    let (saved, progress) = measured_request(&mut search, request.clone()).await;
    assert!(progress.cached);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].identity(), root.join("cache"));
    assert_eq!(saved[0].size, 5);
    let mut filtered = request.clone();
    filtered.options.minimum = 6;
    assert!(measured_request(&mut search, filtered).await.0.is_empty());
    let mut refresh = request;
    refresh.options.refresh = true;
    let (fresh, progress) = measured_request(&mut search, refresh).await;
    assert!(!progress.cached);
    assert_eq!(fresh[0].size, 20);
}

#[tokio::test]
async fn space_analysis_rescans_a_truncated_tree_snapshot_instead_of_hiding_large_folders() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("large")).unwrap();
    fs::write(root.join("large/data"), "1234567890").unwrap();
    let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("saved")));
    let meta = fs::symlink_metadata(&root).unwrap();
    let mut entry = borg_vfs::entry(&root).unwrap();
    entry.usage = Some(Default::default());
    cache.save(
        &root,
        true,
        (meta.dev(), meta.ino()),
        &[entry],
        &borg_core::SearchProgress { limited: true, ..Default::default() },
        cache.epoch(),
    );
    let db = fs::read_dir(temp.path().join("saved"))
        .unwrap()
        .find_map(|e| {
            let p = e.unwrap().path();
            (p.extension().is_some_and(|x| x == "sqlite")).then_some(p)
        })
        .unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    let bytes: Vec<u8> = conn.query_row("SELECT data FROM metadata", [], |r| r.get(0)).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["version"] = 1.into();
    conn.execute("UPDATE metadata SET data=?1", rusqlite::params![serde_json::to_vec(&value).unwrap()]).unwrap();
    drop(conn);
    let mut search = Search::with_cache(cache);
    let (fresh, progress) = measured_request(
        &mut search,
        SearchRequest {
            skip: None,
            root: root.clone(),
            query: "*".into(),
            hidden: true,
            options: borg_core::SearchOptions { kind: SearchKind::FolderTotals, ..Default::default() },
        },
    )
    .await;
    assert!(!progress.cached);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].identity(), root.join("large"));
    assert_eq!(fresh[0].size, 10);
}
#[tokio::test]
async fn background_measurements_stream_all_folders_and_share_the_complete_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("alpha/deep")).unwrap();
    fs::create_dir_all(root.join("beta")).unwrap();
    fs::write(root.join("alpha/deep/data"), [0; 42]).unwrap();
    fs::write(root.join("beta/data"), [0; 17]).unwrap();
    let cache = crate::size_cache::SizeCache::at(Some(temp.path().join("cache")));
    let mut search = Search::with_cache(cache.clone());
    search.measure(21, root.clone());
    let mut entries = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(10), search.incoming.recv()).await.unwrap().unwrap() {
            Msg::Sized { id: 21, entries: batch } => entries.extend(batch),
            Msg::SizeProgress { id: 21, progress } => assert!(progress.errors == 0 && progress.skipped == 0),
            Msg::SizesFinished { id: 21, error } => {
                assert!(error.is_none(), "{error:?}");
                break;
            }
            message => panic!("Background measurement leaked a panel search message: {message:?}"),
        }
    }
    assert_eq!(entries.len(), 4);
    assert_eq!(entries.iter().find(|e| e.identity() == root).unwrap().size, 59);
    assert_eq!(entries.iter().find(|e| e.identity() == root.join("alpha/deep")).unwrap().size, 42);
    let totals = cache.totals(&root);
    assert_eq!(totals[&root.join("alpha")].0, 42);
    assert_eq!(totals[&root.join("beta")].0, 17);
    let (tree, progress) = measured_tree(&mut search, &root, false).await;
    assert_eq!(tree.len(), 3);
    assert!(progress.cached);
}

#[test]
fn folders_already_being_walked_are_not_entered_again() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("a/b")).unwrap();
    let id = |path: &std::path::Path| {
        let meta = fs::symlink_metadata(path).unwrap();
        (meta.dev(), meta.ino())
    };
    let walked = [id(temp.path()), id(&temp.path().join("a"))];
    assert!(revisits(walked, &fs::symlink_metadata(temp.path().join("a")).unwrap()));
    assert!(revisits(walked, &fs::symlink_metadata(temp.path()).unwrap()));
    assert!(!revisits(walked, &fs::symlink_metadata(temp.path().join("a/b")).unwrap()));
}

#[test]
fn running_totals_include_open_subfolders_and_are_marked_partial() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("a/b")).unwrap();
    let frame = |path: std::path::PathBuf, bytes: u64, files: u64, folders: u64| Directory {
        reader: fs::read_dir(&path).unwrap(),
        entry: borg_vfs::entry(&path).unwrap(),
        usage: borg_vfs::FolderUsage { files, folders, ..Default::default() },
        bytes,
        identity: (0, 0),
    };
    let stack = [
        frame(temp.path().to_path_buf(), 10, 1, 0),
        frame(temp.path().join("a"), 5, 2, 0),
        frame(temp.path().join("a/b"), 3, 1, 0),
    ];
    let totals: Vec<_> = provisional(&stack)
        .into_iter()
        .map(|entry| {
            let usage = entry.usage.unwrap();
            (entry.name, entry.size, usage.files, usage.folders, usage.partial)
        })
        .collect();
    assert_eq!(totals[0], ("b".into(), 3, 1, 0, true));
    assert_eq!(totals[1], ("a".into(), 8, 3, 1, true));
    assert_eq!((totals[2].1, totals[2].2, totals[2].3, totals[2].4), (18, 4, 2, true));
}
