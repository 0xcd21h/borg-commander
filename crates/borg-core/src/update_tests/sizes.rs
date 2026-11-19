use super::*;
use crate::Reveal;

fn f3(state: &mut AppState, now: Instant) -> Vec<Effect> {
    press(state, now, key(KeyCode::F(3)))
}

fn hover(state: &mut AppState, name: &str) {
    let rows = state.panel(Side::Left).len() as isize;
    state.panel_mut(Side::Left).move_by(-rows);
    while hovered(state) != Some(name) {
        state.panel_mut(Side::Left).move_by(1);
    }
}

fn measuring(effects: &[Effect]) -> Vec<&Path> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::MeasureSizes { root, .. } => Some(root.as_path()),
            _ => None,
        })
        .collect()
}

fn size_jobs(state: &AppState) -> Vec<&crate::Job> {
    state.jobs.iter().filter(|job| job.kind == JobKind::Sizes).collect()
}

fn measured(path: &str, size: u64) -> Entry {
    let mut entry = dir(Path::new(path).file_name().unwrap().to_str().unwrap());
    entry.source = Some(path.into());
    entry.size = size;
    entry.usage = Some(borg_vfs::FolderUsage { files: 3, ..Default::default() });
    entry
}

fn progress(state: &mut AppState, now: Instant, visited: usize) {
    let id = state.sizing.as_ref().unwrap().id;
    update(state, Msg::SizeProgress { id, progress: crate::SearchProgress { visited, ..Default::default() } }, now);
}

fn finish(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let id = state.sizing.as_ref().unwrap().id;
    update(state, Msg::SizesFinished { id, error: None }, now)
}

#[test]
fn f3_on_a_folder_counts_it_in_the_background_and_its_row_shows_the_size() {
    let (mut state, now) = setup_with(vec![dir("me"), dir("other"), file("notes.txt")]);
    hover(&mut state, "me");
    assert_eq!(measuring(&f3(&mut state, now)), [Path::new("/home/me")]);
    let [job] = size_jobs(&state)[..] else { panic!("one size job") };
    assert!(job.running());
    assert_eq!(job.subject, "/home/me");
    assert_eq!(job.reveal, Some(Reveal::Folder { side: Side::Left, dir: "/home".into(), focus: Some("me".into()) }));
    assert!(state.viewer.is_none() && state.opening.is_none(), "nothing opens in the viewer");

    let id = state.sizing.as_ref().unwrap().id;
    update(&mut state, Msg::Sized { id, entries: vec![measured("/home/me", 42_000)] }, now);
    let size = |state: &AppState| state.active_panel().entries().iter().find(|e| e.name == "me").unwrap().size;
    assert_eq!(size(&state), 42_000);
    progress(&mut state, now, 80);
    let effects = finish(&mut state, now);
    assert!(state.sizing.is_none());
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::List { side: Side::Left, dir, .. } if dir == Path::new("/home"))),
        "the folder holding it lists again from the saved scan: {effects:?}"
    );
    assert!(ended_with(&state, JobKind::Sizes).is_some_and(|summary| summary.contains("80 scanned")));
}

#[test]
fn f3_on_the_parent_row_counts_the_folder_the_panel_shows() {
    let (mut state, now) = setup_with(vec![dir("me"), file("notes.txt")]);
    hover(&mut state, "me");
    state.panel_mut(Side::Left).move_by(-1);
    assert!(matches!(state.active_panel().hovered(), Some(crate::Row::Parent)));
    assert_eq!(measuring(&f3(&mut state, now)), [Path::new("/home")]);
    let [job] = size_jobs(&state)[..] else { panic!("one size job") };
    assert_eq!(job.subject, "/home");
    assert_eq!(job.reveal, Some(Reveal::Folder { side: Side::Left, dir: "/home".into(), focus: None }));
}

#[test]
fn f3_counts_the_marked_folders_one_after_another_in_one_job_and_views_a_file_under_the_cursor() {
    let (mut state, now) = setup_with(vec![dir("a"), dir("b"), dir("c"), file("notes.txt")]);
    for name in ["a", "c", "notes.txt"] {
        hover(&mut state, name);
        press(&mut state, now, key(KeyCode::Insert));
    }
    hover(&mut state, "notes.txt");
    assert!(
        matches!(&f3(&mut state, now)[..], [Effect::View { path, .. }] if path == Path::new("/home/notes.txt")),
        "a file under the cursor still opens in the viewer"
    );
    assert!(size_jobs(&state).is_empty());

    hover(&mut state, "b");
    assert_eq!(measuring(&f3(&mut state, now)), [Path::new("/home/a")], "marked folders count, not the one hovered");
    let [job] = size_jobs(&state)[..] else { panic!("one size job") };
    let job = job.id;
    assert_eq!(size_jobs(&state)[0].subject, "2 folders in /home");
    progress(&mut state, now, 5);
    let effects = finish(&mut state, now);
    assert_eq!(measuring(&effects), [Path::new("/home/c")]);
    assert_eq!(state.sizing.as_ref().map(|scan| scan.job), Some(job), "the same job measures the next folder");
    assert_eq!(state.sizing.as_ref().unwrap().visited(), 5);
    assert!(runs(&state, JobKind::Sizes));

    progress(&mut state, now, 7);
    assert_eq!(state.sizing.as_ref().unwrap().visited(), 12);
    let effects = finish(&mut state, now);
    assert!(measuring(&effects).is_empty());
    assert!(ended_with(&state, JobKind::Sizes).is_some_and(|summary| summary.contains("12 scanned")));
}

