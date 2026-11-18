use std::{
    fs::{self, File},
    io,
    os::unix::fs::FileExt as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use borg_core::{Content, EditorFile, Msg, Password, SaveError, SealedFile, Then, UnsealError, Viewer};
use borg_crypt::{age, pte, sealed};
use borg_view::{Pager, Source};
use zeroize::Zeroizing;

const HEAD_LEN: usize = 2048;
const TAIL_LEN: usize = 1024;
const AGE_EXTENSION: &str = "age";

pub fn detect(path: &Path) -> Option<sealed::Format> {
    let meta = fs::metadata(path).ok().filter(fs::Metadata::is_file)?;
    let len = usize::try_from(meta.len()).ok().filter(|len| *len <= sealed::FILE_LIMIT)?;
    let file = File::open(path).ok()?;
    let head = read_at(&file, 0, HEAD_LEN.min(len)).ok()?;
    let tail_start = len.saturating_sub(TAIL_LEN);
    let worth_reading = sealed::age_start(&head)
        || (len <= pte::FILE_LIMIT && pte::ends_like_pte(&read_at(&file, tail_start, len - tail_start).ok()?));
    if !worth_reading {
        return None;
    }
    sealed::detect(&read_at(&file, 0, len).ok()?)
}

pub fn open(file: SealedFile, password: &Password, size: (usize, usize)) -> Msg {
    let (text, seal, stamp) = match unseal(&file.path, password) {
        Ok(opened) => opened,
        Err(error) => return Msg::UnsealFailed { file, error },
    };
    match file.then {
        Then::View => match viewer(&file.path, text, size) {
            Ok(viewer) => Msg::Opened(viewer),
            Err(error) => Msg::UnsealFailed { file, error: UnsealError::Failed(error.to_string()) },
        },
        Then::Edit(slot) => match editable(text) {
            Some(text) => Msg::SealedEditorOpened { slot, file: EditorFile { path: file.path, text, stamp }, seal },
            None => Msg::UnsealFailed { file, error: UnsealError::NotText },
        },
    }
}

pub fn seal(seal: &sealed::Seal, text: &[u8], path: &Path) -> Result<Vec<u8>, SaveError> {
    let mut random = Zeroizing::new([0; sealed::RANDOM_LEN]);
    getrandom::fill(random.as_mut()).map_err(|error| SaveError::Failed(error.to_string()))?;
    seal.seal(text, &random)
        .map_err(|error| SaveError::Failed(borg_core::describe_unseal_error(&UnsealError::Sealed(error), path)))
}

fn unseal(
    path: &Path,
    password: &Password,
) -> Result<(Zeroizing<Vec<u8>>, sealed::Seal, borg_core::FileStamp), UnsealError> {
    let failed = |error: io::Error| UnsealError::Failed(error.to_string());
    let meta = fs::metadata(path).map_err(failed)?;
    if meta.len() > sealed::FILE_LIMIT as u64 {
        return Err(UnsealError::Sealed(sealed::Error::TooLong));
    }
    let bytes = fs::read(path).map_err(failed)?;
    let max = work_factor_limit().ok_or(UnsealError::Sealed(sealed::Error::NeedsMemory))?;
    let (text, seal) = sealed::unseal(&bytes, password.as_str(), max).map_err(UnsealError::Sealed)?;
    Ok((text, seal, crate::edit_file::stamp(&meta)))
}

fn work_factor_limit() -> Option<age::WorkFactor> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    age::WorkFactor::within(system.available_memory())
}

fn viewer(path: &Path, text: Zeroizing<Vec<u8>>, size: (usize, usize)) -> io::Result<Viewer> {
    let pager = Pager::for_path(Arc::new(Plaintext(text)), &shown_path(path), size.0, size.1)?;
    Ok(Viewer::new(path.to_path_buf(), Content::Text(pager)))
}

fn editable(text: Zeroizing<Vec<u8>>) -> Option<String> {
    if text.contains(&0) {
        return None;
    }
    std::str::from_utf8(&text).ok().map(str::to_owned)
}

fn shown_path(path: &Path) -> PathBuf {
    match path.extension() {
        Some(extension) if extension == AGE_EXTENSION => path.with_extension(""),
        _ => path.to_path_buf(),
    }
}

fn read_at(file: &File, offset: usize, len: usize) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0; len];
    file.read_exact_at(&mut buffer, offset as u64)?;
    Ok(buffer)
}

struct Plaintext(Zeroizing<Vec<u8>>);

impl Source for Plaintext {
    fn len(&self) -> u64 {
        Source::len(&*self.0)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        Source::read_at(&*self.0, offset, buf)
    }
}

#[cfg(test)]
mod tests;
