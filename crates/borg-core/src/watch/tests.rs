use super::*;
use crate::{Appearance, ConfirmPurpose, Msg, Overlay, ProcessKey, Settings, Startup, ThemeChoice, update};
use borg_rules::Level;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

const NOW: Duration = Duration::from_secs(1_800_000_000);
const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

fn moment(after: Duration) -> SystemTime {
    SystemTime::UNIX_EPOCH + NOW + after
}

fn ancestor(pid: u32, program: &str, terminal: bool) -> Ancestor {
    Ancestor {
        pid,
        name: PathBuf::from(program).file_name().unwrap().to_string_lossy().into_owned(),
        program: Some(program.into()),
        terminal,
        command: program.to_owned(),
    }
}

/// A process started by `parent`, with a terminal only where it is said.
fn started(pid: u32, program: &str, parent: Option<Ancestor>) -> StartedProcess {
    let name = PathBuf::from(program).file_name().unwrap().to_string_lossy().into_owned();
    StartedProcess {
        key: ProcessKey { pid, start: 100 },
        name: name.clone(),
        program: Some(program.into()),
        command: vec![program.to_owned()],
        user: "www-data".into(),
        uid: 33,
        ancestors: parent.into_iter().collect(),
        at: moment(Duration::ZERO),
        ..StartedProcess::default()
    }
}

fn guard() -> Guard<'static> {
    Guard { own: 777, keep_running: &[], freeze: true, reviewed: true, first_person: FIRST_PERSON }
}

fn learned() -> Baseline {
    Baseline::started(SystemTime::UNIX_EPOCH, Duration::ZERO)
}

fn rules_of(process: &StartedProcess, baseline: &Baseline) -> Vec<WatchRule> {
    examine(process, baseline, &History::default(), moment(Duration::ZERO))
        .into_iter()
        .map(|finding| finding.rule())
        .collect()
}

#[test]
fn a_service_that_starts_a_shell_is_a_warning_and_one_on_a_socket_is_an_alert() {
    let nginx = ancestor(10, "/usr/sbin/nginx", false);
    let shell = started(20, "/bin/sh", Some(nginx.clone()));
    let mut baseline = learned();
    baseline.note(&shell);
    assert_eq!(rules_of(&shell, &baseline), Vec::new(), "a start the machine makes anyway stays quiet");
    let fresh = learned();
    assert!(rules_of(&shell, &fresh).contains(&WatchRule::ShellFromService));
    let mut remote = shell.clone();
    remote.input = StreamKind::Socket;
    remote.output = StreamKind::Socket;
    let findings = examine(&remote, &baseline, &History::default(), moment(Duration::ZERO));
    let rules: Vec<WatchRule> = findings.iter().map(Finding::rule).collect();
    assert_eq!(rules, [WatchRule::ShellOnSocket], "the socket fires although the pair is known");
    assert_eq!(findings[0].level(), Level::Alert);
    assert!(findings[0].reason().contains("sh"), "{}", findings[0].reason());

    let mut errors_only = shell.clone();
    errors_only.input = StreamKind::Terminal;
    errors_only.output = StreamKind::Terminal;
    errors_only.errors = StreamKind::Socket;
    assert_eq!(rules_of(&errors_only, &baseline), [WatchRule::ShellOnSocket], "a socket on any stream counts");
}

#[test]
fn a_shell_nobody_typed_fires_while_one_in_a_terminal_stays_quiet() {
    let cron = ancestor(11, "/usr/sbin/cron", false);
    let quiet = started(21, "/bin/bash", Some(cron));
    assert!(rules_of(&quiet, &learned()).contains(&WatchRule::ShellWithoutTerminal));
    let typed_by_a_person = ancestor(12, "/usr/bin/tmux", true);
    let mut typed = started(22, "/bin/bash", Some(typed_by_a_person));
    typed.terminal = Some("pts/3".into());
    typed.input = StreamKind::Terminal;
    assert_eq!(rules_of(&typed, &learned()), [WatchRule::ParentNeverStartedIt]);
}

