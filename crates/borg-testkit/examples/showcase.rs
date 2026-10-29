//! Renders the README showcase of folder sizes, the activity monitor, and Docker from
//! fictional folders, processes and containers, so no screenshot shows the machine it was
//! made on.

use std::{
    env,
    f64::consts::PI,
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Result, ensure};
use borg_core::{
    ActivityData, ActivityRequest, MemoryRegion, Motion, Msg, Preferences, ProcessInfo, ProcessKey, ResultSource,
    SearchKind, SearchOptions, SearchProgress, SearchRequest, Settings, Sort, SortKey, SystemSample,
};
use borg_core::{DockerData, DockerRequest};
use borg_docker::{Container, Details, Mount, Network, State, Stats};
use borg_term::GridSize;
use borg_testkit::{Snapshot, Stage};
use borg_vfs::{Entry, EntryKind, FolderUsage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const SIZE: GridSize = GridSize { cols: 120, rows: 32 };
const CLOCK: &str = "2026-10-21T12:00:00+00:00[UTC]";
const NOW: u64 = 1_792_584_000;
const MEASURED: u64 = NOW - 7 * 60;
const HOME: &str = "/home/demo";
const PROJECTS: &str = "/home/demo/projects";
const ATLAS: &str = "/home/demo/projects/atlas";
const SIZES_THEME: &str = "neon";
const ACTIVITY_THEME: &str = "ember";
const DOCKER_THEME: &str = "aurora";
const SAMPLES: usize = 60;
const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;
const PAGE: u64 = 16 * KIB;
const SELECTED: u32 = 4012;
const EXECUTABLE: &str = "/usr/local/bin/rust-analyzer";

fn at(secs: u64) -> Option<std::time::SystemTime> {
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

fn file(name: &str, size: u64) -> Entry {
    Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind: EntryKind::File,
        size,
        modified: at(NOW - 3 * 86_400),
        executable: false,
        permissions: 0o644,
    }
}

/// A folder with a measured total, named after the last component of `path`.
fn folder(path: &str, bytes: u64, files: u64, folders: u64) -> Entry {
    let path = PathBuf::from(path);
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
    Entry {
        source: Some(path),
        usage: Some(FolderUsage { files, folders, measured: at(MEASURED), ..Default::default() }),
        kind: EntryKind::Dir,
        ..file(&name, bytes)
    }
}

fn listed(entry: Entry) -> Entry {
    Entry { source: None, ..entry }
}

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn theme(stage: &mut Stage, id: &str) {
    let index = stage.state.appearance.choices().iter().position(|c| c.id == id).expect("built-in theme");
    stage.state.appearance.select(index, stage.started, Duration::ZERO);
}

fn stage(left: &str, right: &str, listings: [Vec<Entry>; 2]) -> Result<Stage> {
    let preferences = Preferences { nerd_font: true, menu_bar: true, git_plugin: false, ..Preferences::DEFAULT };
    let settings = Settings { motion: Motion::STILL, preferences, ..Settings::default() };
    let mut stage = Stage::new([left.into(), right.into()], listings, SIZE, settings, CLOCK.parse()?)?;
    stage.home = Some(HOME.into());
    Ok(stage)
}

fn save(stage: &mut Stage, output: &Path, name: &str) -> Result<()> {
    let shot = Snapshot::from_buffer(stage.render(stage.started)?, &stage.palette, None);
    let text = shot.lines().join("\n");
    ensure!(!text.contains("/Users/") && !text.contains("/var/folders/"), "{name} must not show host folders");
    shot.save_png(&output.join(name).with_extension("png"))?;
    println!("wrote {}", output.join(name).with_extension("png").display());
    Ok(())
}

fn projects() -> Vec<Entry> {
    [
        ("atlas", 7_912 * MIB, 212_480, 18_204),
        ("website", 2_310 * MIB, 96_112, 9_870),
        ("ml-notebooks", 1_480 * MIB, 3_204, 211),
        ("landing", 655 * MIB, 41_906, 4_012),
        ("dotfiles", 2 * MIB, 184, 37),
        ("notes", 18 * MIB, 912, 64),
    ]
    .into_iter()
    .map(|(name, bytes, files, folders)| listed(folder(&format!("{PROJECTS}/{name}"), bytes, files, folders)))
    .chain([file("README.md", 4_812), file("todo.txt", 920)])
    .collect()
}

fn size_tree(output: &Path) -> Result<()> {
    let mut stage = stage(ATLAS, PROJECTS, [Vec::new(), projects()])?;
    theme(&mut stage, SIZES_THEME);
    let tree = [
        ("", 7_912 * MIB, 212_480, 7),
        ("/target", 4_640 * MIB, 61_214, 2),
        ("/target/debug", 3_910 * MIB, 55_870, 3),
        ("/target/debug/deps", 2_730 * MIB, 31_002, 0),
        ("/target/debug/build", 655 * MIB, 14_210, 0),
        ("/target/debug/incremental", 418 * MIB, 10_388, 0),
        ("/target/release", 730 * MIB, 5_344, 0),
        ("/node_modules", 1_840 * MIB, 138_420, 0),
        ("/assets", 1_090 * MIB, 2_118, 0),
        ("/.git", 312 * MIB, 9_844, 0),
        ("/docs", 24 * MIB, 412, 0),
        ("/src", 3 * MIB, 386, 0),
        ("/scripts", 96 * KIB, 26, 0),
    ]
    .map(|(path, bytes, files, folders)| {
        let mut entry = folder(&format!("{ATLAS}{path}"), bytes, files, folders);
        if path == "/target/debug" {
            let usage = entry.usage.as_mut().unwrap();
            usage.direct_files = 270;
            usage.direct_bytes = 107 * MIB;
        }
        entry
    });
    stage.state.panels[0].open_results(
        ResultSource::Search {
            request: SearchRequest {
                skip: None,
                root: ATLAS.into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTree, ..Default::default() },
            },
            progress: SearchProgress { measured: at(MEASURED), visited: 230_684, ..Default::default() },
        },
        tree.into(),
        80,
    );
    stage.state.panels[0].set_sort(Sort { key: SortKey::Size, reverse: true });
    stage.state.panels[0].move_by(1);
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Right)).send(key(KeyCode::Right));
    save(&mut stage, output, "size-tree")
}

