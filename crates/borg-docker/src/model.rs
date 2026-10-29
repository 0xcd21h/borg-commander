use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

/// `docker ps` output as one JSON object per container, with the Compose labels that
/// group containers into projects.
pub(crate) const LIST_FORMAT: &str = concat!(
    r#"{"id":{{json .ID}},"name":{{json .Names}},"image":{{json .Image}},"command":{{json .Command}},"#,
    r#""created":{{json .CreatedAt}},"state":{{json .State}},"status":{{json .Status}},"ports":{{json .Ports}},"#,
    r#""networks":{{json .Networks}},"project":{{json (.Label "com.docker.compose.project")}},"#,
    r#""service":{{json (.Label "com.docker.compose.service")}},"#,
    r#""dir":{{json (.Label "com.docker.compose.project.working_dir")}},"#,
    r#""files":{{json (.Label "com.docker.compose.project.config_files")}}}"#,
);

/// The parts of `docker inspect` the details pane shows. Environment variables are left
/// out on purpose: they often hold secrets.
pub(crate) const INSPECT_FORMAT: &str = concat!(
    r#"{"started":{{json .State.StartedAt}},"finished":{{json .State.FinishedAt}},"#,
    r#""exit":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},"error":{{json .State.Error}},"#,
    r#""health":{{if .State.Health}}{{json .State.Health.Status}}{{else}}null{{end}},"#,
    r#""restarts":{{json .RestartCount}},"policy":{{json .HostConfig.RestartPolicy.Name}},"#,
    r#""entrypoint":{{json .Config.Entrypoint}},"cmd":{{json .Config.Cmd}},"#,
    r#""workdir":{{json .Config.WorkingDir}},"platform":{{json .Platform}},"#,
    r#""mounts":{{json .Mounts}},"networks":{{json .NetworkSettings.Networks}}}"#,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Running,
    Paused,
    Restarting,
    Created,
    Exited,
    Removing,
    Dead,
    Unknown,
}

impl State {
    fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "created" => Self::Created,
            "exited" => Self::Exited,
            "removing" => Self::Removing,
            "dead" => Self::Dead,
            _ => Self::Unknown,
        }
    }

    /// Whether the container has processes, so it can be stopped or restarted.
    pub fn is_up(self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Restarting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Starting,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: String,
    pub created: String,
    pub state: State,
    /// Docker's own summary, such as "Up 2 hours (healthy)" or "Exited (0) 3 days ago".
    pub status: String,
    pub ports: String,
    pub networks: String,
    pub project: Option<String>,
    pub service: Option<String>,
    pub project_dir: Option<String>,
    pub config_files: Vec<String>,
}

impl Container {
    /// The service name inside a Compose project, otherwise the container name.
    pub fn label(&self) -> &str {
        self.service.as_deref().unwrap_or(&self.name)
    }

    pub fn health(&self) -> Option<Health> {
        if self.status.contains("(healthy)") {
            Some(Health::Healthy)
        } else if self.status.contains("(unhealthy)") {
            Some(Health::Unhealthy)
        } else if self.status.contains("(health: starting)") {
            Some(Health::Starting)
        } else {
            None
        }
    }

    /// The exit code of a stopped container, from a status such as "Exited (137) 1 hour ago".
    pub fn exit_code(&self) -> Option<i32> {
        let rest = self.status.strip_prefix("Exited (")?;
        rest.split(')').next()?.parse().ok()
    }
}

#[derive(Deserialize)]
struct ListRow {
    id: String,
    name: String,
    image: String,
    command: String,
    created: String,
    state: String,
    status: String,
    ports: String,
    networks: String,
    project: String,
    service: String,
    dir: String,
    files: String,
}