#[test]
fn four_different_commands_that_look_around_fire_once_as_a_run() {
    let server = ancestor(13, "/usr/bin/python3", false);
    let mut history = History::default();
    let baseline = learned();
    let mut fired = Vec::new();
    for (step, program) in
        ["/usr/bin/whoami", "/usr/bin/id", "/usr/bin/uname", "/bin/hostname", "/usr/bin/w"].into_iter().enumerate()
    {
        let at = moment(Duration::from_secs(step as u64));
        let mut process = started(30 + step as u32, program, Some(server.clone()));
        process.at = at;
        let findings = examine(&process, &baseline, &history, at);
        history.note(&process);
        fired.push(findings.iter().any(|finding| finding.rule() == WatchRule::DiscoveryRun));
    }
    assert_eq!(fired, [false, false, false, true, false], "the fourth makes the run, the fifth says nothing new");
}

#[test]
fn a_run_of_commands_spread_over_a_long_while_is_not_a_run() {
    let server = ancestor(13, "/usr/bin/python3", false);
    let mut history = History::default();
    let baseline = learned();
    for (step, program) in ["/usr/bin/whoami", "/usr/bin/id", "/usr/bin/uname", "/bin/hostname"].into_iter().enumerate()
    {
        let at = moment(Duration::from_secs(step as u64 * 60));
        let mut process = started(40 + step as u32, program, Some(server.clone()));
        process.at = at;
        let findings = examine(&process, &baseline, &history, at);
        history.note(&process);
        assert!(!findings.iter().any(|finding| finding.rule() == WatchRule::DiscoveryRun), "{program} at {step}");
    }
}

#[test]
fn a_program_from_a_temporary_folder_a_deleted_one_and_one_without_a_file_all_fire() {
    let parent = ancestor(14, "/usr/sbin/sshd", false);
    let mut baseline = learned();
    let temporary = started(50, "/tmp/.x/miner", Some(parent.clone()));
    baseline.note(&temporary);
    let findings = examine(&temporary, &baseline, &History::default(), moment(Duration::ZERO));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule(), WatchRule::ProgramInTempFolder);
    assert!(findings[0].reason().contains("/tmp"), "{}", findings[0].reason());

    let mut deleted = started(51, "/usr/bin/node", Some(parent.clone()));
    deleted.program_deleted = true;
    assert!(rules_of(&deleted, &baseline).contains(&WatchRule::ProgramDeleted));

    let mut nothing = started(52, "/usr/bin/node", Some(parent));
    nothing.program = None;
    assert!(rules_of(&nothing, &baseline).contains(&WatchRule::ProgramWithoutFile));
}

#[test]
fn a_preload_variable_fires_whatever_the_baseline_knows() {
    let parent = ancestor(15, "/usr/sbin/nginx", false);
    let mut process = started(60, "/usr/bin/curl", Some(parent));
    process.preloads = vec![("LD_PRELOAD".into(), "/tmp/libhide.so".into())];
    let mut baseline = learned();
    baseline.note(&process);
    let findings = examine(&process, &baseline, &History::default(), moment(Duration::ZERO));
    assert_eq!(findings.iter().map(Finding::rule).collect::<Vec<_>>(), [WatchRule::PreloadSet]);
    let reason = findings[0].reason();
    assert!(reason.contains("LD_PRELOAD") && reason.contains("/tmp/libhide.so"), "{reason}");
}

#[test]
fn a_name_that_does_not_match_its_program_fires_unless_the_program_runs_scripts() {
    let parent = ancestor(16, "/usr/bin/systemd", false);
    let mut masked = started(70, "/tmp/payload", Some(parent.clone()));
    masked.name = "kworker/2:1".into();
    assert!(rules_of(&masked, &learned()).contains(&WatchRule::NameDiffersFromProgram));

    let mut script = started(71, "/bin/sh", Some(parent.clone()));
    script.name = "backup.sh".into();
    assert!(
        !rules_of(&script, &learned()).contains(&WatchRule::NameDiffersFromProgram),
        "a shell is named after the script it runs"
    );

    let mut linked = started(72, "/usr/bin/mawk", Some(parent));
    linked.name = "awk".into();
    linked.link = Some("/usr/bin/awk".into());
    assert!(
        !rules_of(&linked, &learned()).contains(&WatchRule::NameDiffersFromProgram),
        "a program started through a link in a system folder carries the link's name"
    );
}