fn folder_totals(output: &Path) -> Result<()> {
    let mut stage = stage(PROJECTS, ATLAS, [projects(), Vec::new()])?;
    theme(&mut stage, SIZES_THEME);
    let found = [
        ("atlas/target/debug/deps", 2_730 * MIB, 31_002, 0, 1),
        ("atlas/node_modules", 1_840 * MIB, 138_420, 12_904, 2),
        ("website/node_modules", 1_120 * MIB, 84_118, 8_377, 5),
        ("ml-notebooks/datasets", 1_310 * MIB, 48, 6, 30),
        ("atlas/assets/video", 905 * MIB, 64, 3, 12),
        ("atlas/target/release", 730 * MIB, 5_344, 41, 1),
        ("atlas/target/debug/build", 655 * MIB, 14_210, 980, 1),
        ("landing/node_modules", 612 * MIB, 40_215, 3_910, 44),
        ("website/.next/cache", 486 * MIB, 22_731, 1_412, 3),
        ("atlas/target/debug/incremental", 418 * MIB, 10_388, 312, 1),
        ("atlas/.git/objects", 301 * MIB, 9_610, 258, 2),
        ("website/packages/ui/node_modules", 214 * MIB, 15_402, 1_388, 9),
    ]
    .map(|(path, bytes, files, folders, days)| Entry {
        modified: at(NOW - days * 86_400 - 3_600 * (files % 11)),
        ..folder(&format!("{PROJECTS}/{path}"), bytes, files, folders)
    });
    stage.state.panels[0].open_results(
        ResultSource::Search {
            request: SearchRequest {
                skip: None,
                root: PROJECTS.into(),
                query: "*".into(),
                hidden: true,
                options: SearchOptions { kind: SearchKind::FolderTotals, minimum: 200 * MIB, ..Default::default() },
            },
            progress: SearchProgress { measured: at(MEASURED), visited: 412_966, ..Default::default() },
        },
        found.into(),
        80,
    );
    stage.state.panels[0].set_sort(Sort { key: SortKey::Size, reverse: true });
    stage.state.panels[0].results.as_mut().unwrap().expanded = true;
    stage.state.panels[0].move_by(isize::MIN);
    save(&mut stage, output, "folder-totals")
}

struct Process {
    pid: u32,
    name: &'static str,
    executable: &'static str,
    parent: Option<u32>,
    resident: u64,
    cpu: u32,
}

