use super::*;
use crate::{Action, AppState, Appearance, Console, Effect, Msg, Settings, Startup, ThemeChoice, View, update};
use borg_docker::{CacheRecord, Image, Layer, State, Volume, VolumeUser};
use borg_i18n::tr;
use crossterm::event::KeyCode;
use std::time::Instant;

const MB: u64 = 1_000_000;

fn image(id: &str, tags: &[&str], repository: Option<&str>, size: Option<u64>, unique: u64, containers: u32) -> Image {
    Image {
        id: format!("sha256:{id}"),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        repository: repository.map(str::to_owned),
        size,
        shared: size.map(|size| size - unique),
        unique: size.map(|_| unique),
        created: None,
        containers: Some(containers),
    }
}

fn layer(name: &str, state: State, size: u64) -> Layer {
    Layer {
        id: format!("{name}-id"),
        name: name.into(),
        image: "shop-web:1.4".into(),
        state,
        status: String::new(),
        size: Some(size),
    }
}

fn user(container: &str, state: State, finished: Option<&str>) -> VolumeUser {
    VolumeUser {
        container: container.into(),
        image: "postgres:16".into(),
        state,
        path: "/var/lib/postgresql/data".into(),
        read_only: false,
        started: None,
        finished: finished.map(|time| time.parse().unwrap()),
    }
}

fn volume(name: &str, size: Option<u64>, users: &[&str], anonymous: bool) -> Volume {
    Volume {
        name: name.into(),
        size,
        links: Some(users.len() as u32),
        anonymous,
        created: None,
        compose: None,
        driver: "local".into(),
        options: Vec::new(),
        mountpoint: None,
        users: users.iter().map(|container| user(container, State::Running, None)).collect(),
    }
}

fn record(id: &str, size: u64, shared: bool, in_use: bool) -> CacheRecord {
    CacheRecord {
        id: id.into(),
        kind: "regular".into(),
        size: Some(size),
        shared,
        in_use,
        last_used: None,
        description: format!("[build {id}] RUN make"),
    }
}

/// The `shop` project's engine: a web image two containers use, an older worker image, a
/// dangling build, a digest pull Docker could not measure, and volumes of every kind.
fn usage() -> DiskUse {
    DiskUse {
        images: vec![
            image(
                "a1b2c3d4e5f6a7b8",
                &["shop-web:1.4", "shop-web:latest"],
                Some("shop-web"),
                Some(250 * MB),
                170 * MB,
                2,
            ),
            image("b2c3d4e5f6a7b8c9", &["shop-worker:1.3"], Some("shop-worker"), Some(240 * MB), 160 * MB, 0),
            image("c3d4e5f6a7b8c9d0", &[], None, Some(90 * MB), 90 * MB, 0),
            image("d4e5f6a7b8c9d0e1", &[], Some("mirror.example/tools/jq"), None, 0, 0),
        ],
        containers: vec![
            layer("shop-web-1", State::Running, MB),
            layer("shop-worker-1", State::Exited, 48 * MB),
            layer("shop-migrate-1", State::Created, 0),
        ],
        volumes: vec![
            volume("shop_data", Some(300 * MB), &["shop-db-1"], false),
            volume("blog_uploads", Some(860 * MB), &[], false),
            volume("3b9d5e7f", Some(512 * MB), &[], true),
            volume("scratch-notes", None, &[], false),
        ],
        cache: vec![
            record("r1", 412 * MB, false, false),
            record("r2", 75 * MB, true, false),
            record("r3", 5 * MB, false, true),
        ],
        networks: vec!["blog_default".into(), "shop_backend".into()],
        engine: borg_docker::Engine::default(),
    }
}

fn disk(context: Option<&str>) -> Disk {
    let mut disk = Disk { usage: Some(usage()), context: context.map(str::to_owned), ..Disk::default() };
    disk.rebuild();
    disk
}