#[test]
fn a_copy_of_its_parent_has_started_no_program_of_its_own() {
    let mut parent = ancestor(17, "/usr/bin/bash", false);
    parent.command = "bash /srv/backup.sh".into();
    let mut subshell = started(73, "/usr/bin/bash", Some(parent.clone()));
    subshell.command = vec!["bash".into(), "/srv/backup.sh".into()];
    assert!(subshell.copy_of_parent(), "a subshell is the script's own shell, copied");
    let mut child = subshell.clone();
    child.command = vec!["bash".into(), "-c".into(), "id".into()];
    assert!(!child.copy_of_parent(), "a new command is a start of its own");
    let unreadable =
        StartedProcess { program: None, ancestors: vec![Ancestor { program: None, ..parent }], ..subshell };
    assert!(!unreadable.copy_of_parent(), "without a program to compare, nothing is a copy");
}

#[test]
fn the_baseline_learns_pairs_for_a_week_and_then_flags_what_is_new() {
    let since = moment(Duration::ZERO);
    let mut baseline = Baseline::started(since, WEEK);
    assert!(baseline.learning(since));
    assert!(baseline.learning(since + WEEK - Duration::from_secs(1)));
    assert!(!baseline.learning(since + WEEK));
    assert_eq!(baseline.remaining(since + WEEK / 2), WEEK / 2);
    assert_eq!(baseline.remaining(since + WEEK * 2), Duration::ZERO);

    let parent = ancestor(17, "/usr/sbin/cron", false);
    let known = started(80, "/usr/bin/rsync", Some(parent.clone()));
    let fresh = started(81, "/usr/bin/nc", Some(parent));
    baseline.note(&known);
    assert!(baseline.knows(&known) && !baseline.knows(&fresh));
    assert!(rules_of(&fresh, &baseline).is_empty(), "nothing is new while Borg is still learning");
    let after = Baseline { since: SystemTime::UNIX_EPOCH, period: Duration::ZERO, ..baseline.clone() };
    assert!(rules_of(&fresh, &after).contains(&WatchRule::ParentNeverStartedIt));
}

#[test]
fn a_process_without_a_parent_is_not_measured_against_the_baseline() {
    let orphan = started(82, "/usr/bin/rsync", None);
    assert_eq!(Baseline::pair(&orphan), None);
    let mut baseline = learned();
    baseline.note(&orphan);
    assert!(baseline.pairs.is_empty());
    assert!(!rules_of(&orphan, &baseline).contains(&WatchRule::ParentNeverStartedIt));
}

#[test]
fn an_allowance_matches_only_what_every_field_it_names_matches() {
    let parent = ancestor(18, "/usr/sbin/nginx", false);
    let process = started(90, "/bin/sh", Some(parent));
    let covers = |allowance: Allowance| allowance.covers(&process);
    assert!(covers(Allowance { program: Some("/bin/sh".into()), ..Allowance::default() }));
    assert!(covers(Allowance { program: Some("sh".into()), parent: Some("nginx".into()), ..Allowance::default() }));
    assert!(covers(Allowance { user: Some("www-data".into()), ..Allowance::default() }));
    assert!(!covers(Allowance { program: Some("/bin/sh".into()), user: Some("root".into()), ..Allowance::default() }));
    assert!(!covers(Allowance { parent: Some("apache2".into()), ..Allowance::default() }));
    assert!(!covers(Allowance::default()), "an allowance that names nothing allows nothing");
}

#[test]
fn the_baseline_stops_growing_at_its_limit() {
    let mut baseline = Baseline::started(moment(Duration::ZERO), WEEK);
    for index in 0..PAIRS_LIMIT + 10 {
        let parent = ancestor(1000 + index as u32, &format!("/usr/bin/p{index}"), false);
        baseline.note(&started(2000 + index as u32, &format!("/usr/bin/c{index}"), Some(parent)));
    }
    assert_eq!(baseline.pairs.len(), PAIRS_LIMIT);
}

