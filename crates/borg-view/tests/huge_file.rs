use std::{
    fs::OpenOptions,
    io::Result,
    os::unix::fs::FileExt,
    sync::Arc,
    time::{Duration, Instant},
};

use borg_view::{FileSource, Pager};

const FIVE_GIB: u64 = 5 << 30;
const TAIL: &[u8] = b"\nlast line of a very large log\n";
const BUDGET: Duration = Duration::from_secs(5);

#[test]
fn opens_and_jumps_through_a_five_gigabyte_file_without_loading_it() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("huge.log");
    let file = OpenOptions::new().create_new(true).write(true).open(&path)?;
    file.write_all_at(b"first line\nsecond line\n", 0)?;
    file.set_len(FIVE_GIB)?;
    file.write_all_at(TAIL, FIVE_GIB - TAIL.len() as u64)?;
    drop(file);

    let started = Instant::now();
    let mut pager = Pager::new(Arc::new(FileSource::open(&path)?), 100, 20)?;
    assert_eq!(pager.len(), FIVE_GIB);
    assert_eq!(&pager.rows()[..2], ["first line", "second line"]);

    pager.end()?;
    assert_eq!(pager.rows().last().map(String::as_str), Some("last line of a very large log"));
    assert_eq!(pager.percent(), 100);

    pager.page_up()?;
    pager.home()?;
    assert_eq!(pager.rows()[0], "first line");
    assert!(started.elapsed() < BUDGET, "took {:?}", started.elapsed());
    Ok(())
}