fn kind_row(disk: &Disk, kind: DiskKind) -> usize {
    disk.rows.iter().position(|&row| matches!(row, DiskRow::Kind(group) if disk.groups[group].kind == kind)).unwrap()
}

/// Unfolds a kind and puts the cursor on its `index`th item.
fn select_item(disk: &mut Disk, kind: DiskKind, index: usize) {
    disk.unfolded.insert(kind);
    disk.rebuild();
    disk.select(kind_row(disk, kind) + 1 + index);
}

fn labels(disk: &Disk, kind: DiskKind) -> Vec<String> {
    let usage = disk.usage.as_ref().unwrap();
    disk.groups.iter().find(|group| group.kind == kind).unwrap().items.iter().map(|item| item.label(usage)).collect()
}

fn cleanup(disk: &Disk) -> String {
    String::from_utf8(disk.cleanup().expect("a command").line()).unwrap()
}

#[test]
fn kinds_split_what_docker_keeps_and_add_up_what_removing_it_frees() {
    let usage = usage();
    let totals = |kind: DiskKind| kind.totals(&usage);
    assert_eq!(totals(DiskKind::BuildCache), DiskTotals { count: 3, size: 492 * MB, reclaimable: 412 * MB });
    assert_eq!(totals(DiskKind::DanglingImages), DiskTotals { count: 1, size: 90 * MB, reclaimable: 90 * MB });
    assert_eq!(totals(DiskKind::UnusedImages), DiskTotals { count: 2, size: 240 * MB, reclaimable: 160 * MB });
    assert_eq!(totals(DiskKind::ImagesInUse), DiskTotals { count: 1, size: 250 * MB, reclaimable: 0 });
    assert_eq!(totals(DiskKind::StoppedContainers), DiskTotals { count: 2, size: 48 * MB, reclaimable: 48 * MB });
    assert_eq!(totals(DiskKind::AnonymousVolumes), DiskTotals { count: 1, size: 512 * MB, reclaimable: 512 * MB });
    assert_eq!(totals(DiskKind::NamedVolumes), DiskTotals { count: 2, size: 860 * MB, reclaimable: 860 * MB });
    assert_eq!(totals(DiskKind::VolumesInUse), DiskTotals { count: 1, size: 300 * MB, reclaimable: 0 });
    assert_eq!(totals(DiskKind::UnusedNetworks), DiskTotals { count: 2, size: 0, reclaimable: 0 });
    assert_eq!(disk(None).reclaimable(), (412 + 90 + 160 + 48 + 512 + 860) * MB);
}

#[test]
fn items_list_biggest_first_with_unmeasured_ones_last() {
    let mut disk = disk(None);
    assert_eq!(disk.rows.len(), DiskKind::ALL.len(), "kinds start folded");
    disk.unfolded.extend([DiskKind::UnusedImages, DiskKind::NamedVolumes, DiskKind::StoppedContainers]);
    disk.rebuild();
    assert_eq!(labels(&disk, DiskKind::UnusedImages), ["shop-worker:1.3", "mirror.example/tools/jq"]);
    assert_eq!(labels(&disk, DiskKind::ImagesInUse), ["shop-web:1.4 +1"]);
    assert_eq!(labels(&disk, DiskKind::NamedVolumes), ["blog_uploads", "scratch-notes"]);
    assert_eq!(labels(&disk, DiskKind::StoppedContainers), ["shop-worker-1", "shop-migrate-1"]);
    assert_eq!(disk.rows.len(), DiskKind::ALL.len() + 6);

    disk.filter = "SHOP".into();
    disk.rebuild();
    let kinds: Vec<DiskKind> = disk.groups.iter().map(|group| group.kind).collect();
    assert_eq!(
        kinds,
        [DiskKind::UnusedImages, DiskKind::ImagesInUse, DiskKind::StoppedContainers, DiskKind::VolumesInUse],
        "the filter keeps the kinds with matches, unfolded, and finds volumes by the containers that use them"
    );
    assert_eq!(disk.rows.len(), 4 + 5, "matching items show without unfolding");
}