#[test]
fn nothing_is_frozen_while_borg_learns_but_the_log_says_what_it_would_do() {
    let since = moment(Duration::ZERO);
    let baseline = Baseline::started(since, WEEK);
    let parent = ancestor(19, "/usr/sbin/nginx", false);
    let mut shell = started(100, "/bin/sh", Some(parent));
    shell.input = StreamKind::Socket;
    let finding = Finding::ShellOnSocket { shell: "sh".into() };
    assert_eq!(
        decide(&finding, &shell, &baseline, &guard(), since),
        Response::Learning { would_freeze: true },
        "it would freeze once the learning period is over"
    );
    assert_eq!(decide(&finding, &shell, &baseline, &guard(), since + WEEK), Response::Frozen);

    let waiting = Guard { reviewed: false, ..guard() };
    assert_eq!(
        decide(&finding, &shell, &baseline, &waiting, since),
        Response::Learning { would_freeze: true },
        "what the week would freeze is written down whether or not the user has looked"
    );
    assert_eq!(
        decide(&finding, &shell, &baseline, &waiting, since + WEEK),
        Response::Spared(Protection::Unreviewed),
        "freezing waits until the user has seen what the week would have frozen"
    );
    assert_eq!(
        decide(&finding, &shell, &baseline, &waiting, baseline.review_ends()),
        Response::Frozen,
        "a machine nobody looks at is protected once the days for a review are over"
    );
}

#[test]
fn the_week_is_reviewed_once_every_alert_that_would_have_frozen_is_read() {
    let mut log = AlertLog::default();
    assert!(log.reviewed(), "a week that would have frozen nothing needs no review");
    let finding = Finding::ShellOnSocket { shell: "sh".into() };
    let parent = ancestor(40, "/usr/sbin/nginx", false);
    let quiet = Response::Learning { would_freeze: false };
    log.add(moment(Duration::ZERO), finding.clone(), quiet, started(41, "/bin/sh", Some(parent.clone())));
    assert!(log.reviewed(), "an alert Borg would not have frozen for waits on nobody");
    let would = Response::Learning { would_freeze: true };
    log.add(moment(Duration::ZERO), finding, would, started(42, "/bin/sh", Some(parent)));
    assert!(!log.reviewed());
    log.mark_seen();
    assert!(log.reviewed(), "opening the page marks the week as seen");
}

#[test]
fn the_guardrails_keep_the_machine_and_its_people_running() {
    let now = moment(Duration::ZERO);
    let baseline = learned();
    let parent = ancestor(20, "/usr/sbin/nginx", false);
    let finding = Finding::ShellOnSocket { shell: "sh".into() };
    let spared = |process: &StartedProcess, guard: &Guard<'_>| match decide(&finding, process, &baseline, guard, now) {
        Response::Spared(protection) => Some(protection),
        _ => None,
    };
    let first = StartedProcess { key: ProcessKey { pid: 1, start: 1 }, ..started(1, "/sbin/init", None) };
    assert_eq!(spared(&first, &guard()), Some(Protection::FirstProcess));

    let mut own = started(777, "/usr/local/bin/borg", None);
    own.name = "borg".into();
    assert_eq!(spared(&own, &guard()), Some(Protection::Borg));
    let child_of_borg = started(778, "/bin/sh", Some(ancestor(777, "/usr/local/bin/borg", false)));
    assert_eq!(spared(&child_of_borg, &guard()), Some(Protection::Borg));

    let sshd = started(101, "/usr/sbin/sshd", Some(parent.clone()));
    assert_eq!(spared(&sshd, &guard()), Some(Protection::NeverFrozen("sshd".into())));
    let copy = started(105, "/tmp/sshd", Some(parent.clone()));
    assert_eq!(spared(&copy, &guard()), None, "a copy named after a spared program elsewhere is frozen");
    let unreadable = StartedProcess { program: None, name: "sshd".into(), ..sshd.clone() };
    assert_eq!(spared(&unreadable, &guard()), Some(Protection::NeverFrozen("sshd".into())));

    let kept = ["backup-agent".to_owned()];
    let agent = started(102, "/usr/local/bin/backup-agent", Some(parent.clone()));
    assert_eq!(
        spared(&agent, &Guard { keep_running: &kept, ..guard() }),
        Some(Protection::NeverFrozen("backup-agent".into()))
    );

    let mut at_a_terminal = started(103, "/bin/sh", Some(parent.clone()));
    at_a_terminal.terminal = Some("pts/1".into());
    at_a_terminal.session_parent = Some(ancestor(30, "/usr/sbin/sshd", false));
    assert_eq!(spared(&at_a_terminal, &guard()), Some(Protection::Terminal));
    let in_tmux = StartedProcess {
        session_parent: Some(ancestor(31, "/usr/bin/tmux", false)),
        uid: 1000,
        ..at_a_terminal.clone()
    };
    assert_eq!(spared(&in_tmux, &guard()), Some(Protection::Terminal));
    let root_in_tmux = StartedProcess { uid: 0, ..in_tmux.clone() };
    assert_eq!(spared(&root_in_tmux, &guard()), Some(Protection::Terminal));
    let service_in_tmux = StartedProcess { uid: 33, ..in_tmux.clone() };
    assert_eq!(spared(&service_in_tmux, &guard()), None, "a service account's multiplexer is nobody's login");
    let older_numbering = Guard { first_person: 500, ..guard() };
    let person_from_500 = StartedProcess { uid: 500, ..in_tmux.clone() };
    assert_eq!(spared(&person_from_500, &guard()), None);
    assert_eq!(
        spared(&person_from_500, &older_numbering),
        Some(Protection::Terminal),
        "a system that numbers people from 500 says so, and Borg follows it"
    );
    for opener in [None, Some(ancestor(32, "/usr/bin/python3", false)), Some(ancestor(33, "/tmp/sshd", false))] {
        let opened_by_a_program = StartedProcess { session_parent: opener.clone(), ..at_a_terminal.clone() };
        assert_eq!(spared(&opened_by_a_program, &guard()), None, "nobody logged in to the terminal {opener:?} opened");
    }

    let shell = started(104, "/bin/sh", Some(parent.clone()));
    assert_eq!(spared(&shell, &Guard { freeze: false, ..guard() }), Some(Protection::FreezeOff));
    assert_eq!(spared(&shell, &guard()), None, "a shell on a socket from a service is frozen");

    let notice = Finding::ParentNeverStartedIt { parent: "nginx".into(), program: "sh".into() };
    assert_eq!(
        decide(&notice, &shell, &baseline, &guard(), now),
        Response::Spared(Protection::OnlyANotice),
        "a notice never stops a program"
    );

    let mut allowed = learned();
    allowed.allow(Allowance { program: Some("/bin/sh".into()), ..Allowance::default() });
    assert_eq!(decide(&finding, &shell, &allowed, &guard(), now), Response::Spared(Protection::Allowed));
}

