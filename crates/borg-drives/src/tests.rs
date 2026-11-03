use std::{path::Path, time::Duration};

use crate::{Drive, DriveKind, Drives, Remember, Work};

const GIGABYTE: u64 = 1_000_000_000;

fn drive(id: &str, kind: DriveKind) -> Drive {
    Drive { id: id.into(), kind, filesystem: "test".into(), clones: false }
}

#[test]
fn kinds_of_drive_start_from_their_usual_speeds() {
    let speeds: Vec<u64> = [DriveKind::Memory, DriveKind::SolidState, DriveKind::HardDrive, DriveKind::Network]
        .into_iter()
        .chain([DriveKind::Remote])
        .map(|kind| kind.typical().write)
        .collect();
    assert!(speeds.windows(2).all(|pair| pair[0] > pair[1]), "{speeds:?}");
    assert_eq!(Drives::remote("devbox").kind, DriveKind::Remote);
}

#[test]
fn the_busiest_drive_sets_how_long_work_takes() {
    let drives = Drives::default();
    let (hard, ssd) = (drive("hdd", DriveKind::HardDrive), drive("ssd", DriveKind::SolidState));
    let copy = Work::default().read(&hard, 6 * GIGABYTE).write(&ssd, 6 * GIGABYTE);
    let hard_seconds = 6 * GIGABYTE / DriveKind::HardDrive.typical().read;
    assert_eq!(drives.estimate(&copy).as_secs(), hard_seconds);
    assert_eq!(drives.slowest(&copy), Some(hard.clone()));
    let packing = copy.clone().computing(Duration::from_secs(30));
    assert_eq!(drives.estimate(&packing).as_secs(), hard_seconds + 30);
    let same = Work::default().read(&hard, GIGABYTE).write(&hard, GIGABYTE);
    let both = GIGABYTE as f64 / DriveKind::HardDrive.typical().read as f64
        + GIGABYTE as f64 / DriveKind::HardDrive.typical().write as f64;
    assert_eq!(drives.estimate(&same).as_secs(), both as u64, "one drive reads and writes in turn");
    assert_eq!(drives.estimate(&Work::default()), Duration::ZERO);
}

#[test]
fn finished_work_teaches_the_speed_of_the_drive_that_held_it_up() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    let drives = Drives::load(state.path());
    let hard = drive("/dev/disk9", DriveKind::HardDrive);
    let work = Work::default().write(&hard, 10 * GIGABYTE);
    let expected = drives.estimate(&work);
    drives.learn(&work, expected * 4);
    let slower = drives.rates(&hard).write;
    assert!(slower < DriveKind::HardDrive.typical().write, "{slower}");
    assert_eq!(drives.rates(&hard).read, DriveKind::HardDrive.typical().read, "only what the work did");
    assert!(drives.facts(state.path()).drive.kind != DriveKind::Remote);
    assert_eq!(Drives::load(state.path()).rates(&hard).write, slower, "kept for the next run");

    drives.learn(&Work::default().write(&hard, 1024), Duration::from_secs(60));
    drives.learn(&work, Duration::from_millis(10));
    assert_eq!(drives.rates(&hard).write, slower, "small or short work teaches nothing");
    let replaced = drive("/dev/disk9", DriveKind::SolidState);
    assert_eq!(drives.rates(&replaced), DriveKind::SolidState.typical(), "another drive under the same name");
    Ok(())
}

#[test]
fn answers_keep_borg_from_asking_about_a_file_or_a_kind_of_drive() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    let drives = Drives::load(state.path());
    let archive = Path::new("/data/big.7z");
    assert!(drives.asks(Some(archive), DriveKind::HardDrive) && drives.kept() == 0);
    drives.remember(Remember::File, Some(archive), DriveKind::HardDrive);
    assert!(!drives.asks(Some(archive), DriveKind::HardDrive));
    assert!(drives.asks(Some(Path::new("/data/other.7z")), DriveKind::HardDrive));
    drives.remember(Remember::Kind, None, DriveKind::Network);
    assert!(!drives.asks(None, DriveKind::Network) && drives.asks(None, DriveKind::SolidState));
    let again = Drives::load(state.path());
    assert!(!again.asks(Some(archive), DriveKind::SolidState) && !again.asks(None, DriveKind::Network));
    assert_eq!(again.kept(), 2);
    again.ask_again();
    assert!(again.asks(Some(archive), DriveKind::Network) && again.kept() == 0);
    assert!(Drives::load(state.path()).asks(None, DriveKind::Network));
    Ok(())
}

#[test]
fn a_file_of_another_format_or_too_large_starts_over() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    std::fs::write(state.path().join("drives.json"), br#"{"format": 99, "quiet": {"kinds": ["memory"]}}"#)?;
    assert!(Drives::load(state.path()).asks(None, DriveKind::Memory));
    std::fs::write(state.path().join("drives.json"), vec![b' '; 2 * 1024 * 1024])?;
    assert_eq!(Drives::load(state.path()).kept(), 0);
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn the_system_names_the_filesystem_of_a_folder() -> std::io::Result<()> {
    let folder = tempfile::tempdir()?;
    let drives = Drives::default();
    let found = drives.drive(folder.path());
    assert!(!found.filesystem.is_empty() && !found.id.is_empty(), "{found:?}");
    assert_eq!(drives.drive(&folder.path().join("not/yet/written")), found, "a new file lands on its folder's drive");
    Ok(())
}