#[test]
fn a_second_f3_on_a_folder_being_counted_starts_no_second_count() {
    let (mut state, now) = setup_with(vec![dir("me"), dir("other")]);
    hover(&mut state, "me");
    f3(&mut state, now);
    assert!(f3(&mut state, now).is_empty());
    assert_eq!(size_jobs(&state).len(), 1);
    assert_eq!(state.notice.as_deref(), Some("Already counting sizes in /home/me."));

    press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    assert!(!runs(&state, JobKind::Sizes), "Alt-I stops the count");
    press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    hover(&mut state, "other");
    assert!(f3(&mut state, now).is_empty(), "the count of /home measures its folders too");
    assert_eq!(state.notice.as_deref(), Some("Already counting sizes in /home."));
    assert_eq!(size_jobs(&state).iter().filter(|job| job.running()).count(), 1);
}

#[test]
fn f3_on_another_folder_waits_for_the_count_under_way() {
    let (mut state, now) = setup_with(vec![dir("a"), dir("b"), dir("c")]);
    hover(&mut state, "a");
    f3(&mut state, now);
    hover(&mut state, "b");
    assert!(f3(&mut state, now).is_empty(), "the second count waits");
    hover(&mut state, "c");
    f3(&mut state, now);
    let jobs: Vec<u64> = size_jobs(&state).iter().map(|job| job.id).collect();
    assert_eq!(jobs.len(), 3);
    assert!(size_jobs(&state).iter().all(|job| job.running()));
    assert_eq!(size_jobs(&state)[1].phase, crate::Phase::Preparing);
    assert_eq!(state.sizing.as_ref().unwrap().root, Path::new("/home/a"));

    assert_eq!(measuring(&finish(&mut state, now)), [Path::new("/home/b")]);
    assert_eq!(state.sizing.as_ref().map(|scan| scan.job), Some(jobs[1]));

    let effects = crate::jobs::cancel(&mut state, jobs[1], now);
    assert!(
        matches!(&effects[..], [Effect::CancelMeasure, Effect::MeasureSizes { root, .. }] if root == Path::new("/home/c"))
    );
    assert_eq!(state.sizing.as_ref().map(|scan| scan.job), Some(jobs[2]));
}

#[test]
fn stopping_size_counts_stops_the_waiting_ones_too() {
    let (mut state, now) = setup_with(vec![dir("a"), dir("b"), dir("c")]);
    for name in ["a", "b", "c"] {
        hover(&mut state, name);
        f3(&mut state, now);
    }
    let waiting = size_jobs(&state)[2].id;
    assert!(crate::jobs::cancel(&mut state, waiting, now).is_empty(), "a waiting count just goes");
    assert!(!size_jobs(&state)[2].running());
    assert_eq!(size_jobs(&state).iter().filter(|job| job.running()).count(), 2);

    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    assert_eq!(effects, [Effect::CancelMeasure]);
    assert!(state.sizing.is_none());
    assert!(size_jobs(&state).iter().all(|job| job.ending().is_some_and(|ending| ending.how == How::Cancelled)));
}

#[test]
fn a_folder_that_cannot_be_counted_fails_its_job_and_the_next_count_starts() {
    let (mut state, now) = setup_with(vec![dir("a"), dir("b")]);
    hover(&mut state, "a");
    f3(&mut state, now);
    hover(&mut state, "b");
    f3(&mut state, now);
    let id = state.sizing.as_ref().unwrap().id;
    let effects = update(&mut state, Msg::SizesFinished { id, error: Some("Permission denied".into()) }, now);
    assert_eq!(measuring(&effects), [Path::new("/home/b")]);
    assert_eq!(size_jobs(&state)[0].ending().map(|ending| ending.how), Some(How::Failed));
}

#[test]
fn f3_leaves_links_to_folders_alone() {
    let link = entry("linked", EntryKind::Symlink { to_dir: true });
    let (mut state, now) = setup_with(vec![dir("me"), link]);
    hover(&mut state, "linked");
    assert!(f3(&mut state, now).is_empty());
    assert!(size_jobs(&state).is_empty());
    assert!(crate::folders_to_count(state.active_panel()).is_empty());
}

#[test]
fn the_file_menu_and_the_context_menu_name_what_f3_does_under_the_cursor() {
    let (mut state, now) = setup_with(vec![dir("me"), file("notes.txt")]);
    let first = |state: &AppState| menus(state)[1].item(0).map(|item| (item.label, item.hint.clone(), item.enabled));
    hover(&mut state, "me");
    assert_eq!(first(&state), Some(("Count size", "F3".into(), true)));
    press(&mut state, now, shift(KeyCode::F(10)));
    assert!(context_labels(&state).contains(&"Count size"));
    press(&mut state, now, key(KeyCode::Esc));

    hover(&mut state, "notes.txt");
    assert_eq!(first(&state), Some(("View", "F3".into(), true)));
    press(&mut state, now, shift(KeyCode::F(10)));
    assert!(context_labels(&state).contains(&"View"));
}
