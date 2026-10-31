use borg_core::{Query, RESULT_LIMIT, SearchProgress, SearchRequest};
use borg_vfs::{Entry, EntryKind, FolderUsage};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Read,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const FORMAT: u8 = 2;
const MAX_ROOTS: usize = 16;
const INDEX: &str = "sizes-index.json";
const INDEX_BYTES: u64 = 64 * 1024;
const MAX_RECORD: usize = 64 * 1024;
#[derive(Clone)]
pub struct SizeCache(Arc<Inner>);
struct Inner {
    dir: Option<PathBuf>,
    lock: Mutex<()>,
    epoch: AtomicU64,
    changing: AtomicUsize,
    sequence: AtomicU64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Key {
    root: Vec<u8>,
    hidden: bool,
}
#[derive(Serialize, Deserialize)]
struct SavedEntry {
    path: Vec<u8>,
    bytes: u64,
    files: u64,
    folders: u64,
    partial: bool,
    mode: u32,
    direct_bytes: u64,
    direct_files: u64,
}
#[derive(Serialize, Deserialize)]
struct Meta {
    version: u8,
    key: Key,
    device: u64,
    inode: u64,
    measured: u64,
    visited: usize,
    errors: usize,
    skipped: usize,
}
pub struct ScanStore {
    cache: SizeCache,
    key: Key,
    identity: (u64, u64),
    epoch: u64,
    staged: Option<PathBuf>,
    connection: Option<Connection>,
}
impl Drop for ScanStore {
    fn drop(&mut self) {
        self.connection.take();
        if let Some(path) = self.staged.take() {
            let _ = fs::remove_file(path);
        }
    }
}
impl ScanStore {
    pub fn put(&self, entry: &Entry) -> Option<()> {
        let u = entry.usage.as_ref()?;
        let path = entry.identity();
        let parent = path.parent().unwrap_or(path);
        let saved = SavedEntry {
            path: path.as_os_str().as_bytes().to_vec(),
            bytes: entry.size,
            files: u.files,
            folders: u.folders,
            partial: u.partial,
            mode: entry.permissions,
            direct_bytes: u.direct_bytes,
            direct_files: u.direct_files,
        };
        let data = serde_json::to_vec(&saved).ok()?;
        self.connection
            .as_ref()?
            .execute(
                "INSERT OR REPLACE INTO folders(path,parent,data) VALUES(?1,?2,?3)",
                params![saved.path, parent.as_os_str().as_bytes(), data],
            )
            .ok()?;
        Some(())
    }
    pub fn finish(mut self, progress: &SearchProgress) -> Option<()> {
        let cache = self.cache.clone();
        let _guard = cache.0.lock.lock().ok()?;
        if cache.epoch() != self.epoch || cache.0.changing.load(Ordering::SeqCst) > 0 {
            return None;
        }
        let root = path(&self.key.root);
        let current = fs::symlink_metadata(&root).ok()?;
        if (current.dev(), current.ino()) != self.identity {
            return None;
        }
        let meta = Meta {
            version: FORMAT,
            key: self.key.clone(),
            device: self.identity.0,
            inode: self.identity.1,
            measured: progress.measured.unwrap_or_else(SystemTime::now).duration_since(UNIX_EPOCH).ok()?.as_secs(),
            visited: progress.visited,
            errors: progress.errors,
            skipped: progress.skipped,
        };
        let conn = self.connection.take()?;
        conn.execute("INSERT INTO metadata(data) VALUES(?1)", params![serde_json::to_vec(&meta).ok()?]).ok()?;
        conn.execute_batch("COMMIT;").ok()?;
        drop(conn);
        let dir = cache.0.dir.as_ref()?;
        let staged = self.staged.as_ref()?;
        if cache.epoch() != self.epoch {
            return None;
        }
        fs::rename(staged, file(dir, &self.key)).ok()?;
        self.staged = None;
        let mut keys = cache.index(dir);
        keys.retain(|k| k != &self.key);
        keys.push(self.key.clone());
        while keys.len() > MAX_ROOTS {
            let old = keys.remove(0);
            let _ = fs::remove_file(file(dir, &old));
        }
        crate::store::write_atomically(&dir.join(INDEX), &serde_json::to_vec(&keys).ok()?).ok()?;
        Some(())
    }
}
/// Borg's cache folder: `BORG_CACHE_DIR`, or `borg-commander` in the XDG or home cache.
pub fn cache_dir() -> Option<PathBuf> {
    env::var_os("BORG_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_CACHE_HOME").map(|p| PathBuf::from(p).join("borg-commander")))
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache/borg-commander")))
}
impl SizeCache {
    pub fn locate() -> Self {
        Self::at(cache_dir())
    }
    pub fn at(dir: Option<PathBuf>) -> Self {
        Self(Arc::new(Inner {
            dir,
            lock: Mutex::new(()),
            epoch: AtomicU64::new(0),
            changing: AtomicUsize::new(0),
            sequence: AtomicU64::new(0),
        }))
    }
    pub fn epoch(&self) -> u64 {
        self.0.epoch.load(Ordering::SeqCst)
    }
    pub fn begin_changes(&self) {
        self.0.changing.fetch_add(1, Ordering::SeqCst);
        self.0.epoch.fetch_add(1, Ordering::SeqCst);
    }
    pub fn invalidate(&self, paths: &[PathBuf]) {
        self.0.epoch.fetch_add(1, Ordering::SeqCst);
        if let (Some(dir), Ok(_guard)) = (&self.0.dir, self.0.lock.lock()) {
            let mut keys = self.index(dir);
            keys.retain(|key| {
                let root = path(&key.root);
                if paths.iter().any(|p| p.starts_with(&root) || root.starts_with(p)) {
                    let _ = fs::remove_file(file(dir, key));
                    false
                } else {
                    true
                }
            });
            if let Ok(bytes) = serde_json::to_vec(&keys) {
                let _ = crate::store::write_atomically(&dir.join(INDEX), &bytes);
            }
        }
        self.0.changing.fetch_sub(1, Ordering::SeqCst);
    }
    fn index(&self, dir: &Path) -> Vec<Key> {
        read::<Vec<Key>>(&dir.join(INDEX), INDEX_BYTES).filter(|keys| keys.len() <= MAX_ROOTS).unwrap_or_default()
    }
    pub fn begin(&self, root: &Path, hidden: bool, identity: (u64, u64), epoch: u64) -> Option<ScanStore> {
        let key = Key { root: root.as_os_str().as_bytes().to_vec(), hidden };
        let (connection, staged) = if let Some(dir) = &self.0.dir {
            fs::create_dir_all(dir).ok()?;
            let id = self.0.sequence.fetch_add(1, Ordering::SeqCst);
            let staged = dir.join(format!(".scan-{}-{id}-{}.sqlite", std::process::id(), hex(&key)));
            let conn = Connection::open(&staged).ok()?;
            (conn, Some(staged))
        } else {
            (Connection::open_in_memory().ok()?, None)
        };
        connection.execute_batch("PRAGMA cache_size=-2048; PRAGMA temp_store=FILE; CREATE TABLE folders(path BLOB PRIMARY KEY,parent BLOB NOT NULL,data BLOB NOT NULL); CREATE INDEX by_parent ON folders(parent); CREATE TABLE metadata(data BLOB NOT NULL); BEGIN IMMEDIATE;").ok()?;
        Some(ScanStore { cache: self.clone(), key, identity, epoch, staged, connection: Some(connection) })
    }
    #[cfg(test)]
    pub fn save(
        &self,
        root: &Path,
        hidden: bool,
        identity: (u64, u64),
        entries: &[Entry],
        progress: &SearchProgress,
        epoch: u64,
    ) {
        if self.epoch() != epoch || self.0.changing.load(Ordering::SeqCst) > 0 {
            return;
        }
        if let Some(store) = self.begin(root, hidden, identity, epoch) {
            for entry in entries {
                if store.put(entry).is_none() {
                    return;
                }
            }
            store.finish(progress);
        }
    }
    fn open(&self, key: &Key) -> Option<(Connection, Meta)> {
        if self.0.changing.load(Ordering::SeqCst) > 0 {
            return None;
        }
        let dir = self.0.dir.as_ref()?;
        if !self.index(dir).contains(key) {
            return None;
        }
        let conn = Connection::open_with_flags(
            file(dir, key),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        let data: Vec<u8> = conn.query_row("SELECT data FROM metadata LIMIT 1", [], |r| r.get(0)).ok()?;
        let meta: Meta = serde_json::from_slice(&data).ok()?;
        let stat = fs::symlink_metadata(path(&key.root)).ok()?;
        if meta.version != FORMAT
            || meta.key != *key
            || !stat.is_dir()
            || (stat.dev(), stat.ino()) != (meta.device, meta.inode)
        {
            return None;
        }
        Some((conn, meta))
    }
    #[cfg(test)]
    pub fn load(&self, root: &Path, hidden: bool) -> Option<(Vec<Entry>, SearchProgress)> {
        let key = Key { root: root.as_os_str().as_bytes().to_vec(), hidden };
        let (conn, meta) = self.open(&key)?;
        let mut entries = Vec::new();
        let mut query = conn.prepare("SELECT data FROM folders ORDER BY path LIMIT ?1").ok()?;
        let rows = query.query_map(params![(RESULT_LIMIT + 1) as i64], |r| r.get::<_, Vec<u8>>(0)).ok()?;
        for data in rows {
            entries.push(decode(&data.ok()?, &meta)?);
        }
        let mut progress = progress(&meta);
        progress.limited = entries.len() > RESULT_LIMIT;
        entries.truncate(RESULT_LIMIT);
        if !entries.iter().any(|e| e.identity() == root) {
            return None;
        }
        Some((entries, progress))
    }
    pub fn tree(&self, root: &Path, hidden: bool) -> Option<(Vec<Entry>, SearchProgress)> {
        let key = Key { root: root.as_os_str().as_bytes().to_vec(), hidden };
        let (conn, meta) = self.open(&key)?;
        let data: Vec<u8> =
            conn.query_row("SELECT data FROM folders WHERE path=?1", params![key.root], |r| r.get(0)).ok()?;
        let mut entries = vec![decode(&data, &meta)?];
        let (children, limited) = children(&conn, &meta, root, RESULT_LIMIT - 1)?;
        entries.extend(children);
        let mut progress = progress(&meta);
        progress.limited = limited;
        Some((entries, progress))
    }
    pub fn children(&self, root: &Path, hidden: bool, parent: &Path) -> Option<(Vec<Entry>, bool)> {
        if !parent.starts_with(root) {
            return None;
        }
        let (conn, meta) = self.open(&Key { root: root.as_os_str().as_bytes().to_vec(), hidden })?;
        children(&conn, &meta, parent, RESULT_LIMIT)
    }
    pub fn flat(&self, request: &SearchRequest, stopped: impl Fn() -> bool) -> Option<(Vec<Entry>, SearchProgress)> {
        let (conn, meta) =
            self.open(&Key { root: request.root.as_os_str().as_bytes().to_vec(), hidden: request.hidden })?;
        let mut query = conn.prepare("SELECT data FROM folders").ok()?;
        let rows = query.query_map([], |r| r.get::<_, Vec<u8>>(0)).ok()?;
        let filter = Query::parse(&request.query);
        let mut largest = std::collections::BTreeMap::new();
        let mut limited = false;
        for data in rows {
            if stopped() {
                return None;
            }
            let entry = decode(&data.ok()?, &meta)?;
            if entry.identity() != request.root
                && entry.size >= request.options.minimum
                && entry.usage.as_ref()?.files >= request.options.minimum_files
                && filter.accepts(&entry)
            {
                largest.insert((entry.size, entry.identity().to_path_buf()), entry);
                if largest.len() > RESULT_LIMIT {
                    largest.pop_first();
                    limited = true;
                }
            }
        }
        let mut progress = progress(&meta);
        progress.limited = limited;
        Some((largest.into_values().rev().collect(), progress))
    }
    pub fn totals(&self, dir: &Path) -> std::collections::HashMap<PathBuf, (u64, FolderUsage)> {
        let mut totals: std::collections::HashMap<PathBuf, (u64, FolderUsage)> = Default::default();
        let Some(base) = &self.0.dir else { return totals };
        let mut keys = self.index(base);
        keys.sort_by_key(|k| (std::cmp::Reverse(path(&k.root).components().count()), !k.hidden));
        for key in keys.iter().filter(|k| dir.starts_with(path(&k.root)) || path(&k.root).parent() == Some(dir)) {
            if let Some((conn, meta)) = self.open(key)
                && let Some((entries, _)) = children(&conn, &meta, dir, usize::MAX / 2)
            {
                for entry in entries {
                    if let Some(usage) = entry.usage {
                        let source = entry.source.unwrap_or_default();
                        if totals.get(&source).is_none_or(|(_, old)| old.measured < usage.measured) {
                            totals.insert(source, (entry.size, usage));
                        }
                    }
                }
            }
        }
        totals
    }
}
fn children(conn: &Connection, meta: &Meta, parent: &Path, limit: usize) -> Option<(Vec<Entry>, bool)> {
    let mut query =
        conn.prepare("SELECT data FROM folders WHERE parent=?1 AND path<>parent ORDER BY path LIMIT ?2").ok()?;
    let rows = query
        .query_map(params![parent.as_os_str().as_bytes(), limit.saturating_add(1) as i64], |r| r.get::<_, Vec<u8>>(0))
        .ok()?;
    let mut entries = Vec::new();
    for data in rows {
        entries.push(decode(&data.ok()?, meta)?);
    }
    let limited = entries.len() > limit;
    entries.truncate(limit);
    Some((entries, limited))
}
fn decode(data: &[u8], meta: &Meta) -> Option<Entry> {
    if data.len() > MAX_RECORD {
        return None;
    }
    let saved: SavedEntry = serde_json::from_slice(data).ok()?;
    let source = path(&saved.path);
    let root = path(&meta.key.root);
    if !source.is_absolute()
        || !source.starts_with(root)
        || source.components().any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(Entry {
        name: source.file_name().unwrap_or_else(|| source.as_os_str()).to_string_lossy().into_owned(),
        source: Some(source),
        missing: false,
        kind: EntryKind::Dir,
        size: saved.bytes,
        modified: None,
        executable: false,
        permissions: saved.mode,
        usage: Some(FolderUsage {
            files: saved.files,
            folders: saved.folders,
            partial: saved.partial,
            measured: Some(UNIX_EPOCH.checked_add(std::time::Duration::from_secs(meta.measured))?),
            cached: true,
            direct_bytes: saved.direct_bytes,
            direct_files: saved.direct_files,
        }),
    })
}
fn progress(meta: &Meta) -> SearchProgress {
    SearchProgress {
        visited: meta.visited,
        errors: meta.errors,
        skipped: meta.skipped,
        measured: UNIX_EPOCH.checked_add(std::time::Duration::from_secs(meta.measured)),
        cached: true,
        ..Default::default()
    }
}
fn path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}
fn hex(key: &Key) -> String {
    let mut digest = Sha256::new();
    digest.update(&key.root);
    digest.update([u8::from(key.hidden)]);
    format!("{:x}", digest.finalize())
}
fn file(dir: &Path, key: &Key) -> PathBuf {
    dir.join(format!("sizes-{}.sqlite", hex(key)))
}
fn read<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Option<T> {
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests;
