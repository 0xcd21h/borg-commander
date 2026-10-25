//! Bounded directory cursors: each entry is enumerated and statted once per listing.

use std::{
    collections::HashMap,
    fs::{self, ReadDir},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use borg_vfs::Entry;
use serde::Serialize;

const IDLE: Duration = Duration::from_secs(30);
const MAX_CURSORS: usize = 64;
const BUDGET: Duration = Duration::from_millis(25);

#[derive(Clone, Default)]
pub struct Listings(Arc<Mutex<HashMap<String, Cursor>>>);

struct Cursor {
    reader: ReadDir,
    path: PathBuf,
    owner: Option<String>,
    touched: Instant,
}

#[derive(Serialize)]
pub struct Page {
    pub streaming: bool,
    pub entries: Vec<Entry>,
    pub cursor: Option<String>,
    /// Present for clients that also understand the earlier offset protocol.
    pub next: Option<usize>,
}

/// Compatibility path: skip directory records before requesting metadata, and return
/// the raw directory offset so vanished/unreadable entries cannot duplicate a later page.
pub fn legacy_page(path: &Path, offset: usize) -> io::Result<Page> {
    let mut entries = Vec::with_capacity(128);
    let mut next = None;
    for (index, item) in fs::read_dir(path)?.enumerate().skip(offset) {
        if entries.len() == 128 {
            next = Some(index);
            break;
        }
        if let Ok(item) = item
            && let Ok(entry) = borg_vfs::entry(&item.path())
        {
            entries.push(entry);
        }
    }
    Ok(Page { streaming: false, entries, cursor: None, next })
}

impl Cursor {
    fn batch(
        &mut self,
        limit: usize,
        budget: Duration,
        mut stat: impl FnMut(&Path) -> io::Result<Entry>,
    ) -> (Vec<Entry>, bool) {
        let start = Instant::now();
        let mut entries = Vec::with_capacity(limit);
        loop {
            match self.reader.next() {
                Some(Ok(item)) => {
                    if let Ok(entry) = stat(&item.path()) {
                        entries.push(entry);
                    }
                }
                Some(Err(_)) => {}
                None => return (entries, true),
            }
            // A slow metadata syscall cannot be interrupted, but never wait for a full page
            // after it completes. Empty batches also carry a continuation cursor.
            if entries.len() >= limit || start.elapsed() >= budget {
                return (entries, false);
            }
        }
    }
}

impl Listings {
    pub fn page(&self, path: PathBuf, owner: Option<String>, token: Option<String>, limit: usize) -> io::Result<Page> {
        let now = Instant::now();
        let (token, mut cursor) = if let Some(token) = token {
            let mut cursors = self.0.lock().expect("directory cursors");
            cursors.retain(|_, cursor| now.duration_since(cursor.touched) < IDLE);
            let cursor = cursors.get(&token).ok_or_else(|| io::Error::other("directory cursor expired; refresh"))?;
            if cursor.path != path || cursor.owner != owner {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "directory cursor belongs to another listing",
                ));
            }
            (token.clone(), cursors.remove(&token).expect("validated cursor"))
        } else {
            let token = borg_link::hex(&borg_link::secret());
            let cursor = Cursor { reader: fs::read_dir(&path)?, path, owner, touched: now };
            (token, cursor)
        };
        let (entries, done) = cursor.batch(limit.clamp(1, 1024), BUDGET, borg_vfs::entry);
        if !done {
            cursor.touched = Instant::now();
            let mut cursors = self.0.lock().expect("directory cursors");
            cursors.retain(|_, cursor| now.saturating_duration_since(cursor.touched) < IDLE);
            if cursors.len() >= MAX_CURSORS
                && let Some(oldest) =
                    cursors.iter().min_by_key(|(_, cursor)| cursor.touched).map(|(key, _)| key.clone())
            {
                cursors.remove(&oldest);
            }
            cursors.insert(token.clone(), cursor);
        }
        Ok(Page { streaming: true, entries, cursor: (!done).then_some(token), next: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_directory_is_read_once_across_chunks_and_slow_stats_yield_early() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..2048 {
            fs::write(dir.path().join(n.to_string()), b"").unwrap();
        }
        let mut cursor = Cursor {
            reader: fs::read_dir(dir.path()).unwrap(),
            path: dir.path().into(),
            owner: None,
            touched: Instant::now(),
        };
        let mut stats = 0;
        let start = Instant::now();
        let mut names = std::collections::HashSet::new();
        loop {
            let (entries, done) = cursor.batch(128, Duration::from_secs(1), |path| {
                stats += 1;
                borg_vfs::entry(path)
            });
            names.extend(entries.into_iter().map(|entry| entry.name));
            if done {
                break;
            }
        }
        assert_eq!(names.len(), 2048);
        assert_eq!(stats, 2048, "pagination must not repeat metadata syscalls");
        let cursor_elapsed = start.elapsed();
        let start = Instant::now();
        let mut legacy_stats = 0;
        let mut offset = 0;
        loop {
            let page = borg_vfs::entries(dir.path())
                .unwrap()
                .inspect(|_| legacy_stats += 1)
                .skip(offset)
                .take(129)
                .collect::<Vec<_>>();
            if page.len() <= 128 {
                break;
            }
            offset += 128;
        }
        eprintln!(
            "2048 entries: previous pagination {legacy_stats} metadata calls / {:?}; cursor {stats} calls / {:?}",
            start.elapsed(),
            cursor_elapsed
        );
        assert!(legacy_stats > stats * 8);

        cursor.reader = fs::read_dir(dir.path()).unwrap();
        let (entries, done) = cursor.batch(128, Duration::from_millis(5), |path| {
            std::thread::sleep(Duration::from_millis(2));
            borg_vfs::entry(path)
        });
        assert!(!done);
        assert!(!entries.is_empty() && entries.len() < 128, "slow syscalls must not delay an entire page");
    }

    #[test]
    fn cursor_cannot_be_reused_for_another_path_or_computer() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one"), b"").unwrap();
        let listings = Listings::default();
        let page = listings.page(dir.path().into(), Some("alice".into()), None, 1).unwrap();
        let token = page.cursor.unwrap();
        assert!(listings.page(dir.path().into(), Some("bob".into()), Some(token.clone()), 1).is_err());
        assert!(listings.page(dir.path().join("other"), Some("alice".into()), Some(token.clone()), 1).is_err());
        assert!(listings.page(dir.path().into(), Some("alice".into()), Some(token), 1).unwrap().cursor.is_none());
    }
}