const PROCESSES: [Process; 16] = [
    Process { pid: 4012, name: "rust-analyzer", executable: EXECUTABLE, parent: Some(3001), resident: 0, cpu: 1_240 },
    Process {
        pid: 5133,
        name: "rustc",
        executable: "/opt/rust/bin/rustc",
        parent: Some(5120),
        resident: 1_090 * MIB,
        cpu: 9_820,
    },
    Process {
        pid: 3880,
        name: "node",
        executable: "/usr/local/bin/node",
        parent: Some(1022),
        resident: 742 * MIB,
        cpu: 610,
    },
    Process {
        pid: 3001,
        name: "editor",
        executable: "/usr/local/bin/editor",
        parent: None,
        resident: 638 * MIB,
        cpu: 380,
    },
    Process {
        pid: 2210,
        name: "postgres",
        executable: "/usr/local/bin/postgres",
        parent: None,
        resident: 512 * MIB,
        cpu: 120,
    },
    Process {
        pid: 5120,
        name: "cargo",
        executable: "/opt/rust/bin/cargo",
        parent: Some(1022),
        resident: 410 * MIB,
        cpu: 2_300,
    },
    Process {
        pid: 3520,
        name: "photo-indexer",
        executable: "/usr/local/libexec/photo-indexer",
        parent: None,
        resident: 336 * MIB,
        cpu: 40,
    },
    Process {
        pid: 3410,
        name: "music",
        executable: "/usr/local/bin/music",
        parent: None,
        resident: 208 * MIB,
        cpu: 90,
    },
    Process {
        pid: 2750,
        name: "containerd",
        executable: "/usr/local/bin/containerd",
        parent: None,
        resident: 182 * MIB,
        cpu: 30,
    },
    Process {
        pid: 3300,
        name: "mail-sync",
        executable: "/usr/local/libexec/mail-sync",
        parent: None,
        resident: 118 * MIB,
        cpu: 10,
    },
    Process {
        pid: 2214,
        name: "postgres",
        executable: "/usr/local/bin/postgres",
        parent: Some(2210),
        resident: 96 * MIB,
        cpu: 20,
    },
    Process {
        pid: 3640,
        name: "backup-agent",
        executable: "/usr/local/libexec/backup-agent",
        parent: None,
        resident: 88 * MIB,
        cpu: 0,
    },
    Process {
        pid: 2602,
        name: "redis-server",
        executable: "/usr/local/bin/redis-server",
        parent: None,
        resident: 64 * MIB,
        cpu: 20,
    },
    Process { pid: 4410, name: "borg", executable: "/usr/local/bin/borg", parent: None, resident: 38 * MIB, cpu: 90 },
    Process { pid: 1022, name: "zsh", executable: "/bin/zsh", parent: Some(4410), resident: 7 * MIB, cpu: 0 },
    Process { pid: 3700, name: "ssh-agent", executable: "/usr/bin/ssh-agent", parent: None, resident: 4 * MIB, cpu: 0 },
];

/// Resident memory of the language server: indexing, a release of caches, then a slow climb.
fn analyzer(tick: usize) -> u64 {
    let t = tick as f64;
    let indexing = 1.38 + 0.34 / (1.0 + (-(t - 18.0) / 4.0).exp());
    let released = if tick >= 38 { -0.17 + 0.004 * (t - 38.0) } else { 0.0 };
    let ripple = 0.012 * (t * PI / 5.0).sin();
    ((indexing + released + ripple) * GIB as f64) as u64
}

fn sample(tick: usize) -> SystemSample {
    let processes = PROCESSES
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let wobble = ((tick + i * 7) as f64 / 3.0).sin() * p.resident as f64 * 0.01;
            ProcessInfo {
                key: ProcessKey { pid: p.pid, start: 100 },
                parent: p.parent,
                name: p.name.into(),
                executable: Some(p.executable.into()),
                cpu: Some(p.cpu),
                resident: Some(if p.pid == SELECTED { analyzer(tick) } else { (p.resident as f64 + wobble) as u64 }),
                virtual_bytes: Some(if p.pid == SELECTED { 412 * GIB } else { p.resident * 6 + 4 * GIB }),
                read: Some(if p.pid == 5133 { 48 * MIB } else { 120 * KIB * (i as u64 % 4) }),
                written: Some(if p.pid == 5133 { 22 * MIB } else { 16 * KIB * (i as u64 % 3) }),
            }
        })
        .collect();
    SystemSample {
        processes,
        total: 32 * GIB,
        used: 21 * GIB + 400 * MIB,
        swap_total: 2 * GIB,
        swap_used: 412 * MIB,
        cpu: vec![9_100, 7_800, 6_400, 5_200, 2_900, 2_100, 1_400, 800],
    }
}