/// A shell on a socket, from a parent the baseline knows, so only that one rule fires.
fn remote_shell(pid: u32) -> StartedProcess {
    let mut shell = started(pid, "/bin/sh", Some(ancestor(21, "/usr/sbin/nginx", false)));
    shell.input = StreamKind::Socket;
    shell
}

#[test]
fn the_watcher_freezes_a_few_a_minute_and_leaves_the_rest_running() {
    let now = moment(Duration::ZERO);
    let mut watcher = Watcher::new(learned(), Limits::default());
    let mut frozen = 0;
    let mut spared = 0;
    for pid in 0..FREEZES_PER_MINUTE + 3 {
        let outcomes = watcher.examine(&remote_shell(200 + pid as u32), &guard(), now);
        assert!(!outcomes.is_empty());
        match &outcomes[0].response {
            Response::Frozen => frozen += 1,
            Response::Spared(Protection::EnoughForNow) => spared += 1,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!((frozen, spared), (FREEZES_PER_MINUTE, 3));
    let later = now + Duration::from_secs(61);
    assert_eq!(watcher.examine(&remote_shell(300), &guard(), later)[0].response, Response::Frozen);
}

#[test]
fn too_many_findings_at_once_are_counted_instead_of_written_down() {
    let now = moment(Duration::ZERO);
    let mut watcher = Watcher::new(learned(), Limits { alerts_per_minute: 2, freezes_per_minute: 10 });
    let alert_only = Guard { freeze: false, ..guard() };
    let written = watcher.examine(&remote_shell(400), &alert_only, now).len();
    assert!(written > 1, "a shell on a socket from a service breaks several rules at once");
    assert_eq!(watcher.examine(&remote_shell(401), &alert_only, now).len(), written);
    assert!(watcher.examine(&remote_shell(402), &alert_only, now).is_empty(), "the third is only counted");
    assert_eq!(watcher.suppressed(), written as u64);
    assert_eq!(watcher.seen(), 3);
    let later = now + Duration::from_secs(61);
    assert_eq!(watcher.examine(&remote_shell(403), &alert_only, later).len(), written);
}

#[test]
fn a_flood_of_alerts_never_keeps_a_process_from_being_frozen() {
    let now = moment(Duration::ZERO);
    let mut watcher = Watcher::new(learned(), Limits { alerts_per_minute: 2, freezes_per_minute: 3 });
    let alert_only = Guard { freeze: false, ..guard() };
    for pid in 0..5 {
        watcher.examine(&remote_shell(700 + pid), &alert_only, now);
    }
    let outcomes = watcher.examine(&remote_shell(710), &guard(), now);
    assert!(!outcomes.is_empty(), "the alert about a frozen process is written however many came before");
    assert_eq!(outcomes[0].response, Response::Frozen);
    for pid in 0..2 {
        assert_eq!(watcher.examine(&remote_shell(720 + pid), &guard(), now)[0].response, Response::Frozen);
    }
    for pid in 0..2 {
        assert_eq!(
            watcher.examine(&remote_shell(730 + pid), &guard(), now)[0].response,
            Response::Spared(Protection::EnoughForNow),
            "a process Borg would have frozen past the freeze limit is still written down"
        );
    }
    assert!(
        watcher.examine(&remote_shell(740), &guard(), now).is_empty(),
        "past every limit a finding is only counted"
    );
}

#[test]
fn a_run_of_notices_leaves_room_for_warnings_and_alerts() {
    let now = moment(Duration::ZERO);
    let mut watcher = Watcher::new(learned(), Limits { alerts_per_minute: 2, freezes_per_minute: 10 });
    let alert_only = Guard { freeze: false, ..guard() };
    let parent = ancestor(23, "/usr/bin/make", false);
    let new_pairs: Vec<usize> = (0..5)
        .map(|index| {
            watcher
                .examine(
                    &started(800 + index, &format!("/usr/bin/tool{index}"), Some(parent.clone())),
                    &alert_only,
                    now,
                )
                .len()
        })
        .collect();
    assert_eq!(new_pairs, [1, 1, 0, 0, 0], "notices have a limit of their own");
    assert!(!watcher.examine(&remote_shell(810), &alert_only, now).is_empty(), "the notices took none of its room");
}

#[test]
fn the_watcher_learns_while_it_watches_so_a_second_start_is_no_longer_new() {
    let since = moment(Duration::ZERO);
    let mut watcher = Watcher::new(Baseline::started(since, WEEK), Limits::default());
    let parent = ancestor(22, "/usr/sbin/cron", false);
    let backup = started(500, "/usr/bin/rsync", Some(parent));
    assert!(watcher.examine(&backup, &guard(), since).is_empty());
    assert!(watcher.baseline.knows(&backup), "what the machine does becomes what it may do");

    let dropped = started(501, "/tmp/backup", Some(ancestor(22, "/usr/sbin/cron", false)));
    assert!(!watcher.examine(&dropped, &guard(), since).is_empty());
    assert!(!watcher.baseline.knows(&dropped), "a start a rule always fires on is never learned");
    let from_service = started(502, "/bin/sh", Some(ancestor(23, "/usr/sbin/postgres", false)));
    watcher.examine(&from_service, &guard(), since);
    assert!(watcher.baseline.knows(&from_service), "a start only learnable rules fired on is learned");
}

#[test]
fn every_rule_that_fires_on_one_process_says_the_same_about_what_borg_did() {
    let now = moment(Duration::ZERO);
    let mut watcher = Watcher::new(learned(), Limits::default());
    let mut shell = remote_shell(600);
    shell.program = Some("/tmp/sh".into());
    let outcomes = watcher.examine(&shell, &guard(), now);
    assert!(outcomes.len() > 1, "{outcomes:?}");
    assert!(outcomes.iter().all(|outcome| outcome.response == Response::Frozen));
    assert_eq!(outcomes[0].finding.level(), Level::Alert, "the worst finding comes first");
}

fn log_with(alerts: usize) -> AlertLog {
    let mut log = AlertLog::watching(WatchStatus { source: WatchSource::Kernel, ..WatchStatus::default() });
    for index in 0..alerts {
        let mut shell = remote_shell(700 + index as u32);
        shell.user = if index % 2 == 0 { "root".into() } else { "www-data".into() };
        let finding = Finding::ShellOnSocket { shell: "sh".into() };
        log.add(moment(Duration::from_secs(index as u64)), finding, Response::Frozen, shell);
    }
    log
}

#[test]
fn an_alert_from_earlier_lands_below_the_ones_that_came_after_it() {
    let mut log = log_with(2);
    let newest = log.alerts[0].at;
    let earlier = Finding::ShellOnSocket { shell: "sh".into() };
    let id = log.add(newest - Duration::from_secs(600), earlier, Response::Alerted, remote_shell(999));
    assert_eq!(log.alerts.last().map(|alert| alert.id), Some(id), "the log stays newest first");
    let later = Finding::ShellOnSocket { shell: "sh".into() };
    let id = log.add(newest + Duration::from_secs(1), later, Response::Alerted, remote_shell(1000));
    assert_eq!(log.alerts.first().map(|alert| alert.id), Some(id));
}

#[test]
fn the_log_keeps_the_newest_alerts_under_their_own_ids() {
    let log = log_with(ALERTS_KEPT + 5);
    assert_eq!(log.alerts.len(), ALERTS_KEPT);
    assert_eq!(log.alerts[0].id, (ALERTS_KEPT + 5) as u64, "the newest alert comes first");
    assert_eq!(log.unread(), ALERTS_KEPT);
    let mut read = log.clone();
    read.mark_seen();
    assert_eq!(read.unread(), 0);
    assert!(read.alerts[0].frozen() && read.alerts[0].resumable());
}

#[test]
fn the_filter_keeps_the_alerts_that_hold_every_word_and_drops_what_follows_a_bang() {
    let log = log_with(4);
    assert_eq!(alerts_filtered(&log.alerts, "").len(), 4);
    assert_eq!(alerts_filtered(&log.alerts, "root").len(), 2);
    assert_eq!(alerts_filtered(&log.alerts, "shell-on-socket !root").len(), 2);
    assert_eq!(alerts_filtered(&log.alerts, "nginx").len(), 4);
    assert_eq!(alerts_filtered(&log.alerts, "apache").len(), 0);
}

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        120,
        32,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.settings.preferences.git_plugin = false;
    (state, now)
}

fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, modifiers)), state.epoch)
}

