use borg_i18n::tr;
use std::{
    fs, io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Result, ensure};
use borg_core::Content;
use borg_gfx::{holds_picture, is_picture};
use borg_view::{FileSource, Pager, Source};

use crate::app::{picture, span};

pub fn open(path: &Path, size: (usize, usize), cell: (u16, u16), cancelled: Arc<AtomicBool>) -> Result<Content> {
    ensure!(fs::metadata(path)?.is_file(), tr!(not_a_regular_file));
    // A file named like a picture that holds something else reads as text.
    if is_picture(path) && holds_picture(path) {
        let bounds = (span(size.0, cell.0), span(size.1, cell.1));
        return picture(path, bounds).map(Content::Image);
    }
    let source = Arc::new(LoadingSource { file: FileSource::open(path)?, cancelled, loading: AtomicBool::new(true) });
    let pager = Pager::for_path(source.clone(), path, size.0, size.1)?;
    source.loading.store(false, Ordering::Relaxed);
    Ok(Content::Text(pager))
}

struct LoadingSource {
    file: FileSource,
    cancelled: Arc<AtomicBool>,
    loading: AtomicBool,
}

impl Source for LoadingSource {
    fn len(&self) -> u64 {
        self.file.len()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if self.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "document loading cancelled"));
        }
        self.file.read_at(offset, buf)
    }

    fn is_cancelled(&self) -> bool {
        self.loading.load(Ordering::Relaxed) && self.cancelled.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use borg_core::{Aside, Peek};

    use super::*;

    #[test]
    fn quick_view_and_viewer_render_the_same_documents() {
        let folder = tempfile::tempdir().unwrap();
        for (name, source) in [
            ("main.rs", "fn main() {\n    println!(\"hello\");\n}\n"),
            ("guide.md", "# Heading\n\nA **bold** word.\n\n```rust\nfn main() {}\n```\n"),
            ("dump.bin", "first line\n\0binary"),
            ("photo.jpg", "notes kept under a picture's name\n"),
        ] {
            let path = folder.path().join(name);
            fs::write(&path, source).unwrap();
            let cancelled = Arc::new(AtomicBool::new(false));
            let Peek::Text(mut preview) =
                crate::peek::peek(Aside::QuickView, &path, None, (45, 20), (8, 16), cancelled.clone())
            else {
                panic!("expected text preview")
            };
            let Content::Text(viewer) = open(&path, (132, 35), (8, 16), cancelled).unwrap() else {
                panic!("expected text viewer")
            };
            preview.resize(132, 35).unwrap();
            assert_eq!(preview.rows(), viewer.rows(), "{name}");
            assert_eq!(preview.rich_rows(), viewer.rich_rows(), "{name}");
            assert_eq!(preview.formatted(), viewer.formatted(), "{name}");
            assert_eq!(preview.language(), viewer.language(), "{name}");
        }
    }

    #[test]
    fn a_contents_result_previews_around_its_match_deep_in_a_large_file_and_in_markdown_source() {
        const HAY: usize = 256 * 1024;
        let folder = tempfile::tempdir().unwrap();
        let query = borg_view::Query { text: "needle".into(), ..Default::default() };
        for (name, text) in [
            ("huge.txt", "hay\n".repeat(HAY) + "the needle\n" + &"hay\n".repeat(HAY)),
            ("notes.md", "# Notes\n\nA **needle** in the text.\n".to_owned()),
        ] {
            let path = folder.path().join(name);
            fs::write(&path, &text).unwrap();
            let offset = text.find("needle").unwrap() as u64;
            let at = borg_core::FirstMatch { query: query.clone(), offset };
            let cancelled = Arc::new(AtomicBool::new(false));
            let Peek::Text(pager) = crate::peek::peek(Aside::QuickView, &path, Some(&at), (45, 20), (8, 16), cancelled)
            else {
                panic!("expected text preview")
            };
            assert_eq!(pager.current_match(), Some(offset..offset + 6), "{name}");
            assert!(pager.shows_matches() && !pager.formatted(), "{name}: marked, in the source the search read");
            assert!(pager.rows().iter().any(|row| row.contains("needle")), "{name}");
        }
    }

    #[test]
    fn completed_documents_remain_scrollable_after_the_load_is_cancelled() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("notes.txt");
        fs::write(&path, "A line of text\n".repeat(100)).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let Content::Text(mut pager) = open(&path, (45, 20), (8, 16), cancelled.clone()).unwrap() else {
            panic!("expected text viewer")
        };
        cancelled.store(true, Ordering::Relaxed);
        pager.page_down().unwrap();
        assert!(pager.offset() > 0);
    }
}
