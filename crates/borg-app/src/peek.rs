use std::{
    fs::{self, File, Metadata},
    io::Read as _,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
    sync::Arc,
};

use anyhow::Result;
use borg_core::{Aside, Facts, Nature, Peek};
use borg_gfx::is_picture;
use borg_vfs::{Entry, entries};
use borg_view::{FileSource, Pager};

use crate::app::{picture, span};

const SNIFF_BYTES: u64 = 8 * 1024;
const NUL: u8 = 0;
const PERMISSION_BITS: u32 = 0o7777;

pub fn peek(aside: Aside, path: &Path, size: (usize, usize), cell: (u16, u16)) -> Peek {
    let peeked = match aside {
        Aside::QuickView => quick_view(path, size, cell),
        Aside::Info => info(path).map(Peek::Info),
    };
    peeked.unwrap_or_else(|e| Peek::Unavailable(format!("{e:#}")))
}

fn quick_view(path: &Path, (width, height): (usize, usize), (cell_width, cell_height): (u16, u16)) -> Result<Peek> {
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        return folder(path, height);
    }
    if is_picture(path) {
        let bounds = (span(width, cell_width), span(height, cell_height));
        return Ok(Peek::Picture(picture(path, bounds)?));
    }
    if is_binary(path)? {
        return Ok(Peek::Binary);
    }
    let pager = Pager::for_path(Arc::new(FileSource::open(path)?), path, width, height)?;
    Ok(Peek::Text(pager))
}

fn is_binary(path: &Path) -> Result<bool> {
    let mut head = Vec::new();
    File::open(path)?.take(SNIFF_BYTES).read_to_end(&mut head)?;
    Ok(head.contains(&NUL))
}

fn folder(path: &Path, shown: usize) -> Result<Peek> {
    let mut all: Vec<Entry> = entries(path)?.collect();
    all.sort_by_cached_key(|entry| (!entry.is_dir(), entry.name.to_lowercase()));
    let folders = all.iter().filter(|entry| entry.is_dir()).count();
    let bytes = all.iter().filter(|entry| !entry.is_dir()).map(|entry| entry.size).sum();
    let files = all.len() - folders;
    all.truncate(shown);
    Ok(Peek::Folder { folders, files, bytes, entries: all })
}

fn nature(path: &Path, own: &Metadata) -> Nature {
    let kind = own.file_type();
    if kind.is_symlink() {
        fs::read_link(path).map_or(Nature::Special, Nature::Link)
    } else if kind.is_dir() {
        Nature::Folder
    } else if kind.is_file() {
        Nature::File
    } else {
        Nature::Special
    }
}

fn owner(uid: u32) -> String {
    uzers::get_user_by_uid(uid).map_or_else(|| uid.to_string(), |user| user.name().to_string_lossy().into_owned())
}

fn group(gid: u32) -> String {
    uzers::get_group_by_gid(gid).map_or_else(|| gid.to_string(), |group| group.name().to_string_lossy().into_owned())
}

fn disk(path: &Path) -> Option<(u64, u64)> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some((stats.f_bavail.saturating_mul(stats.f_frsize), stats.f_blocks.saturating_mul(stats.f_frsize)))
}

fn info(path: &Path) -> Result<Facts> {
    let own = fs::symlink_metadata(path)?;
    let effective = fs::metadata(path).unwrap_or_else(|_| own.clone());
    let items = effective.is_dir().then(|| fs::read_dir(path).map(Iterator::count).ok()).flatten();
    Ok(Facts {
        nature: nature(path, &own),
        size: effective.len(),
        permissions: own.permissions().mode() & PERMISSION_BITS,
        owner: owner(own.uid()),
        group: group(own.gid()),
        modified: own.modified().ok(),
        accessed: own.accessed().ok(),
        created: own.created().ok(),
        inode: own.ino(),
        links: own.nlink(),
        items,
        disk: disk(path),
    })
}