/// A macOS-style memory map: regions laid out one after another, sized in pages.
struct Map {
    regions: Vec<MemoryRegion>,
    next: u64,
}

impl Map {
    fn add(&mut self, kind: &str, detail: Option<&str>, permissions: &str, sizes: [u64; 3]) -> &mut Self {
        let [mapped, resident, dirty] = sizes.map(|bytes| bytes.div_ceil(PAGE) * PAGE);
        let path = detail.filter(|d| d.starts_with('/'));
        self.regions.push(MemoryRegion {
            start: self.next,
            end: self.next + mapped,
            permissions: permissions.into(),
            name: detail.map_or_else(|| kind.into(), |d| format!("{kind} · {d}")),
            resident: Some(resident),
            swap: Some(0),
            dirty: Some(dirty),
            path: path.map(Into::into),
            ..Default::default()
        });
        self.next += mapped + PAGE;
        self
    }
}

const LIBRARIES: [(&str, u64); 22] = [
    ("/usr/lib/system/libsystem_malloc.dylib", 420),
    ("/usr/lib/libc++.1.dylib", 1_180),
    ("/usr/lib/libobjc.A.dylib", 660),
    ("/usr/lib/system/libsystem_kernel.dylib", 270),
    ("/usr/lib/system/libsystem_pthread.dylib", 64),
    ("/usr/lib/system/libdyld.dylib", 210),
    ("/usr/lib/dyld", 690),
    ("/usr/lib/libSystem.B.dylib", 16),
    ("/usr/lib/libz.1.dylib", 96),
    ("/usr/lib/libiconv.2.dylib", 1_020),
    ("/usr/lib/libxml2.2.dylib", 1_450),
    ("/usr/lib/libsqlite3.dylib", 1_610),
    ("/usr/lib/libicucore.A.dylib", 3_900),
    ("/usr/lib/libcompression.dylib", 340),
    ("/usr/lib/swift/libswiftCore.dylib", 5_800),
    ("/usr/lib/system/libcorecrypto.dylib", 610),
    ("/usr/lib/libnetwork.dylib", 3_300),
    ("/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation", 5_200),
    ("/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation", 11_600),
    ("/System/Library/Frameworks/Security.framework/Versions/A/Security", 7_900),
    ("/System/Library/Frameworks/CFNetwork.framework/Versions/A/CFNetwork", 4_100),
    ("/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit", 1_200),
];

