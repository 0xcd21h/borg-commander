use std::{
    fs::{self, Metadata},
    io,
    path::Path,
};

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{Backend, Batches, Entry, EntryKind};

#[derive(Debug, Clone, Copy)]
pub struct Local {
    batch_size: usize,
}

impl Local {
    pub fn new(batch_size: usize) -> Self {
        Self { batch_size }
    }
}

impl Backend for Local {
    fn list(&self, dir: &Path) -> Batches {
        let (tx, rx) = mpsc::channel(1);
        let dir = dir.to_path_buf();
        let batch_size = self.batch_size;
        tokio::task::spawn_blocking(move || stream_dir(&dir, batch_size, &tx));
        ReceiverStream::new(rx).boxed()
    }
}

pub fn entries(dir: &Path) -> io::Result<impl Iterator<Item = Entry>> {
    Ok(fs::read_dir(dir)?.filter_map(Result::ok).filter_map(|item| entry(&item.path()).ok()))
}

fn stream_dir(dir: &Path, batch_size: usize, tx: &mpsc::Sender<io::Result<Vec<Entry>>>) {
    let reader = match entries(dir) {
        Ok(reader) => reader,
        Err(e) => {
            let _ = tx.blocking_send(Err(e));
            return;
        }
    };

    let mut batch = Vec::with_capacity(batch_size);
    for entry in reader {
        batch.push(entry);
        if batch.len() < batch_size {
            continue;
        }
        let full = std::mem::replace(&mut batch, Vec::with_capacity(batch_size));
        if tx.blocking_send(Ok(full)).is_err() {
            return;
        }
    }

    if !batch.is_empty() {
        let _ = tx.blocking_send(Ok(batch));
    }
}

pub fn entry(path: &Path) -> io::Result<Entry> {
    let own = fs::symlink_metadata(path)?;
    let target = own.file_type().is_symlink().then(|| fs::metadata(path).ok()).flatten();
    let effective = target.as_ref().unwrap_or(&own);
    Ok(Entry {
        source: Some(path.to_path_buf()),
        missing: false,
        usage: None,
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        kind: kind_of(&own, target.as_ref()),
        size: effective.len(),
        modified: effective.modified().ok(),
        executable: is_executable(effective),
        permissions: permissions(&own),
    })
}

fn kind_of(own: &Metadata, target: Option<&Metadata>) -> EntryKind {
    let file_type = own.file_type();
    if file_type.is_symlink() {
        EntryKind::Symlink { to_dir: target.is_some_and(Metadata::is_dir) }
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

#[cfg(unix)]
fn is_executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    const ANY_EXECUTE: u32 = 0o111;
    meta.is_file() && meta.permissions().mode() & ANY_EXECUTE != 0
}

#[cfg(not(unix))]
fn is_executable(_: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn permissions(meta: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    const PERMISSION_BITS: u32 = 0o7777;
    meta.permissions().mode() & PERMISSION_BITS
}

#[cfg(not(unix))]
fn permissions(_: &Metadata) -> u32 {
    0
}
