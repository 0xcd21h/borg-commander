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
    log::write_log(state, &mut alert_log(3)).unwrap();

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
    log::write_log(state, &mut alert_log(1)).unwrap();
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
    log::write_log(state, &mut alert_log(1)).unwrap();
    let alert = log::read_log(state).alerts[0].clone();
    let request = AlertsRequest::Resume { alert: alert.id, process: alert.process.key };
    let reply = answer(state, &request).await.unwrap();
    assert_eq!(reply.log.alerts[0].state, AlertState::Gone);
    assert!(reply.notice.is_some_and(|notice| notice.contains("sh")));
}

#[tokio::test]
async fn what_the_page_writes_survives_the_watchers_next_alert() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    log::write_log(state, &mut alert_log(1)).unwrap();
    let watching = log::read_log(state).watch;
    answer(state, &AlertsRequest::Open).await.unwrap();

    let next = alert_log(1).alerts[0].clone();
    let log = change_log(state, &watching, |log| {
        log.add(SystemTime::UNIX_EPOCH, next.finding, next.response, next.process);
    })
    .unwrap();
    assert_eq!(log.alerts.len(), 2);
    assert_eq!(log.unread(), 1, "only the new alert is unread");
    assert_eq!(log::read_log(state), log, "and that is what the state folder holds");
}

/// A log Borg cannot read shows as an empty page. Opening the page keeps it aside under a name
/// of its own, and the new log says where while that file is there, whoever wrote it next.
#[tokio::test]
async fn a_log_borg_cannot_read_is_kept_aside_and_the_new_log_says_where() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    let damaged = b"{\"alerts\":[{\"id\":1,".to_vec();
    std::fs::write(log::log_path(state), &damaged).unwrap();

    let read = answer(state, &AlertsRequest::Read).await.unwrap();
    assert_eq!((read.log, read.notice), (AlertLog::default(), None));
    assert_eq!(std::fs::read(log::log_path(state)).unwrap(), damaged, "reading again moves nothing");

    let opened = answer(state, &AlertsRequest::Open).await.unwrap();
    let aside = state.join(borg_core::ALERT_LOG_ASIDE);
    assert_eq!(opened.log.kept_aside.as_ref(), Some(&aside));
    assert_eq!(std::fs::read(&aside).unwrap(), damaged);

    let next = alert_log(1).alerts[0].clone();
    let log = change_log(state, &opened.log.watch, |log| {
        log.add(SystemTime::UNIX_EPOCH, next.finding, next.response, next.process);
    })
    .unwrap();
    assert_eq!(log.kept_aside.as_ref(), Some(&aside), "the watcher's next alert keeps saying where");
    assert_eq!(log::read_log(state), log, "the new log holds the new alert");
    assert_eq!(std::fs::read(&aside).unwrap(), damaged, "and the old one is still there as it was");

    std::fs::remove_file(&aside).unwrap();
    assert_eq!(log::read_log(state).kept_aside, None, "once the user removes that file the page stops saying so");
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
    let mut polling = Polling::new(Some(uid), &processes);
    let mut everyone = Polling::new(None, &processes);
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
    assert_eq!(
        (everyone.started(&processes), everyone.started(&processes)),
        (Some(20), Some(30)),
        "run as root, Borg looks at every process the same way"
    );
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

#[test]
fn the_command_line_warning_says_how_to_set_delivery_up_and_try_it() {
    let warned = alert_log(0).unsent(None, SystemTime::now()).expect("nothing takes alerts");
    let advice = unsent_advice(warned);
    assert!(advice.starts_with("Alerts stay on this machine."), "{advice}");
    assert!(advice.contains("borg service watch notify"), "{advice}");
}

/// An allowances file that is there but that Borg cannot read is left as it is: allowing one
/// more start would otherwise write a list of one over every start allowed before.
#[tokio::test]
async fn allowing_a_start_never_writes_over_allowances_borg_cannot_read() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    log::write_log(state, &mut alert_log(1)).unwrap();
    let damaged = b"[{\"program\":".to_vec();
    std::fs::write(log::allowed_path(state), &damaged).unwrap();
    let allowance = alert_log(1).alerts[0].allowance();

    let refused = answer(state, &AlertsRequest::Allow { alert: 1, allowance }).await;
    assert!(refused.is_err(), "the page says what went wrong");
    assert_eq!(std::fs::read(log::allowed_path(state)).unwrap(), damaged);
    assert_eq!(log::read_log(state).alerts[0].state, AlertState::Frozen, "and the alert is not marked allowed");
}

/// The page and the watcher change one log. Each change starts from the log as the state folder
/// holds it under a lock, so with both writing at once every alert and every mark is kept.
#[test]
fn changes_to_the_log_from_several_writers_at_once_are_all_kept() {
    const WRITERS: usize = 4;
    const ALERTS_EACH: usize = 10;
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    std::thread::scope(|scope| {
        for _ in 0..WRITERS {
            scope.spawn(|| {
                for _ in 0..ALERTS_EACH {
                    let next = alert_log(1).alerts[0].clone();
                    log::change_log(state, |log| {
                        log.add(SystemTime::UNIX_EPOCH, next.finding, Response::Alerted, next.process);
                    })
                    .unwrap();
                    log::change_log(state, AlertLog::mark_seen).unwrap();
                }
            });
        }
    });
    let log = log::read_log(state);
    assert_eq!(log.alerts.len(), WRITERS * ALERTS_EACH);
    let ids: HashSet<u64> = log.alerts.iter().map(|alert| alert.id).collect();
    assert_eq!(ids.len(), WRITERS * ALERTS_EACH, "no two alerts share an id");
}

fn sources_after_the_root_watcher(link: root::RootLink, root: UnboundedReceiver<StartedProcess>) -> Sources {
    Sources {
        kernel: None,
        kernel_source: WatchSource::Off,
        root: Some(root),
        link: Some(link),
        own: None,
        uid: Some(rustix::process::getuid().as_raw()),
        heard_root: true,
    }
}

/// The root watcher may stop at any time. Borg then looks at the user's own processes again
/// instead of waiting for a watcher that says nothing more.
#[tokio::test]
async fn borg_looks_at_its_own_processes_again_once_the_root_watcher_has_gone() {
    const SETTLE: Duration = Duration::from_millis(100);
    let folder = tempfile::tempdir().unwrap();
    let (link, root) = root::RootLink::listen(folder.path()).await.unwrap();
    let mut sources = sources_after_the_root_watcher(link, root);
    let processes = Processes::default();
    let _ = tokio::time::timeout(SETTLE, sources.next(&processes)).await;
    assert!(sources.own.is_some() && !sources.heard_root);
    assert_eq!(sources.label(), WatchSource::OwnProcesses);
}

/// With no root watcher connected, Borg signals a process itself at once, where asking a
/// watcher that is not there would wait and then do it whenever the watcher came back.
#[tokio::test]
async fn a_process_is_signalled_by_borg_itself_while_no_root_watcher_is_connected() {
    let folder = tempfile::tempdir().unwrap();
    let (link, root) = root::RootLink::listen(folder.path()).await.unwrap();
    let mut sources = sources_after_the_root_watcher(link, root);
    let nobody = ProcessKey { pid: i32::MAX as u32, start: 0 };
    let outcome = sources.act(&Processes::default(), root::Act::Freeze, nobody).await;
    assert_eq!(outcome, ControlOutcome::Gone, "Borg looked for the process itself");
}
