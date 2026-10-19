use std::{fs::File, io, os::unix::fs::FileExt, path::Path};

pub trait Source: Send + Sync {
    fn len(&self) -> u64;

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug)]
pub struct FileSource {
    file: File,
    len: u64,
}

impl FileSource {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let available = usize::try_from(self.len.saturating_sub(offset)).unwrap_or(usize::MAX).min(buf.len());
        let mut filled = 0;
        while filled < available {
            match self.file.read_at(&mut buf[filled..available], offset + filled as u64)? {
                0 => break,
                n => filled += n,
            }
        }
        Ok(filled)
    }
}

impl Source for Vec<u8> {
    fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.as_slice().len());
        let chunk = &self[start..];
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        Ok(n)
    }
}
