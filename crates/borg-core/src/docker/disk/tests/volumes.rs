use super::*;
use crate::VolumeMount;
use std::path::{Path, PathBuf};

fn time(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn changed(newest: &str) -> VolumeContents {
    VolumeContents { newest: Some(time(newest)), top: vec!["2026".into(), "index.json".into()] }
}

#[test]
fn the_last_use_is_the_latest_of_a_container_s_stop_and_a_change_to_the_files() {
    let mut disk = disk(None);
    let mut uploads = volume("blog_uploads", Some(860 * MB), &[], false);
    assert_eq!(disk.last_used(&uploads), LastUsed::Unknown);
    disk.contents.insert("blog_uploads".into(), changed("2026-10-02T09:00:00Z"));
    assert_eq!(disk.last_used(&uploads), LastUsed::Changed(time("2026-10-02T09:00:00Z")), "the files tell alone");
    uploads.users = vec![user("blog-web-1", State::Exited, Some("2026-10-20T18:00:00Z"))];
    assert_eq!(disk.last_used(&uploads), LastUsed::Stopped(time("2026-10-20T18:00:00Z")), "a later stop is exact");
    disk.contents.insert("blog_uploads".into(), changed("2026-11-01T08:00:00Z"));
    assert_eq!(
        disk.last_used(&uploads),
        LastUsed::Changed(time("2026-11-01T08:00:00Z")),
        "a change after the stop came from a container removed since"
    );
    uploads.users.push(user("blog-web-2", State::Running, None));
    assert_eq!(disk.last_used(&uploads), LastUsed::Now);
}

/// Disk use open on the shop engine, with the files of the volumes in `folders` readable here.
fn measured_with(folders: &[(&str, &str)]) -> (AppState, Vec<Effect>) {
    let mut state = state();
    let now = state.epoch;
    let effects = crate::actions::perform(&mut state, Action::DockerDisk, now);
    let [id] = measures(&effects)[..] else { panic!("opening measures once: {effects:?}") };
    let folders = folders.iter().map(|&(name, folder)| (name.to_owned(), PathBuf::from(folder))).collect();
    let data = DockerData::DiskUse { usage: Box::new(usage()), context: None, contexts: None, folders };
    let effects = update(&mut state, Msg::Docker { id, result: Ok(data) }, now);
    (state, effects)
}

/// The folders `effects` list in a panel.
fn listed(effects: &[Effect]) -> Vec<&Path> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::List { dir, .. } => Some(dir.as_path()),
            _ => None,
        })
        .collect()
}

/// Finishes the listings `effects` start, empty.
fn arrive(state: &mut AppState, effects: &[Effect]) {
    let now = state.epoch;
    for effect in effects {
        if let Effect::List { side, generation, .. } = *effect {
            update(state, Msg::Batch { side, generation, entries: Vec::new() }, now);
            update(state, Msg::Listed { side, generation, error: None }, now);
        }
    }
}

/// The volumes `effects` ask to read, in order.
fn reads(effects: &[Effect]) -> Vec<(u64, Vec<String>)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Docker { id, request: crate::DockerRequest::ReadVolumes(volumes) } => {
                Some((*id, volumes.iter().map(|(name, _)| name.clone()).collect()))
            }
            _ => None,
        })
        .collect()
}

fn select_volume(state: &mut AppState, kind: DiskKind, name: &str) {
    let disk = &mut state.docker.disk;
    let index = disk.usage.as_ref().unwrap().volumes.iter().position(|volume| volume.name == name).unwrap();
    disk.unfolded.insert(kind);
    disk.rebuild();
    let row = disk.rows.iter().position(|&row| matches!(row, DiskRow::Item(_, DiskItem::Volume(i)) if i == index));
    disk.select(row.unwrap());
}

#[test]
fn measuring_reads_the_volumes_borg_can_reach_biggest_first() {
    let (mut state, effects) = measured_with(&[("shop_data", "/vol/shop_data"), ("blog_uploads", "/vol/blog_uploads")]);
    let [(id, ref names)] = reads(&effects)[..] else { panic!("one read: {effects:?}") };
    assert_eq!(names, &["blog_uploads", "shop_data"]);
    assert_eq!(state.docker.disk.reading, Some(id));
    let now = state.epoch;
    let read = DockerData::VolumeRead { name: "blog_uploads".into(), contents: changed("2026-10-02T09:00:00Z") };
    update(&mut state, Msg::Docker { id, result: Ok(read) }, now);
    update(&mut state, Msg::Docker { id, result: Ok(DockerData::Done) }, now);
    let disk = &state.docker.disk;
    assert_eq!(disk.reading, None);
    assert_eq!(disk.contents["blog_uploads"].top, ["2026", "index.json"]);
    assert_eq!(disk.unread(), [("shop_data".to_owned(), PathBuf::from("/vol/shop_data"))], "what was read stays read");

    let [measure] = measures(&press(&mut state, KeyCode::Char('r')))[..] else { panic!("R measures again") };
    assert!(state.docker.disk.contents.is_empty(), "R reads every volume again");
    let folders = [("blog_uploads".to_owned(), PathBuf::from("/vol/blog_uploads"))].into();
    let data = DockerData::DiskUse { usage: Box::new(usage()), context: None, contexts: None, folders };
    let effects = update(&mut state, Msg::Docker { id: measure, result: Ok(data) }, now);
    assert_eq!(reads(&effects).into_iter().map(|(_, names)| names).collect::<Vec<_>>(), [["blog_uploads"]]);
}

