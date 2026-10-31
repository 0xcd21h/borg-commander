use super::*;
use crate::HISTORY;
use borg_docker::{Action, Container, State, Stats};
use std::time::Instant;

fn container(id: &str, project: Option<&str>, service: &str, state: State) -> Container {
    Container {
        id: id.into(),
        name: project.map_or_else(|| service.to_owned(), |p| format!("{p}-{service}-1")),
        image: format!("{service}:latest"),
        command: String::new(),
        created: String::new(),
        state,
        status: String::new(),
        ports: String::new(),
        networks: String::new(),
        project: project.map(str::to_owned),
        service: project.map(|_| service.to_owned()),
        project_dir: None,
        config_files: Vec::new(),
    }
}

fn docker() -> Docker {
    let mut docker = Docker {
        containers: vec![
            container("c1", None, "scratch", State::Exited),
            container("w1", Some("web"), "worker", State::Running),
            container("a1", Some("api"), "db", State::Running),
            container("w2", Some("web"), "app", State::Exited),
        ],
        ..Docker::default()
    };
    docker.rebuild();
    docker
}

#[test]
fn projects_sort_by_name_with_other_containers_last() {
    let docker = docker();
    let labels: Vec<String> = docker
        .rows
        .iter()
        .map(|row| match *row {
            DockerRow::Group(i) => docker.groups[i].project.clone().unwrap_or_else(|| "-".into()),
            DockerRow::Container(i) => format!("  {}", docker.containers[i].label()),
        })
        .collect();
    assert_eq!(labels, ["api", "  db", "web", "  app", "  worker", "-", "  scratch"]);
    assert_eq!(docker.groups[1].running, 1);
}

#[test]
fn the_selection_survives_refreshes_and_folding() {
    let now = Instant::now();
    let mut docker = docker();
    docker.select(4, now);
    assert_eq!(docker.container().map(|c| c.id.as_str()), Some("w1"));
    docker.containers.remove(0);
    docker.rebuild();
    assert_eq!(docker.container().map(|c| c.id.as_str()), Some("w1"), "rows above changed");
    docker.collapsed.insert(Some("web".into()));
    docker.rebuild();
    assert_eq!(docker.row(), Some(DockerRow::Group(1)), "a folded project keeps the cursor on itself");
    docker.filter = "work".into();
    docker.rebuild();
    assert!(docker.rows.contains(&DockerRow::Container(0)), "filtering unfolds matches");
    assert_eq!(docker.rows.len(), 2);
}

#[test]
fn project_actions_target_only_the_containers_they_change() {
    let now = Instant::now();
    let mut docker = docker();
    docker.select(2, now);
    assert_eq!(targets(&docker, Action::Start), ["w2"]);
    assert_eq!(targets(&docker, Action::Stop), ["w1"]);
    docker.working.push(Working { id: 9, action: Action::Stop, ids: vec!["w1".into()] });
    assert!(targets(&docker, Action::Stop).is_empty(), "a container already stopping is left alone");
    docker.select(6, now);
    assert!(targets(&docker, Action::Restart).is_empty());
    assert_eq!(pause_toggle(&docker), Action::Pause);
}

#[test]
fn stats_keep_a_minute_of_history() {
    let mut docker = docker();
    for second in 0..(HISTORY as u32 + 5) {
        docker.sample(vec![("w1".into(), Stats { cpu: second, memory: 1024, ..Stats::default() })]);
    }
    let usage = &docker.usage["w1"];
    assert_eq!(usage.cpu.len(), HISTORY);
    assert_eq!(usage.latest.cpu, HISTORY as u32 + 4);
    docker.select(4, Instant::now());
    assert_eq!(docker.group_usage(docker.group().unwrap()), (u64::from(HISTORY as u32 + 4), 1024));
}