#[test]
fn commands_name_the_context_when_the_shell_could_pick_another() {
    let names = |all: &[&str]| Some(all.iter().map(|name| (*name).to_owned()).collect());
    assert_eq!(named_context(Some("default".into()), names(&["default"])), None, "the only context");
    assert_eq!(named_context(Some("default".into()), names(&["default", "orbit"])), Some("default".into()));
    assert_eq!(named_context(Some("orbit".into()), None), Some("orbit".into()), "the list could not be read");
    assert_eq!(named_context(None, names(&["default", "orbit"])), None);
}

#[test]
fn a_kind_types_its_prune_and_kinds_in_use_type_nothing() {
    let mut disk = disk(Some("orbit"));
    let prune = |disk: &mut Disk, kind: DiskKind| {
        disk.select(kind_row(disk, kind));
        disk.cleanup().map(|line| String::from_utf8(line.line()).unwrap())
    };
    assert_eq!(prune(&mut disk, DiskKind::BuildCache).as_deref(), Ok("docker --context orbit builder prune"));
    assert_eq!(prune(&mut disk, DiskKind::DanglingImages).as_deref(), Ok("docker --context orbit image prune"));
    assert_eq!(prune(&mut disk, DiskKind::UnusedImages).as_deref(), Ok("docker --context orbit image prune -a"));
    assert_eq!(prune(&mut disk, DiskKind::StoppedContainers).as_deref(), Ok("docker --context orbit container prune"));
    assert_eq!(prune(&mut disk, DiskKind::AnonymousVolumes).as_deref(), Ok("docker --context orbit volume prune"));
    assert_eq!(prune(&mut disk, DiskKind::NamedVolumes).as_deref(), Ok("docker --context orbit volume prune -a"));
    assert_eq!(prune(&mut disk, DiskKind::UnusedNetworks).as_deref(), Ok("docker --context orbit network prune"));
    assert_eq!(prune(&mut disk, DiskKind::ImagesInUse), Err(NoCleanup::InUse));
    assert_eq!(prune(&mut disk, DiskKind::VolumesInUse), Err(NoCleanup::InUse));
    for kind in DiskKind::ALL {
        let line = prune(&mut disk, kind).unwrap_or_default();
        assert!(!line.split(' ').any(|word| word == "-f" || word == "--force"), "{line}");
    }
    assert_eq!(Disk::default().cleanup().map(|line| line.line()), Err(NoCleanup::Empty));
}

#[test]
fn items_go_by_every_tag_or_their_id_and_marked_ones_go_together() {
    let mut disk = disk(None);
    select_item(&mut disk, DiskKind::UnusedImages, 0);
    assert_eq!(cleanup(&disk), "docker rmi shop-worker:1.3");
    select_item(&mut disk, DiskKind::UnusedImages, 1);
    assert_eq!(cleanup(&disk), "docker rmi d4e5f6a7b8c9", "an untagged image goes by its ID");
    assert_eq!(disk.removing(), 1);

    disk.toggle_mark();
    select_item(&mut disk, DiskKind::DanglingImages, 0);
    disk.toggle_mark();
    select_item(&mut disk, DiskKind::StoppedContainers, 0);
    disk.toggle_mark();
    assert_eq!(disk.mark_count(), 3);
    select_item(&mut disk, DiskKind::UnusedImages, 0);
    assert_eq!(cleanup(&disk), "docker rmi c3d4e5f6a7b8 d4e5f6a7b8c9", "the marked images, not the selected one");
    assert_eq!(disk.removing(), 2);
    select_item(&mut disk, DiskKind::StoppedContainers, 1);
    assert_eq!(cleanup(&disk), "docker rm shop-worker-1");

    select_item(&mut disk, DiskKind::ImagesInUse, 0);
    disk.toggle_mark();
    assert_eq!(disk.mark_count(), 3, "images in use cannot be marked");
    assert_eq!(disk.cleanup().map(|line| line.line()), Err(NoCleanup::InUse));
    select_item(&mut disk, DiskKind::BuildCache, 0);
    assert_eq!(disk.cleanup().map(|line| line.line()), Err(NoCleanup::CacheRecord));

    disk.select(kind_row(&disk, DiskKind::NamedVolumes));
    disk.invert_marks();
    select_item(&mut disk, DiskKind::AnonymousVolumes, 0);
    assert_eq!(cleanup(&disk), "docker volume rm blog_uploads scratch-notes", "inverting marks the whole kind");
    disk.toggle_mark();
    assert_eq!(cleanup(&disk), "docker volume rm 3b9d5e7f blog_uploads scratch-notes");
}