fn memory_map() -> Vec<MemoryRegion> {
    let mut map = Map { regions: Vec::new(), next: 0x1_0000_0000 };
    map.add("__TEXT", Some(EXECUTABLE), "r-x", [41 * MIB, 33 * MIB, 0])
        .add("__DATA_CONST", Some(EXECUTABLE), "r--", [2 * MIB, 2 * MIB, 96 * KIB])
        .add("__DATA", Some(EXECUTABLE), "rw-", [640 * KIB, 512 * KIB, 448 * KIB])
        .add("__LINKEDIT", Some(EXECUTABLE), "r--", [9 * MIB, 2 * MIB, 0]);
    for (i, (bytes, dirty)) in
        [(384, 371), (256, 249), (192, 180), (128, 121), (96, 88), (64, 61)].into_iter().enumerate()
    {
        map.add("MALLOC_LARGE", Some("DefaultMallocZone"), "rw-", [bytes * MIB, dirty * MIB, (dirty - i as u64) * MIB]);
    }
    for _ in 0..6 {
        map.add("MALLOC_SMALL", Some("DefaultMallocZone"), "rw-", [32 * MIB, 21 * MIB, 19 * MIB]);
    }
    map.add("MALLOC_TINY", Some("DefaultMallocZone"), "rw-", [4 * MIB, 3 * MIB, 3 * MIB])
        .add("MALLOC metadata", Some("DefaultMallocZone"), "rw-", [800 * KIB, 640 * KIB, 640 * KIB])
        .add("VM_ALLOCATE", None, "rw-", [24 * MIB, 18 * MIB, 18 * MIB]);
    for thread in 0..14u64 {
        let detail = format!("thread {thread}");
        let (size, used) = if thread == 0 { (8 * MIB, 512 * KIB) } else { (2 * MIB, (64 + thread * 32) * KIB) };
        map.add("STACK GUARD", Some(&format!("stack guard for thread {thread}")), "---", [16 * KIB, 0, 0]).add(
            "Stack",
            Some(&detail),
            "rw-",
            [size, used, used],
        );
    }
    let base = map.next.max(0x1_8000_0000);
    map.next = base;
    for (path, text) in LIBRARIES {
        let text = text * KIB;
        map.add("__TEXT", Some(path), "r-x", [text, text * 9 / 10, 0])
            .add("__DATA_CONST", Some(path), "r--", [text / 12, text / 14, 0])
            .add("__DATA", Some(path), "rw-", [text / 40 + KIB, text / 50 + KIB, text / 90 + KIB])
            .add("__AUTH_CONST", Some(path), "r--", [text / 20, text / 30, 0]);
    }
    map.add("__LINKEDIT", Some("/usr/lib/dyld"), "r--", [612 * MIB, 54 * MIB, 0])
        .add("mapped file", Some("/usr/share/icu/icudt76l.dat"), "r--", [31 * MIB, 3 * MIB, 0])
        .add("mapped file", Some("/opt/rust/lib/rustlib/src/rust/library/std.idx"), "r--", [18 * MIB, 11 * MIB, 0])
        .add("shared memory", None, "rw-", [64 * KIB, 64 * KIB, 64 * KIB])
        .add("shared memory", None, "r--", [32 * KIB, 32 * KIB, 32 * KIB])
        .add("Kernel Alloc Once", None, "rw-", [32 * KIB, 16 * KIB, 16 * KIB])
        .add("page table in kernel", None, "rw-", [3 * MIB, 3 * MIB, 3 * MIB])
        .add("VM_ALLOCATE (reserved)", Some("reserved VM address space (unallocated)"), "rw-", [1_024 * MIB, 0, 0])
        .add("MALLOC guard page", None, "---", [32 * KIB, 0, 0]);
    map.regions
}

fn activity(output: &Path) -> Result<()> {
    let mut stage = stage(PROJECTS, ATLAS, [projects(), Vec::new()])?;
    theme(&mut stage, ACTIVITY_THEME);
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)));
    for tick in 0..SAMPLES {
        let id = stage.state.activity.pending.as_ref().map_or(0, |p| p.0);
        stage.send(Msg::Activity { id, result: Ok(ActivityData::Sample(sample(tick))) });
        let next = stage.state.activity.pending.as_ref().map_or(id + 1, |p| p.0);
        stage.state.activity.pending = Some((next, ActivityRequest::Sample));
    }
    stage.state.activity.pending = None;
    ensure!(stage.state.activity.selected.map(|p| p.pid) == Some(SELECTED), "the language server leads the list");
    save(&mut stage, output, "activity")?;

    stage.send(key(KeyCode::Enter));
    let id = stage.state.activity.pending.as_ref().map_or(0, |p| p.0);
    stage.send(Msg::Activity { id, result: Ok(ActivityData::Regions(memory_map())) });
    let libraries = stage
        .state
        .activity
        .region_rows()
        .iter()
        .position(|row| *row == borg_core::RegionRow::Group(borg_core::MemoryGroup::Libraries))
        .unwrap_or(0);
    for _ in 0..libraries {
        stage.send(key(KeyCode::Down));
    }
    stage.send(key(KeyCode::Right)).send(key(KeyCode::Up));
    stage.state.activity.pending = None;
    save(&mut stage, output, "memory-map")
}

fn container(id: &str, project: Option<&str>, service: &str, state: State, status: &str) -> Container {
    Container {
        id: format!("{id}{}", "0".repeat(60)),
        name: project.map_or_else(|| service.to_owned(), |p| format!("{p}-{service}-1")),
        image: match service {
            "db" => "postgres:17".into(),
            "cache" => "redis:7.4".into(),
            "web" => "nginx:1.27".into(),
            other => format!("ghcr.io/demo/{other}:2.3"),
        },
        command: "docker-entrypoint.sh".into(),
        created: "2026-10-21 09:12:44 +0000 UTC".into(),
        state,
        status: status.into(),
        ports: match service {
            "web" => "0.0.0.0:8080->80/tcp".into(),
            "api" => "0.0.0.0:3000->3000/tcp".into(),
            _ => String::new(),
        },
        networks: project.map_or_else(|| "bridge".into(), |p| format!("{p}_default")),
        project: project.map(str::to_owned),
        service: project.map(|_| service.to_owned()),
        project_dir: project.map(|p| format!("{PROJECTS}/{p}")),
        config_files: project.map(|p| vec![format!("{PROJECTS}/{p}/compose.yaml")]).unwrap_or_default(),
    }
}