fn present(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

pub(crate) fn parse_list(text: &str) -> Result<Vec<Container>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: ListRow = serde_json::from_str(line).context("reading docker ps output")?;
            Ok(Container {
                id: row.id,
                name: row.name.trim_start_matches('/').to_owned(),
                image: row.image,
                command: row.command.trim_matches('"').to_owned(),
                created: row.created,
                state: State::parse(&row.state),
                status: row.status,
                ports: row.ports,
                networks: row.networks,
                project: present(row.project),
                service: present(row.service),
                project_dir: present(row.dir),
                config_files: row.files.split(',').filter(|f| !f.is_empty()).map(str::to_owned).collect(),
            })
        })
        .collect()
}

/// A container's resource use from one `docker stats` sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Hundredths of a percent of one CPU, so 250% means two and a half cores.
    pub cpu: u32,
    pub memory: u64,
    pub memory_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub read: u64,
    pub written: u64,
    pub pids: u32,
}

/// A size as `docker stats` prints it: "0B", "804KiB", "144.8MiB", or "723MB".
pub fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim();
    let split = value.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale = match unit {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * scale).round() as u64)
}

fn pair(value: &str) -> (u64, u64) {
    let mut sides = value.split(" / ").map(|side| parse_size(side).unwrap_or(0));
    (sides.next().unwrap_or(0), sides.next().unwrap_or(0))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatsRow {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "CPUPerc")]
    cpu_perc: String,
    mem_usage: String,
    #[serde(rename = "NetIO")]
    net_io: String,
    #[serde(rename = "BlockIO")]
    block_io: String,
    #[serde(rename = "PIDs")]
    pids: String,
}

fn parse_stats(line: &str) -> Option<(String, Stats)> {
    let row: StatsRow = serde_json::from_str(line).ok()?;
    let cpu = row.cpu_perc.trim_end_matches('%').parse::<f64>().map_or(0, |p| (p * 100.0).round() as u32);
    let (memory, memory_limit) = pair(&row.mem_usage);
    let (net_rx, net_tx) = pair(&row.net_io);
    let (read, written) = pair(&row.block_io);
    let pids = row.pids.parse().unwrap_or(0);
    Some((row.id, Stats { cpu, memory, memory_limit, net_rx, net_tx, read, written, pids }))
}

/// Splits the endless output of `docker stats` into samples. Each sample starts by moving
/// the cursor home (`ESC [ H`), so a sample is complete when the next one begins, and each
/// line ends by erasing the rest of the terminal line (`ESC [ K`). Docker redraws twice
/// for each new measurement, so a sample that repeats the previous one is dropped.
#[derive(Debug, Default)]
pub struct Frames {
    current: Vec<(String, Stats)>,
    last: Option<Vec<(String, Stats)>>,
    started: bool,
}

impl Frames {
    /// Reads one output line and returns the sample it completes, if any. A sample with no
    /// running containers is empty.
    pub fn push(&mut self, line: &str) -> Option<Vec<(String, Stats)>> {
        let finished = if line.contains("\x1b[H") {
            let finished = self.started.then(|| std::mem::take(&mut self.current));
            self.started = true;
            finished.filter(|frame| self.last.as_ref() != Some(frame)).inspect(|frame| self.last = Some(frame.clone()))
        } else {
            None
        };
        let json = line.find('{').zip(line.rfind('}')).and_then(|(start, end)| line.get(start..=end));
        if let Some(stats) = json.and_then(parse_stats) {
            self.current.push(stats);
        }
        finished
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub kind: String,
    /// The volume name for volumes, otherwise the host path.
    pub source: String,
    pub destination: String,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub started: Option<String>,
    pub finished: Option<String>,
    pub exit_code: i64,
    pub oom_killed: bool,
    pub error: String,
    pub health: Option<String>,
    pub restarts: u64,
    pub restart_policy: String,
    pub command: String,
    pub workdir: String,
    pub platform: String,
    pub mounts: Vec<Mount>,
    pub networks: Vec<Network>,
}

/// Docker reports a time that never happened as the year 1.
fn moment(value: &Value) -> Option<String> {
    value.as_str().filter(|time| !time.starts_with("0001-")).map(str::to_owned)
}

fn words(value: &Value) -> Vec<String> {
    value.as_array().map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(quote).collect())
}