#[test]
fn removing_every_tag_keeps_the_rest_of_the_line_quoted() {
    let mut usage = usage();
    usage.images[1].tags = vec!["registry.example:5000/shop-worker:1.3".into(), "shop worker:odd".into()];
    let mut disk = Disk { usage: Some(usage), context: Some("ci's".into()), ..Disk::default() };
    select_item(&mut disk, DiskKind::UnusedImages, 0);
    assert_eq!(
        cleanup(&disk),
        "docker --context 'ci'\\''s' rmi registry.example:5000/shop-worker:1.3 'shop worker:odd'"
    );
}

#[test]
fn marks_and_the_cursor_survive_a_new_measurement_until_their_items_go() {
    let mut disk = disk(None);
    select_item(&mut disk, DiskKind::NamedVolumes, 1);
    disk.toggle_mark();
    select_item(&mut disk, DiskKind::StoppedContainers, 0);
    disk.toggle_mark();
    assert_eq!(disk.mark_count(), 2);

    let mut usage = usage();
    usage.volumes.retain(|volume| volume.name != "blog_uploads");
    usage.containers[1].state = State::Running;
    disk.usage = Some(usage);
    disk.rebuild();
    assert_eq!(disk.mark_count(), 1, "a container that started again is no longer marked");
    let marked = disk.marked(DiskItem::Volume(2));
    assert!(marked, "scratch-notes keeps its mark at its new index");
    let usage = disk.usage.as_ref().unwrap();
    let DiskRow::Item(_, item) = disk.row().unwrap() else { unreachable!() };
    assert_eq!(item.label(usage), "shop-migrate-1", "the cursor stays on its row when its item leaves the kind");
}

fn state() -> AppState {
    let appearance = Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0);
    let (state, _) =
        AppState::new(Startup::at("/work".into()), 120, 32, Settings::default(), appearance, Instant::now());
    state
}

fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let now = state.epoch;
    update(state, Msg::Key(code.into()), now)
}

/// What `effects` type at the prompt.
fn typed(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Write(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect()
}

/// The disk requests among `effects`.
fn measures(effects: &[Effect]) -> Vec<u64> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Docker { id, request: crate::DockerRequest::DiskUse } => Some(*id),
            _ => None,
        })
        .collect()
}

/// Disk use open on the shop project's engine, with one context.
fn measured() -> AppState {
    let mut state = state();
    let now = state.epoch;
    let effects = crate::actions::perform(&mut state, Action::DockerDisk, now);
    let [id] = measures(&effects)[..] else { panic!("opening measures once: {effects:?}") };
    assert!(state.docker.is_open() && state.docker.disk.open);
    let data = crate::DockerData::DiskUse {
        usage: Box::new(usage()),
        context: Some("default".into()),
        contexts: Some(vec!["default".into()]),
        folders: HashMap::new(),
    };
    update(&mut state, Msg::Docker { id, result: Ok(data) }, now);
    assert!(state.docker.disk.loading.is_none());
    state
}

