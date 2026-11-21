use std::thread;

use super::*;

const WRITERS: usize = 8;
const WRITES: usize = 200;
const LINES: usize = 64;

fn contents(writer: usize, write: usize) -> String {
    format!("{writer}.{write}\n").repeat(LINES)
}

/// Borgs and the service save the sessions file at the same moment. Each save replaces the
/// file whole, none fails, and no staged copy stays behind.
#[test]
fn writers_saving_one_file_at_once_each_replace_it_whole() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("sessions.json");
    thread::scope(|scope| {
        let writers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let file = &file;
                scope.spawn(move || -> Result<()> {
                    (0..WRITES).try_for_each(|write| write_privately(file, contents(writer, write).as_bytes()))
                })
            })
            .collect();
        writers.into_iter().try_for_each(|writer| writer.join().expect("a writer panicked"))
    })?;
    let saved = fs::read_to_string(&file)?;
    let first = saved.lines().next().unwrap_or_default();
    assert!(saved.lines().count() == LINES && saved.lines().all(|line| line == first), "a mixed save: {saved}");
    let left: Vec<_> =
        fs::read_dir(dir.path())?.map(|entry| entry.map(|entry| entry.file_name())).collect::<Result<_, _>>()?;
    assert_eq!(left, [file.file_name().unwrap_or_default()]);
    Ok(())
}

/// The configuration holds tokens and the state says what the user did, so a save leaves the
/// file readable by its owner alone, whatever it was before.
#[test]
fn a_saved_file_is_its_owners_alone_even_where_it_replaces_one_others_could_read() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    const OWNER_ONLY: u32 = 0o600;
    const READABLE_BY_ALL: u32 = 0o644;
    const PERMISSIONS: u32 = 0o777;
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    fs::write(&file, "old")?;
    fs::set_permissions(&file, fs::Permissions::from_mode(READABLE_BY_ALL))?;
    write_privately(&file, b"new")?;
    assert_eq!(fs::read(&file)?, b"new");
    assert_eq!(fs::metadata(&file)?.permissions().mode() & PERMISSIONS, OWNER_ONLY);
    Ok(())
}
