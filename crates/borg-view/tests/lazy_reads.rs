use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use borg_view::{Pager, Source};

const LARGE_FILE: u64 = 5 << 30;
const LINE: &[u8] = b"let value = 42;\n";
const OPEN_WINDOW: u64 = 32 * 1024;

#[derive(Default)]
struct Reads {
    calls: usize,
    furthest: u64,
}

#[derive(Default)]
struct Repeated {
    reads: Arc<Mutex<Reads>>,
    cancelled: AtomicBool,
    single_line: bool,
}

impl Source for Repeated {
    fn len(&self) -> u64 {
        LARGE_FILE
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let count = (LARGE_FILE.saturating_sub(offset) as usize).min(buf.len());
        for (i, byte) in buf[..count].iter_mut().enumerate() {
            *byte = if self.single_line { b'x' } else { LINE[(offset as usize + i) % LINE.len()] };
        }
        let mut reads = self.reads.lock().unwrap();
        reads.calls += 1;
        reads.furthest = reads.furthest.max(offset + count as u64);
        Ok(count)
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

fn open(width: usize, height: usize) -> (Pager, Arc<Mutex<Reads>>) {
    let reads = Arc::new(Mutex::new(Reads::default()));
    let source = Arc::new(Repeated { reads: reads.clone(), ..Repeated::default() });
    (Pager::for_path(source, Path::new("large.rs"), width, height).unwrap(), reads)
}

#[test]
fn opening_either_view_only_reads_near_the_first_page() {
    for size in [(45, 20), (132, 35)] {
        let (pager, reads) = open(size.0, size.1);
        assert_eq!(pager.len(), LARGE_FILE);
        assert!(
            reads.lock().unwrap().furthest < OPEN_WINDOW,
            "opening a viewport must not pre-highlight a large chunk of the file"
        );
    }
}

#[test]
fn drawing_progress_does_not_read_the_file() {
    let (pager, reads) = open(45, 20);
    let before = reads.lock().unwrap().calls;
    for _ in 0..3 {
        assert_eq!(pager.percent(), 0);
    }
    assert_eq!(reads.lock().unwrap().calls, before, "rendering the footer must not scan the file tail");
}

#[test]
fn scrolling_forward_does_not_scan_the_file_tail() {
    let (mut pager, reads) = open(45, 20);
    pager.page_down().unwrap();
    assert!(reads.lock().unwrap().furthest < OPEN_WINDOW, "forward scrolling must only read nearby lines");
    assert!(pager.offset() > 0);
}

#[test]
fn cancelled_documents_stop_before_more_file_reads() {
    let source = Arc::new(Repeated::default());
    let mut pager = Pager::for_path(source.clone(), Path::new("large.rs"), 45, 20).unwrap();
    let before = source.reads.lock().unwrap().calls;
    source.cancelled.store(true, Ordering::Relaxed);
    assert_eq!(pager.page_down().unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert_eq!(source.reads.lock().unwrap().calls, before);
}

#[test]
fn giant_wrapped_lines_do_not_expand_the_highlighting_window_to_many_lines() {
    let source = Arc::new(Repeated { single_line: true, ..Repeated::default() });
    let pager = Pager::for_path(source.clone(), Path::new("large.js"), 45, 20).unwrap();
    assert_eq!(pager.rows().len(), 20);
    assert!(source.reads.lock().unwrap().furthest < borg_view::MAX_LINE * 2);
}