/// The Docker view over fictional Compose projects, on the API container: its details and a
/// minute of live CPU and memory.
fn docker(output: &Path) -> Result<()> {
    let mut stage = stage(PROJECTS, ATLAS, [projects(), Vec::new()])?;
    theme(&mut stage, DOCKER_THEME);
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)));
    let Some((id, DockerRequest::List)) = stage.state.docker.pending.clone() else { anyhow::bail!("Docker lists") };
    let containers = vec![
        container("a1", Some("shop"), "api", State::Running, "Up 3 hours (healthy)"),
        container("a2", Some("shop"), "web", State::Running, "Up 3 hours"),
        container("a3", Some("shop"), "db", State::Running, "Up 3 hours (healthy)"),
        container("a4", Some("shop"), "cache", State::Running, "Up 3 hours"),
        container("a5", Some("shop"), "worker", State::Exited, "Exited (1) 12 minutes ago"),
        container("b1", Some("blog"), "app", State::Running, "Up 2 days"),
        container("b2", Some("blog"), "db", State::Paused, "Up 2 days (Paused)"),
        container("c1", None, "scratch", State::Exited, "Exited (0) 2 days ago"),
    ];
    let ids: Vec<String> = containers.iter().map(|c| c.id.clone()).collect();
    stage
        .send(Msg::Docker { id, result: Ok(DockerData::Listed { context: Some("desktop-linux".into()), containers }) });
    for second in 0..SAMPLES as u64 {
        let wave = (second as f64 / 5.0).sin().abs();
        let stats = |cpu: f64, memory: u64| Stats {
            cpu: (cpu * 100.0) as u32,
            memory,
            memory_limit: 16 * GIB,
            net_rx: 184 * MIB,
            net_tx: 61 * MIB,
            read: 22 * MIB,
            written: 9 * MIB,
            pids: 24,
        };
        stage.send(Msg::DockerStats(vec![
            (ids[0].clone(), stats(6.0 + 34.0 * wave, 310 * MIB + second * 2 * MIB)),
            (ids[1].clone(), stats(0.4, 24 * MIB)),
            (ids[2].clone(), stats(2.1 + wave, 612 * MIB)),
            (ids[3].clone(), stats(0.9, 48 * MIB)),
            (ids[5].clone(), stats(1.4, 188 * MIB)),
            (ids[6].clone(), stats(0.0, 96 * MIB)),
        ]));
    }
    stage.send(Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    for _ in 0..3 {
        stage.send(Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    }
    ensure!(stage.state.docker.container().is_some_and(|c| c.label() == "api"), "the API container is selected");
    let at = stage.started + Duration::from_secs(1);
    borg_core::update(&mut stage.state, Msg::Tick, at);
    let Some((id, DockerRequest::Inspect(inspected))) = stage.state.docker.pending.clone() else {
        anyhow::bail!("resting on a container inspects it")
    };
    let details = Details {
        started: Some("2026-10-21T09:12:46Z".into()),
        finished: None,
        exit_code: 0,
        oom_killed: false,
        error: String::new(),
        health: Some("healthy".into()),
        restarts: 0,
        restart_policy: "unless-stopped".into(),
        command: "node dist/server.js".into(),
        workdir: "/app".into(),
        platform: "linux".into(),
        mounts: vec![Mount {
            kind: "volume".into(),
            source: "shop_uploads".into(),
            destination: "/app/uploads".into(),
            writable: true,
        }],
        networks: vec![Network { name: "shop_default".into(), address: "172.18.0.5".into() }],
    };
    stage.send(Msg::Docker { id, result: Ok(DockerData::Inspected { id: inspected, details: Box::new(details) }) });
    stage.started = at;
    save(&mut stage, output, "docker")
}

fn main() -> Result<()> {
    let output = PathBuf::from(env::args().nth(1).unwrap_or_else(|| "docs/showcase".into()));
    fs::create_dir_all(&output)?;
    size_tree(&output)?;
    folder_totals(&output)?;
    activity(&output)?;
    docker(&output)
}