fn asked(effects: &[Effect]) -> Option<(u64, AlertsRequest)> {
    effects.iter().find_map(|effect| match effect {
        Effect::Alerts { id, request } => Some((*id, request.clone())),
        _ => None,
    })
}

fn opened() -> (AppState, Instant) {
    let (mut state, now) = state();
    let (id, request) = asked(&press(&mut state, KeyCode::Char('l'), KeyModifiers::ALT)).unwrap();
    assert_eq!(request, AlertsRequest::Open, "opening the page reads the log and marks it read");
    assert!(state.alerts.is_open() && state.monitor_open());
    let reply = AlertsReply { log: log_with(3), notice: None };
    update(&mut state, Msg::Alerts { id, result: Ok(reply) }, now);
    (state, now)
}

#[test]
fn alt_l_opens_the_alerts_page_reads_the_log_and_closes_it_again() {
    let (mut state, now) = state();
    let (first, _) = asked(&press(&mut state, KeyCode::Char('l'), KeyModifiers::ALT)).unwrap();
    assert!(!state.alerts.loaded);
    let reply = || AlertsReply { log: log_with(3), notice: None };
    update(&mut state, Msg::Alerts { id: first + 1, result: Ok(reply()) }, now);
    assert!(!state.alerts.loaded, "an answer to another request is dropped");
    update(&mut state, Msg::Alerts { id: first, result: Ok(reply()) }, now);
    assert_eq!(state.alerts.len(), 3);
    assert_eq!(state.alerts.selected().map(|alert| alert.id), Some(3));
    press(&mut state, KeyCode::Char('l'), KeyModifiers::ALT);
    assert!(!state.alerts.is_open());
}

