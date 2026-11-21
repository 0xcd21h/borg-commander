use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use borg_core::ReportError;

use super::save;

const OWNER_ONLY: u32 = 0o600;
const MODE_BITS: u32 = 0o777;
const REPORT: &str = "# Port scan of 192.0.2.60\n";
const NEWER: &str = "# Port scan of 192.0.2.61\n";

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & MODE_BITS
}

#[test]
fn a_new_report_is_readable_by_its_owner_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("port-scan-192.0.2.60-2026-01-15.md");
    assert_eq!(save(&path, REPORT, false, None), Ok(path.clone()));
    assert_eq!(fs::read_to_string(&path).unwrap(), REPORT);
    assert_eq!(mode(&path), OWNER_ONLY);
}

#[test]
fn a_file_that_is_there_is_left_alone_until_replacing_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.md");
    fs::write(&path, REPORT).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(save(&path, NEWER, false, None), Err(ReportError::Exists));
    assert_eq!(fs::read_to_string(&path).unwrap(), REPORT);
    assert_eq!(save(&path, NEWER, true, None), Ok(path.clone()));
    assert_eq!(fs::read_to_string(&path).unwrap(), NEWER);
    assert_eq!(mode(&path), OWNER_ONLY);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_leading_tilde_is_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let written = save(Path::new("~/neighbors-2026-01-15.csv"), REPORT, false, Some(home.path())).unwrap();
    assert_eq!(written, home.path().join("neighbors-2026-01-15.csv"));
    assert_eq!(fs::read_to_string(written).unwrap(), REPORT);
}

#[test]
fn a_missing_folder_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing").join("report.json");
    assert!(matches!(save(&path, REPORT, false, None), Err(ReportError::Failed(_))));
    assert!(!dir.path().join("missing").exists());
}