fn quote(word: &str) -> String {
    if word.is_empty() || word.contains(char::is_whitespace) {
        format!("'{}'", word.replace('\'', r"'\''"))
    } else {
        word.to_owned()
    }
}

pub(crate) fn parse_details(text: &str) -> Result<Details> {
    let value: Value = serde_json::from_str(text.trim()).context("reading docker inspect output")?;
    let string = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    let mounts = value["mounts"].as_array().map_or_else(Vec::new, |mounts| {
        mounts
            .iter()
            .map(|mount| {
                let kind = mount["Type"].as_str().unwrap_or_default();
                let source = if kind == "volume" { &mount["Name"] } else { &mount["Source"] };
                Mount {
                    kind: kind.to_owned(),
                    source: source.as_str().unwrap_or_default().to_owned(),
                    destination: mount["Destination"].as_str().unwrap_or_default().to_owned(),
                    writable: mount["RW"].as_bool().unwrap_or(false),
                }
            })
            .collect()
    });
    let mut networks: Vec<Network> = value["networks"].as_object().map_or_else(Vec::new, |networks| {
        networks
            .iter()
            .map(|(name, network)| Network {
                name: name.clone(),
                address: network["IPAddress"].as_str().unwrap_or_default().to_owned(),
            })
            .collect()
    });
    networks.sort_by(|a, b| a.name.cmp(&b.name));
    let command: Vec<String> = words(&value["entrypoint"]).into_iter().chain(words(&value["cmd"])).collect();
    Ok(Details {
        started: moment(&value["started"]),
        finished: moment(&value["finished"]),
        exit_code: value["exit"].as_i64().unwrap_or(0),
        oom_killed: value["oom"].as_bool().unwrap_or(false),
        error: string("error"),
        health: value["health"].as_str().map(str::to_owned),
        restarts: value["restarts"].as_u64().unwrap_or(0),
        restart_policy: string("policy"),
        command: command.join(" "),
        workdir: string("workdir"),
        platform: string("platform"),
        mounts,
        networks,
    })
}

/// Lists a folder inside a container with one `stat` call: mode in hex, size, modification
/// time and name per line, then a `/` line and the names of links that lead to folders.
/// Names cannot contain `/`, so it separates the fields. Works with GNU and BusyBox `stat`.
pub(crate) const LIST_FILES: &str = r#"cd -- "$1" || exit 1
stat -c '%f/%s/%Y/%n' -- .[!.]* ..?* * 2>/dev/null
echo /
for f in .[!.]* ..?* *; do if [ -L "$f" ] && [ -d "$f" ]; then echo "$f"; fi; done"#;

/// Size, modification time and path of each argument that exists, one per line; missing
/// paths print nothing. Paths are absolute, so the path is everything after the second `/`.
pub(crate) const STAT_PATHS: &str =
    r#"for p; do if [ -e "$p" ] || [ -L "$p" ]; then stat -c '%s/%Y/%n' -- "$p"; fi; done"#;

pub(crate) fn parse_stat(text: &str) -> std::collections::HashMap<std::path::PathBuf, (u64, i64)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '/');
            let size = fields.next()?.parse().ok()?;
            let modified = fields.next()?.parse().ok()?;
            Some((std::path::PathBuf::from(fields.next()?), (size, modified)))
        })
        .collect()
}

const TYPE_MASK: u32 = 0o170_000;
const DIR: u32 = 0o040_000;
const LINK: u32 = 0o120_000;
const FILE: u32 = 0o100_000;
const EXECUTE: u32 = 0o111;

/// An entry of a folder inside a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub mode: u32,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: i64,
    /// For a link: whether it leads to a folder.
    pub to_dir: bool,
}

impl File {
    pub fn is_dir(&self) -> bool {
        self.mode & TYPE_MASK == DIR
    }

