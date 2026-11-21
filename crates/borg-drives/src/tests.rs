use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

use crate::{Drive, DriveKind, Drives, Remember, Work};

const GIGABYTE: u64 = 1_000_000_000;
/// Borgs writing `drives.json` at the same moment, and the writes each makes.
const WRITERS: usize = 4;
const WRITES: usize = 25;
const PERMISSIONS: u32 = 0o777;
const OWNER_ONLY: u32 = 0o600;
const EVERYONE_READS: u32 = 0o644;

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

/// Two Borgs open on one machine read `drives.json` once each. What one answers or learns must
/// survive the other's next write, and forgetting the answers in one must not come back with it.
#[test]
fn two_borgs_keep_what_the_other_answered_and_learned() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    let archive = state.path().join("big.zip");
    let first = Drives::load(state.path());
    let second = Drives::load(state.path());
    first.remember(Remember::File, Some(&archive), DriveKind::HardDrive);
    second.remember(Remember::Kind, None, DriveKind::Network);
    let hard = drive("/dev/disk9", DriveKind::HardDrive);
    let work = Work::default().write(&hard, 10 * GIGABYTE);
    second.learn(&work, second.estimate(&work) * 4);
    let slower = second.rates(&hard).write;
    first.remember(Remember::Kind, None, DriveKind::Remote);

    let both = Drives::load(state.path());
    assert!(!both.asks(Some(&archive), DriveKind::HardDrive), "the first Borg's answer about the archive");
    assert!(!both.asks(None, DriveKind::Network), "the second Borg's answer about network shares");
    assert_eq!(both.rates(&hard).write, slower, "and the speed the second one learned");
    assert_eq!(first.rates(&hard).write, slower, "the first Borg took them in as it wrote");

    first.ask_again();
    second.remember(Remember::Kind, None, DriveKind::Memory);
    let after = Drives::load(state.path());
    assert!(after.asks(Some(&archive), DriveKind::HardDrive) && after.asks(None, DriveKind::Network));
    assert!(!after.asks(None, DriveKind::Memory), "only what was answered since");
    assert_eq!(after.kept(), 1);
    Ok(())
}

#[test]
fn drives_json_belongs_to_its_owner_alone() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    let file = state.path().join("drives.json");
    let mode = |file: &Path| std::fs::metadata(file).map(|meta| meta.permissions().mode() & PERMISSIONS);
    Drives::load(state.path()).remember(Remember::Kind, None, DriveKind::Network);
    assert_eq!(mode(&file)?, OWNER_ONLY);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(EVERYONE_READS))?;
    let again = Drives::load(state.path());
    assert_eq!(mode(&file)?, OWNER_ONLY, "a file an older Borg left readable to others");
    assert!(!again.asks(None, DriveKind::Network));
    Ok(())
}

/// Several Borgs writing at once each stage under a name of their own, so the file always reads
/// whole and nothing staged stays behind.
#[test]
fn borgs_writing_at_once_never_leave_a_half_written_file() -> std::io::Result<()> {
    let state = tempfile::tempdir()?;
    let file = state.path().join("drives.json");
    assert_ne!(crate::drives::staging(&file), crate::drives::staging(&file));
    std::thread::scope(|scope| {
        for writer in 0..WRITERS {
            let folder = state.path();
            scope.spawn(move || {
                let drives = Drives::load(folder);
                for write in 0..WRITES {
                    let archive = folder.join(format!("{writer}-{write}.zip"));
                    drives.remember(Remember::File, Some(&archive), DriveKind::HardDrive);
                }
            });
        }
        scope.spawn(|| {
            for _ in 0..WRITERS * WRITES {
                if let Ok(bytes) = std::fs::read(&file) {
                    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok(), "a half-written file");
                }
            }
        });
    });
    let names: Vec<_> = std::fs::read_dir(state.path())?.flatten().map(|entry| entry.file_name()).collect();
    assert_eq!(names, ["drives.json"], "nothing staged stays behind");
    assert!(Drives::load(state.path()).kept() > 0);
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
