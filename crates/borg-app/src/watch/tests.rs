use super::*;
use borg_core::{AlertsRequest, WatchFinding};

fn alert_log(alerts: usize) -> AlertLog {
    let mut log = AlertLog::watching(WatchStatus { source: WatchSource::Kernel, ..WatchStatus::default() });
    for index in 0..alerts {
        let process = StartedProcess {
            key: ProcessKey { pid: 900 + index as u32, start: 10 },
            name: "sh".into(),
            program: Some("/bin/sh".into()),
            command: vec!["sh".into(), "-i".into()],
            user: "www-data".into(),
            ..StartedProcess::default()
        };
        let finding = WatchFinding::ShellOnSocket { shell: "sh".into() };
        log.add(SystemTime::UNIX_EPOCH, finding, Response::Frozen, process);
    }
    log
}

#[tokio::test]
async fn opening_the_page_reads_the_log_and_marks_it_read_while_reading_again_does_not() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    log::write_log(state, &alert_log(3)).unwrap();

    let read = answer(state, &AlertsRequest::Read).await.unwrap();
    assert_eq!(read.log.alerts.len(), 3);
    assert_eq!(read.log.unread(), 3, "reading again leaves unread alerts unread");
    assert_eq!(read.notice, None);

    let opened = answer(state, &AlertsRequest::Open).await.unwrap();
    assert_eq!(opened.log.unread(), 0);
    assert_eq!(log::read_log(state).unread(), 0, "what the user has seen stays seen");
}

#[tokio::test]
async fn allowing_an_alert_writes_an_allowance_the_watcher_takes_in() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    log::write_log(state, &alert_log(1)).unwrap();
    let alert = log::read_log(state).alerts[0].clone();
    let request = AlertsRequest::Allow { alert: alert.id, allowance: alert.allowance() };

    let reply = answer(state, &request).await.unwrap();
    assert_eq!(reply.log.alerts[0].state, AlertState::Allowed);
    assert!(reply.notice.is_some());
    let allowed = allowances(&Settings::default(), state);
    assert!(allowed.iter().any(|allowance| allowance.covers(&alert.process)), "the start is allowed from now on");
    assert_eq!(log::read_log(state).alerts[0].state, AlertState::Allowed, "and it is written down");

    let from_the_configuration = Settings {
        allow: vec![Allowance { user: Some("www-data".into()), ..Allowance::default() }],
        ..Settings::default()
    };
    assert_eq!(allowances(&from_the_configuration, state).len(), 2, "the configuration's allowances come first");
}

#[tokio::test]
async fn acting_on_a_process_that_has_gone_says_so_and_writes_it_down() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    log::write_log(state, &alert_log(1)).unwrap();
    let alert = log::read_log(state).alerts[0].clone();
    let request = AlertsRequest::Resume { alert: alert.id, process: alert.process.key };
    let reply = answer(state, &request).await.unwrap();
    assert_eq!(reply.log.alerts[0].state, AlertState::Gone);
    assert!(reply.notice.is_some_and(|notice| notice.contains("sh")));
}

#[test]
fn the_polling_source_reports_each_new_process_once_and_forgets_those_that_have_gone() {
    let folder = tempfile::tempdir().unwrap();
    let processes = facts::Processes::at(folder.path());
    let uid = rustix::process::getuid().as_raw();
    let add = |pid: u32| {
        let at = folder.path().join(pid.to_string());
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(at.join("stat"), format!("{pid} (sh) S 1 1 1 0 {}\n", "0 ".repeat(20))).unwrap();
        std::fs::write(at.join("status"), format!("Uid:\t{uid}\t{uid}\t{uid}\t{uid}\n")).unwrap();
    };
    add(10);
    let mut polling = Polling::new(uid, &processes);
    assert_eq!(polling.started(&processes), None, "what is already running has not just started");
    add(20);
    add(30);
    assert_eq!(polling.started(&processes), Some(20));
    assert_eq!(polling.started(&processes), Some(30));
    assert_eq!(polling.started(&processes), None);
    std::fs::remove_dir_all(folder.path().join("20")).unwrap();
    assert_eq!(polling.started(&processes), None, "a process that has gone is forgotten");
    add(20);
    assert_eq!(polling.started(&processes), Some(20), "and its number counts as new if it comes back");
}

#[test]
fn the_hash_of_a_program_is_its_contents_and_nothing_else() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("miner");
    std::fs::write(&program, b"borg").unwrap();
    assert_eq!(
        hash_of(&program).as_deref(),
        Some("8c98028bf435d0af5ab8074969a7e4f7e136b30b53df80685e5763ea415d8fbe"),
        "a known file hashes to the same value every time"
    );
    assert_eq!(hash_of(&folder.path().join("missing")), None);
}

#[tokio::test]
async fn the_examiner_reads_a_program_away_from_the_watcher_and_says_which_alerts_it_was_for() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("miner");
    std::fs::write(&program, b"borg").unwrap();
    let (jobs, mut found) = examiner(crate::rule_scan::compile(None));
    jobs.send(Job { alerts: vec![3, 4], program }).unwrap();
    let found = found.recv().await.expect("the examiner answers");
    assert_eq!(found.alerts, [3, 4]);
    assert_eq!(found.program_hash.as_deref(), Some("8c98028bf435d0af5ab8074969a7e4f7e136b30b53df80685e5763ea415d8fbe"));
    assert_eq!(found.missing, None);
}

#[test]
fn an_alert_reads_as_one_line_for_the_command_line() {
    let log = alert_log(1);
    let line = describe(&log.alerts[0]);
    assert!(line.contains("sh"), "{line}");
    assert!(line.contains(AlertState::Frozen.label()), "{line}");
}