#[test]
fn opening_another_monitor_closes_the_alerts_page() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT);
    assert!(state.startup_items.is_open() && !state.alerts.is_open());
    press(&mut state, KeyCode::Char('l'), KeyModifiers::ALT);
    assert!(state.alerts.is_open() && !state.startup_items.is_open());
    press(&mut state, KeyCode::Char('m'), KeyModifiers::ALT);
    assert!(state.activity.side.is_some() && !state.alerts.is_open());
}

#[test]
fn the_cursor_stays_on_the_same_alert_while_the_filter_narrows_the_list() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    let chosen = state.alerts.selected().unwrap().id;
    press(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);
    update(&mut state, Msg::Paste("www-data".into()), now);
    assert_eq!(state.alerts.len(), 1);
    assert_eq!(state.alerts.selected().map(|alert| alert.id), Some(chosen));
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(state.alerts.len(), 3, "Esc clears the filter");
    assert!(state.alerts.is_open());
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.alerts.is_open(), "Esc then closes the page");
}

#[test]
fn enter_shows_the_alerts_process_in_the_activity_monitor() {
    let (mut state, _) = opened();
    let pid = state.alerts.selected().unwrap().process.key.pid;
    press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(state.activity.side.is_some() && !state.alerts.is_open());
    assert!(state.activity.reveals(pid), "the monitor selects it once the next sample lists it");
}