    pub fn is_link(&self) -> bool {
        self.mode & TYPE_MASK == LINK
    }

    pub fn is_file(&self) -> bool {
        self.mode & TYPE_MASK == FILE
    }

    pub fn executable(&self) -> bool {
        self.is_file() && self.mode & EXECUTE != 0
    }
}

pub(crate) fn parse_files(text: &str) -> Vec<File> {
    let (entries, links) = text.split_once("\n/\n").unwrap_or((text, ""));
    let to_dir: std::collections::HashSet<&str> = links.lines().collect();
    entries
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '/');
            let mode = u32::from_str_radix(fields.next()?, 16).ok()?;
            let size = fields.next()?.parse().ok()?;
            let modified = fields.next()?.parse().ok()?;
            let name = fields.next()?.to_owned();
            let to_dir = to_dir.contains(name.as_str());
            Some(File { name, mode, size, modified, to_dir })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
}

impl Action {
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
        }
    }

    /// Whether the action applies to a container in `state`, so a project action only
    /// touches the containers it changes.
    pub fn applies_to(self, state: State) -> bool {
        match self {
            Self::Start => !state.is_up(),
            Self::Stop | Self::Restart => state.is_up(),
            Self::Pause => matches!(state, State::Running),
            Self::Unpause => matches!(state, State::Paused),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = r#"{"id":"a16b","name":"shop-db-1","image":"postgres:16","command":"\"docker-entrypoint.sh postgres\"","created":"2026-10-22 13:49:07 +0000 UTC","state":"running","status":"Up 44 minutes (healthy)","ports":"127.0.0.1:5432->5432/tcp","networks":"shop_default","project":"shop","service":"db","dir":"/srv/shop","files":"/srv/shop/compose.yaml,/srv/shop/compose.override.yaml"}
{"id":"7a81","name":"scratch","image":"ubuntu:24.04","command":"\"sleep infinity\"","created":"2026-10-22 13:44:12 +0000 UTC","state":"exited","status":"Exited (137) 5 minutes ago","ports":"","networks":"bridge","project":"","service":"","dir":"","files":""}
"#;

    #[test]
    fn containers_keep_their_project_labels_and_status() {
        let containers = parse_list(PS).unwrap();
        assert_eq!(containers.len(), 2);
        let db = &containers[0];
        assert_eq!((db.label(), db.state, db.health()), ("db", State::Running, Some(Health::Healthy)));
        assert_eq!(db.command, "docker-entrypoint.sh postgres");
        assert_eq!(db.config_files, ["/srv/shop/compose.yaml", "/srv/shop/compose.override.yaml"]);
        let scratch = &containers[1];
        assert_eq!((scratch.label(), scratch.project.as_deref(), scratch.exit_code()), ("scratch", None, Some(137)));
        assert!(!scratch.state.is_up() && Action::Start.applies_to(scratch.state));
        assert!(!Action::Stop.applies_to(scratch.state));
    }

    #[test]
    fn sizes_use_decimal_and_binary_units() {
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("804KiB"), Some(804 * 1024));
        assert_eq!(parse_size("144.8MiB"), Some((144.8 * 1024.0 * 1024.0_f64).round() as u64));
        assert_eq!(parse_size("723MB"), Some(723_000_000));
        assert_eq!(parse_size("1.2kB"), Some(1200));
        assert_eq!(parse_size("--"), None);
    }

    #[test]
    fn the_stats_stream_splits_into_samples_at_each_cursor_home() {
        let first = r#"{"BlockIO":"723MB / 447MB","CPUPerc":"250.35%","Container":"a16b","ID":"a16b","MemPerc":"0.80%","MemUsage":"144.8MiB / 7.75GiB","Name":"shop-db-1","NetIO":"244MB / 253MB","PIDs":"6"}"#;
        let second = r#"{"BlockIO":"0B / 0B","CPUPerc":"0.00%","Container":"7a81","ID":"7a81","MemPerc":"0.00%","MemUsage":"804KiB / 7.75GiB","Name":"x","NetIO":"1kB / 0B","PIDs":"2"}"#;
        let mut frames = Frames::default();
        assert_eq!(frames.push(&format!("\x1b[H{first}\x1b[K")), None);
        assert_eq!(frames.push(&format!("{second}\x1b[K")), None);
        assert_eq!(frames.push("\x1b[K"), None);
        let sample = frames.push(&format!("\x1b[J\x1b[H{first}")).expect("a complete sample");
        assert_eq!(sample.len(), 2);
        let (id, stats) = &sample[0];
        assert_eq!(id, "a16b");
        assert_eq!((stats.cpu, stats.pids, stats.net_rx, stats.written), (25_035, 6, 244_000_000, 447_000_000));
        assert_eq!(stats.memory_limit, (7.75 * 1024.0 * 1024.0 * 1024.0_f64).round() as u64);
        assert_eq!(frames.push(&format!("\x1b[J\x1b[H{first}")).map(|s| s.len()), Some(1));
        assert_eq!(frames.push("\x1b[J\x1b[H"), None, "a repeated sample is dropped");
        assert_eq!(frames.push("\x1b[J\x1b[H"), Some(Vec::new()), "no running containers is still a sample");
    }

    #[test]
    fn stat_reads_absolute_paths_after_the_size_and_time() {
        let found = parse_stat("12/1729600001//etc/motd\n4096/1729600000//srv/my dir\n");
        assert_eq!(found[std::path::Path::new("/etc/motd")], (12, 1_729_600_001));
        assert_eq!(found[std::path::Path::new("/srv/my dir")], (4096, 1_729_600_000));
    }

    #[test]
    fn container_folders_list_files_folders_and_links() {
        let text = "41ed/4096/1729600000/etc\n81a4/12/1729600001/.env\n81ed/900/1729600002/run.sh\na1ff/7/1729600003/data\na1ff/9/1729600004/log\n/\ndata\n";
        let files = parse_files(text);
        assert_eq!(files.len(), 5);
        assert!(files[0].is_dir() && !files[0].executable());
        assert!(files[1].is_file() && !files[1].executable() && files[1].size == 12);
        assert!(files[2].executable());
        assert!(files[3].is_link() && files[3].to_dir);
        assert!(files[4].is_link() && !files[4].to_dir);
        assert!(parse_files("/\n").is_empty(), "an empty folder");
    }

    #[test]
    fn details_leave_out_unset_times_and_quote_arguments() {
        let text = r#"{"started":"2026-10-22T13:49:07.966Z","finished":"0001-01-01T00:00:00Z","exit":0,"oom":false,"error":"","health":"healthy","restarts":2,"policy":"unless-stopped","entrypoint":["docker-entrypoint.sh"],"cmd":["sh","-c","echo hi"],"workdir":"","platform":"linux","mounts":[{"Type":"volume","Name":"shop_data","Source":"/var/lib/docker/volumes/shop_data/_data","Destination":"/data","RW":true},{"Type":"bind","Source":"/srv/shop/init.sql","Destination":"/init.sql","RW":false}],"networks":{"shop_default":{"IPAddress":"172.18.0.2"}}}"#;
        let details = parse_details(text).unwrap();
        assert_eq!(details.started.as_deref(), Some("2026-10-22T13:49:07.966Z"));
        assert_eq!(details.finished, None);
        assert_eq!(details.command, "docker-entrypoint.sh sh -c 'echo hi'");
        assert_eq!((details.restarts, details.health.as_deref()), (2, Some("healthy")));
        assert_eq!(details.mounts[0].source, "shop_data");
        assert!(!details.mounts[1].writable);
        assert_eq!(details.networks, [Network { name: "shop_default".into(), address: "172.18.0.2".into() }]);
        let none = parse_details(r#"{"health":null,"entrypoint":null,"cmd":null,"mounts":null,"networks":null}"#);
        assert_eq!(none.unwrap().command, "");
    }
}