#[test]
fn enter_shows_a_reachable_volume_read_only_until_its_top_is_left() {
    let (mut state, _) = measured_with(&[("blog_uploads", "/vol/blog_uploads")]);
    let side = state.docker.side.unwrap();
    let host = state.panel(side).dir().to_path_buf();
    select_volume(&mut state, DiskKind::NamedVolumes, "blog_uploads");
    let effects = press(&mut state, KeyCode::Enter);
    assert!(!state.docker.is_open(), "the panel shows the volume in place of the Docker view");
    assert_eq!(listed(&effects), [Path::new("/vol/blog_uploads")]);
    arrive(&mut state, &effects);
    let root = PathBuf::from("/vol/blog_uploads");
    let mount = VolumeMount { name: "blog_uploads".into(), root: root.clone(), host: host.clone() };
    assert_eq!(state.panel(side).volume.as_ref(), Some(&mount));
    assert_eq!(state.panel(side).shell_folder(), host, "the shell stays out of the volume");

    let now = state.epoch;
    for action in [Action::Delete, Action::Mkdir, Action::Move, Action::EditInPanel] {
        state.notice = None;
        crate::actions::perform(&mut state, action, now);
        assert_eq!(state.notice.as_deref(), Some(tr!(docker_volume_read_only)), "{action:?}");
    }
    state.active = side.other();
    state.notice = None;
    crate::actions::perform(&mut state, Action::Copy, now);
    assert_eq!(state.notice.as_deref(), Some(tr!(docker_volume_read_only)), "nothing copies into it");
    state.active = side;

    let inside = state.navigate(side, root.join("2026"), None);
    arrive(&mut state, &[inside]);
    assert!(state.panel(side).volume.is_some(), "its folders are still the volume");
    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    assert_eq!(listed(&effects), [root.as_path()]);
    arrive(&mut state, &effects);
    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    assert_eq!(listed(&effects), [host.as_path()], "`..` at its top returns to the folder before");
    assert_eq!(state.panel(side).volume, None);
}

#[test]
fn going_anywhere_else_leaves_the_volume() {
    let (mut state, _) = measured_with(&[("blog_uploads", "/vol/blog_uploads")]);
    select_volume(&mut state, DiskKind::NamedVolumes, "blog_uploads");
    press(&mut state, KeyCode::Enter);
    let side = state.active;
    state.navigate(side, PathBuf::from("/vol"), None);
    assert_eq!(state.panel(side).volume, None);
    let now = state.epoch;
    crate::actions::perform(&mut state, Action::Mkdir, now);
    assert_ne!(state.notice.as_deref(), Some(tr!(docker_volume_read_only)));
}

#[test]
fn enter_on_a_volume_out_of_reach_says_why() {
    let (mut state, _) = measured_with(&[]);
    select_volume(&mut state, DiskKind::NamedVolumes, "blog_uploads");
    assert!(press(&mut state, KeyCode::Enter).is_empty());
    assert!(state.docker.disk.open);
    assert_eq!(state.notice.as_deref(), Some(tr!(docker_volume_out_of_reach)));
    state.docker.disk.usage.as_mut().unwrap().engine.system = "OrbStack".into();
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.notice.as_deref(),
        Some(tr!(docker_volume_share_denied)),
        "OrbStack shares them, so access was refused"
    );
}

#[test]
fn the_filter_finds_volumes_by_project_and_container_and_anonymous_ones_go_by_a_short_name() {
    let mut disk = disk(None);
    let usage = disk.usage.as_mut().unwrap();
    usage.volumes[1].compose = Some(borg_docker::Compose { project: "blog".into(), volume: Some("uploads".into()) });
    usage.volumes[1].name = "media".into();
    usage.volumes[2].name = "3b9d5e7f1a2c4b6d8e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d".into();
    disk.filter = "blog".into();
    disk.rebuild();
    assert_eq!(labels(&disk, DiskKind::NamedVolumes), ["media"]);
    disk.filter = "postgres".into();
    disk.rebuild();
    assert_eq!(labels(&disk, DiskKind::VolumesInUse), ["shop_data"], "the image of a container that mounts it");
    disk.filter.clear();
    disk.rebuild();
    assert_eq!(labels(&disk, DiskKind::AnonymousVolumes), ["3b9d5e7f1a2c"]);
    select_item(&mut disk, DiskKind::AnonymousVolumes, 0);
    assert!(cleanup(&disk).ends_with("volume rm 3b9d5e7f1a2c4b6d8e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d"));
}