#[test]
fn cleanup_types_the_command_without_enter_and_measures_again_once_it_ran() {
    let mut state = measured();
    state.shell.integrated = true;
    assert_eq!(state.docker.disk.context, None, "a single context goes unnamed");
    let [id] = measures(&press(&mut state, KeyCode::Char('r')))[..] else { panic!("R measures again") };
    assert!(measures(&press(&mut state, KeyCode::Char('r'))).is_empty(), "one measurement at a time");
    let now = state.epoch;
    update(&mut state, Msg::Docker { id, result: Err("Cannot connect to the Docker daemon".into()) }, now);
    assert!(state.docker.disk.error.is_some() && state.docker.disk.usage.is_some(), "a failure keeps the last numbers");

    let effects = press(&mut state, KeyCode::F(8));
    assert_eq!(typed(&effects), ["docker builder prune"], "no Enter: {effects:?}");
    assert_eq!((state.view, state.shell.return_view), (View::Shell, View::Panels));
    assert!(!crate::update::shell_ready(&state), "the typed command waits at the prompt");

    state.settings.preferences.after_command = crate::AfterCommand::ShowShell;
    update(&mut state, Msg::Signal(borg_term::ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert!(state.docker.disk.stale, "the command that ended is taken to be the cleanup");
    assert!(measures(&update(&mut state, Msg::Tick, now)).is_empty(), "the console hides the view");
    state.view = View::Panels;
    let effects = update(&mut state, Msg::Tick, now);
    assert_eq!(measures(&effects).len(), 1, "back in the view, it measures again: {effects:?}");
    assert!(!state.docker.disk.stale);
    update(&mut state, Msg::Signal(borg_term::ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert!(!state.docker.disk.stale, "later commands leave it alone");
}

#[test]
fn a_busy_shell_or_another_console_types_nothing() {
    let mut state = measured();
    state.shell.running = true;
    assert!(typed(&press(&mut state, KeyCode::F(8))).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(shell_is_busy_finish_or_clear_the)));
    assert_eq!(state.view, View::Panels);

    state.shell.running = false;
    state.console = Console::Remote { machine: "server".into(), session: "main".into() };
    assert!(typed(&press(&mut state, KeyCode::F(8))).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(docker_disk_host_console)));

    state.console = Console::Host;
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Down);
    assert!(typed(&press(&mut state, KeyCode::Delete)).is_empty(), "build cache records go together");
    assert_eq!(state.notice.as_deref(), Some(tr!(docker_disk_cache_together)));
    assert!(!state.docker.disk.typed);
}

#[test]
fn d_switches_between_the_containers_and_disk_use() {
    let mut state = measured();
    assert!(measures(&press(&mut state, KeyCode::Char('d'))).is_empty());
    assert!(state.docker.is_open() && !state.docker.disk.open, "D returns to the containers");
    assert!(measures(&press(&mut state, KeyCode::Char('D'))).is_empty(), "a fresh measurement is kept");
    assert!(state.docker.disk.open);
    press(&mut state, KeyCode::Esc);
    assert!(state.docker.is_open() && !state.docker.disk.open, "Esc returns to the containers");
    assert!(measures(&press(&mut state, KeyCode::Char('d'))).is_empty());

    press(&mut state, KeyCode::Char('/'));
    press(&mut state, KeyCode::Char('d'));
    assert_eq!((state.docker.disk.filter.as_str(), state.docker.disk.open), ("d", true), "the filter takes letters");
}

#[test]
fn a_name_with_a_control_character_types_no_command() {
    let mut usage = usage();
    usage.containers[1].name = "shop-worker-1\nlater".into();
    let mut disk = Disk { usage: Some(usage), ..Disk::default() };
    select_item(&mut disk, DiskKind::StoppedContainers, 0);
    assert_eq!(disk.cleanup().map(|line| line.line()), Err(NoCleanup::ControlCharacter));
    let mut disk = Disk { usage: Some(self::usage()), context: Some("orbit\u{7}".into()), ..Disk::default() };
    disk.rebuild();
    disk.select(kind_row(&disk, DiskKind::UnusedNetworks));
    assert_eq!(disk.cleanup().map(|line| line.line()), Err(NoCleanup::ControlCharacter), "nor a context's name");
}

mod volumes;