#[test]
fn c_resumes_a_frozen_process_and_says_so_when_there_is_nothing_to_resume() {
    let (mut state, now) = opened();
    let alert = state.alerts.selected().unwrap().id;
    let process = state.alerts.selected().unwrap().process.key;
    let (_, request) = asked(&press(&mut state, KeyCode::Char('c'), KeyModifiers::NONE)).unwrap();
    assert_eq!(request, AlertsRequest::Resume { alert, process });

    let mut log = log_with(1);
    log.alerts[0].state = AlertState::Running;
    let reply = AlertsReply { log, notice: Some("done".into()) };
    let id = state.alerts.pending.unwrap();
    update(&mut state, Msg::Alerts { id, result: Ok(reply) }, now);
    assert_eq!(state.notice.as_deref(), Some("done"));
    assert!(asked(&press(&mut state, KeyCode::Char('c'), KeyModifiers::NONE)).is_none());
    assert!(state.notice.is_some());
}

#[test]
fn k_asks_before_killing_and_only_then_sends_the_signal() {
    let (mut state, now) = opened();
    let alert = state.alerts.selected().unwrap().id;
    let process = state.alerts.selected().unwrap().process.key;
    assert!(asked(&press(&mut state, KeyCode::Char('k'), KeyModifiers::NONE)).is_none(), "it asks first");
    assert!(matches!(
        &state.overlay,
        Some(Overlay::Confirm(confirm)) if confirm.purpose == ConfirmPurpose::AlertKill { alert, process } && !confirm.accept
    ));
    let effects = update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)), now);
    assert_eq!(asked(&effects).map(|(_, request)| request), Some(AlertsRequest::Kill { alert, process }));
}

#[test]
fn a_allows_starts_like_the_selected_one() {
    let (mut state, _) = opened();
    let alert = state.alerts.selected().unwrap();
    let (id, allowance) = (alert.id, alert.allowance());
    assert_eq!(allowance.program.as_deref(), Some("/bin/sh"));
    assert_eq!(allowance.parent.as_deref(), Some("nginx"));
    let (_, request) = asked(&press(&mut state, KeyCode::Char('a'), KeyModifiers::NONE)).unwrap();
    assert_eq!(request, AlertsRequest::Allow { alert: id, allowance });
}

#[test]
fn r_reads_the_log_again_and_ctrl_c_copies_the_command() {
    let (mut state, _) = opened();
    let (_, request) = asked(&press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE)).unwrap();
    assert_eq!(request, AlertsRequest::Read, "reading again leaves unread alerts unread");
    let effects = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(effects, [Effect::Copy("/bin/sh".into())]);
}

#[test]
fn the_count_of_unread_alerts_follows_the_log_even_while_the_page_is_closed() {
    let (mut state, now) = state();
    update(&mut state, Msg::AlertsChanged(Box::new(log_with(2))), now);
    assert_eq!(state.alerts.unread, 2);
    assert!(!state.alerts.is_open());
    let mut read = log_with(2);
    read.mark_seen();
    update(&mut state, Msg::AlertsChanged(Box::new(read)), now);
    assert_eq!(state.alerts.unread, 0);
}

#[test]
fn the_page_says_what_went_wrong_instead_of_a_list() {
    let (mut state, now) = state();
    let (id, _) = asked(&press(&mut state, KeyCode::Char('l'), KeyModifiers::ALT)).unwrap();
    update(&mut state, Msg::Alerts { id, result: Err("no state folder".into()) }, now);
    assert_eq!(state.alerts.error.as_deref(), Some("no state folder"));
    assert!(state.alerts.is_empty());
}
