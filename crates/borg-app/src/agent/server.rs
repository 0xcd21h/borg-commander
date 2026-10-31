//! `borg mcp` itself: the MCP server that routes an agent's calls to a session here or to a paired borg.

use super::{
    Call, PROTOCOL, Reply, SOCKET_PREFIX, SOCKET_SUFFIX, hostname, routing_tools, socket_dir, tools, transfer_tools,
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// The newest session that answers, among the sockets in the agent folder.
pub(super) async fn connect(runtime_dir: &Path, session_id: Option<&str>) -> Result<UnixStream> {
    let dir = socket_dir(runtime_dir)?;
    let mut sockets: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if let Some(id) = session_id {
                name.starts_with(&format!("{SOCKET_PREFIX}{id}-")) && name.ends_with(SOCKET_SUFFIX)
            } else {
                name.starts_with(SOCKET_PREFIX) && name.ends_with(SOCKET_SUFFIX)
            }
        })
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    sockets.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, socket) in sockets {
        if let Ok(stream) = UnixStream::connect(&socket).await {
            return Ok(stream);
        }
    }
    bail!("no session")
}

pub(crate) struct Session {
    pub(super) lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    pub(super) write: tokio::net::unix::OwnedWriteHalf,
}

impl Session {
    pub(crate) async fn open(runtime_dir: &Path, session_id: Option<&str>) -> Option<Self> {
        let (read, write) = connect(runtime_dir, session_id).await.ok()?.into_split();
        Some(Self { lines: BufReader::new(read).lines(), write })
    }

    pub(crate) async fn call(&mut self, call: &Call) -> Option<Reply> {
        self.write.write_all(format!("{}\n", serde_json::to_string(call).ok()?).as_bytes()).await.ok()?;
        serde_json::from_str(&self.lines.next_line().await.ok()??).ok()
    }
}

/// Where `borg mcp` sends the calls it gets: the borg open on this machine, or a paired
/// borg reached through borg cloud.
pub(super) enum Backend {
    Local { runtime_dir: PathBuf, session_id: String, session: Option<Session> },
    Remote { state: PathBuf, server: crate::link::Server, remote: Option<Box<crate::link::Remote>> },
    Routing(Router),
}

/// One MCP connection can address this computer or any server paired with it. Selection is
/// private to this MCP process, so two agents cannot silently redirect each other's calls.
pub(super) struct Router {
    pub(super) runtime_dir: PathBuf,
    pub(super) state: Option<PathBuf>,
    pub(super) selected: String,
    pub(super) locals: HashMap<String, Session>,
    pub(super) remotes: HashMap<String, Box<crate::link::Remote>>,
}

impl Router {
    pub(super) fn servers(&self) -> Vec<crate::link::Server> {
        self.state.as_deref().map(crate::link::servers).unwrap_or_default()
    }

    pub(super) fn local_sessions(&self) -> Vec<crate::sessions::NamedSession> {
        self.state
            .as_ref()
            .map(|state| crate::sessions::list(&crate::session::StateDir::at(state.clone())).unwrap_or_default())
            .filter(|sessions| !sessions.is_empty())
            .unwrap_or_else(|| {
                vec![crate::sessions::NamedSession {
                    id: "default".into(),
                    name: "Default".into(),
                    archived: false,
                    last_opened: 0,
                }]
            })
    }

    pub(super) fn resolve(&self, text: &str) -> Result<String, String> {
        if let Some((machine, session)) = text.strip_prefix("remote:").and_then(|rest| rest.split_once('/'))
            && !session.is_empty()
            && self.servers().iter().any(|server| server.name == machine)
        {
            return Ok(format!("remote:{machine}/{session}"));
        }
        let local_matches: Vec<_> = self
            .local_sessions()
            .into_iter()
            .filter(|session| text == format!("local:{}", session.id) || text == session.id || text == session.name)
            .collect();
        if text == "local" {
            return Ok(format!(
                "local:{}",
                self.local_sessions()
                    .into_iter()
                    .filter(|session| !session.archived)
                    .max_by_key(|session| session.last_opened)
                    .map_or_else(|| "default".into(), |session| session.id)
            ));
        }
        let servers = self.servers();
        let remote_matches: Vec<_> =
            servers.iter().filter(|server| text == format!("remote:{}", server.name) || text == server.name).collect();
        match (local_matches.as_slice(), remote_matches.as_slice()) {
            ([session], []) => Ok(format!("local:{}", session.id)),
            ([], [server]) => Ok(format!("remote:{}", server.name)),
            ([], []) => Err(format!("No session named {text}. Call list_sessions to see available targets.")),
            _ => Err(format!("{text} is ambiguous. Use the full session ID from list_sessions.")),
        }
    }

    pub(super) async fn call(&mut self, call: &Call) -> Reply {
        match call.tool.as_str() {
            "list_sessions" => {
                let mut lines: Vec<_> = self
                    .local_sessions()
                    .into_iter()
                    .map(|session| {
                        format!(
                            "local:{}  {}  {}{}",
                            session.id,
                            hostname(),
                            session.name,
                            if session.archived { " (archived)" } else { "" }
                        )
                    })
                    .collect();
                lines.extend(
                    self.servers().into_iter().map(|server| format!("remote:{}  {}", server.name, server.relay)),
                );
                return Reply::ok(lines.join("\n"));
            }
            "use_session" => {
                let Some(name) = call.args["session"].as_str() else {
                    return Reply::error("use_session needs a session ID or name from list_sessions");
                };
                return match self.resolve(name) {
                    Ok(id) => {
                        self.selected = id.clone();
                        Reply::ok(format!("Selected {id} for this MCP connection."))
                    }
                    Err(error) => Reply::error(error),
                };
            }
            "list_remote_sessions" => {
                let Some(machine) = call.args["server"].as_str() else {
                    return Reply::error("list_remote_sessions needs a paired server name");
                };
                let Some(server) = self.servers().into_iter().find(|server| server.name == machine) else {
                    return Reply::error(format!("no paired server named {machine}"));
                };
                let Some(state) = self.state.as_ref() else {
                    return Reply::error("Borg has no state folder for paired servers");
                };
                let key = format!("remote:{machine}");
                if !self.remotes.contains_key(&key) {
                    match crate::link::Remote::open(state, &server).await {
                        Ok(open) => {
                            self.remotes.insert(key.clone(), Box::new(open));
                        }
                        Err(error) => return Reply::error(format!("{error:#}")),
                    }
                }
                let reply = self
                    .remotes
                    .get_mut(&key)
                    .expect("opened remote")
                    .call(&Call::new("list_sessions", json!({})))
                    .await;
                return match reply {
                    Ok(reply) if reply.ok => Reply::ok(
                        reply
                            .text
                            .lines()
                            .map(|line| format!("remote:{machine}/{line}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    Ok(reply) => reply,
                    Err(error) => {
                        self.remotes.remove(&key);
                        Reply::error(format!("{error:#}"))
                    }
                };
            }
            _ => {}
        }
        let requested = call.args["session"].as_str().unwrap_or(&self.selected);
        let target = match self.resolve(requested) {
            Ok(id) => id,
            Err(error) => return Reply::error(error),
        };
        let mut routed = call.clone();
        if let Value::Object(args) = &mut routed.args {
            args.remove("session");
        }
        if let Some(id) = target.strip_prefix("local:") {
            let mut backend = Backend::Local {
                runtime_dir: self.runtime_dir.clone(),
                session_id: id.into(),
                session: self.locals.remove(id),
            };
            let reply = Box::pin(backend.call(&routed)).await;
            if let Backend::Local { session: Some(session), .. } = backend {
                self.locals.insert(id.into(), session);
            }
            return reply;
        }
        let Some(state) = self.state.as_ref() else {
            return Reply::error("Borg has no state folder for paired servers");
        };
        let remote_target = target.trim_start_matches("remote:");
        let (name, remote_session) =
            remote_target.split_once('/').map_or((remote_target, None), |(name, id)| (name, Some(id)));
        let Some(server) = self.servers().into_iter().find(|server| server.name == name) else {
            return Reply::error(format!("{target} is no longer paired"));
        };
        if let Some(id) = remote_session
            && let Value::Object(args) = &mut routed.args
        {
            args.insert("session".into(), json!(id));
        }
        let cache_key = format!("remote:{name}");
        let mut backend = Backend::Remote { state: state.clone(), server, remote: self.remotes.remove(&cache_key) };
        let reply = Box::pin(backend.call(&routed)).await;
        if let Backend::Remote { remote: Some(remote), .. } = backend {
            self.remotes.insert(cache_key, remote);
        }
        reply
    }
}

impl Backend {
    pub(super) fn instructions(&self) -> String {
        match self {
            Self::Local { .. } => format!(
                "Borg Commander is a terminal file manager the user has open on {}. These tools see what the user sees there and work on that machine. Every command and change waits for the user's approval in Borg, so explain what you want to do and why.",
                hostname()
            ),
            Self::Remote { server, .. } => format!(
                "Borg Commander is a terminal file manager the user has open on {}, which these tools reach through borg cloud, encrypted end to end. They see what the user sees there and work on that machine; `download` and `upload` copy files between it and this computer, where you run. Every command and change waits for the user's approval in Borg, so explain what you want to do and why.",
                server.name
            ),
            Self::Routing(_) => format!(
                "Borg Commander can work in the local session on {} or a paired remote. Call list_sessions, then use_session to choose the target for this MCP connection. Every tool also accepts an optional session ID to override the target for one call. Commands and changes wait for approval in the target Borg.",
                hostname()
            ),
        }
    }

    pub(super) fn tools(&self) -> Value {
        let mut tools = tools();
        if let (Self::Remote { .. }, Value::Array(list)) = (self, &mut tools) {
            list.extend(transfer_tools());
        }
        if let (Self::Routing(_), Value::Array(list)) = (self, &mut tools) {
            list.extend(transfer_tools());
            list.extend(routing_tools());
            for tool in list.iter_mut() {
                tool["inputSchema"]["properties"]["session"] = json!({
                    "type": "string",
                    "description": "Target ID from list_sessions. Overrides the session selected with use_session for this call."
                });
            }
        }
        tools
    }

    pub(super) async fn call(&mut self, call: &Call) -> Reply {
        match self {
            Self::Local { runtime_dir, session_id, session } => {
                if session.is_none() {
                    *session = Session::open(runtime_dir, Some(session_id)).await;
                }
                let mut reply = match session.as_mut() {
                    Some(open) => open.call(call).await,
                    None => None,
                };
                if reply.is_none() {
                    // The session may have restarted: try the newest one once more.
                    *session = Session::open(runtime_dir, Some(session_id)).await;
                    if let Some(open) = session.as_mut() {
                        reply = open.call(call).await;
                    }
                }
                reply.unwrap_or_else(|| {
                    Reply::error(format!(
                        "Borg is not open on {}. Ask the user to start borg in their session there.",
                        hostname()
                    ))
                })
            }
            Self::Remote { state, server, remote } => {
                // A session the relay dropped, after idling or a restart, is opened again once.
                for attempt in 0..2 {
                    if remote.is_none() {
                        match crate::link::Remote::open(state, server).await {
                            Ok(opened) => *remote = Some(Box::new(opened)),
                            Err(error) => return Reply::error(format!("{error:#}")),
                        }
                    }
                    let Some(open) = remote.as_mut() else { break };
                    let result = match call.tool.as_str() {
                        "download" => transfer_download(open, call).await,
                        "upload" => transfer_upload(open, call).await,
                        _ => open.call(call).await,
                    };
                    match result {
                        Ok(reply) => return reply,
                        Err(error) if attempt == 1 => return Reply::error(format!("{error:#}")),
                        Err(_) => *remote = None,
                    }
                }
                Reply::error(format!("borg on {} is not reachable", server.name))
            }
            Self::Routing(router) => router.call(call).await,
        }
    }
}

/// A path on this computer: absolute, under ~, or from the folder `borg mcp` runs in.
pub(super) fn local_path(text: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    }
}

pub(super) async fn transfer_download(remote: &mut crate::link::Remote, call: &Call) -> Result<Reply> {
    let Some(path) = call.args["path"].as_str() else { return Ok(Reply::error("download needs a path")) };
    let name = Path::new(path).file_name().map_or_else(|| "download".into(), |name| name.to_os_string());
    let mut to = match call.args["to"].as_str() {
        Some(to) => local_path(to),
        None => {
            let downloads = local_path("~/Downloads");
            if downloads.is_dir() { downloads } else { PathBuf::from(".") }
        }
    };
    if to.is_dir() {
        to = to.join(name);
    }
    match crate::link::download_in_session(remote, call.args["session"].as_str(), path, &to, |_, _| {}).await {
        Ok(size) => Ok(Reply::ok(format!("Downloaded {path} ({size} bytes) to {}", to.display()))),
        Err(error) if error.downcast_ref::<std::io::Error>().is_some() => Ok(Reply::error(format!("{error:#}"))),
        Err(error) => Err(error),
    }
}

pub(super) async fn transfer_upload(remote: &mut crate::link::Remote, call: &Call) -> Result<Reply> {
    let (Some(from), Some(to)) = (call.args["from"].as_str(), call.args["to"].as_str()) else {
        return Ok(Reply::error("upload needs from and to"));
    };
    let from = local_path(from);
    if !from.is_file() {
        return Ok(Reply::error(format!("{} is not a file on this computer", from.display())));
    }
    match crate::link::upload_in_session(remote, call.args["session"].as_str(), &from, to, |_, _| {}).await {
        Ok(size) => Ok(Reply::ok(format!("Uploaded {} ({size} bytes) to {to}", from.display()))),
        Err(error) => Ok(Reply::error(format!("{error:#}"))),
    }
}

/// `borg mcp`: one MCP server for this machine and every server paired with it.
pub async fn serve(runtime_dir: PathBuf) -> Result<()> {
    let state = crate::session::StateDir::locate().map(|dir| dir.path().to_path_buf());
    let mut selected = state
        .as_ref()
        .and_then(|root| crate::sessions::list(&crate::session::StateDir::at(root.clone())).ok())
        .and_then(|sessions| {
            sessions.into_iter().filter(|session| !session.archived).max_by_key(|session| session.last_opened)
        })
        .map_or_else(|| "local:default".to_owned(), |session| format!("local:{}", session.id));
    // Connects at once, so the selected borg shows that Claude connected before its first call.
    let mut locals = HashMap::new();
    let mut open = Session::open(&runtime_dir, selected.strip_prefix("local:")).await;
    if open.is_none() {
        // The MCP process may have a different state-folder environment from the terminal
        // that launched Borg. The runtime socket is still the authority for the live session.
        selected = "local:default".into();
        open = Session::open(&runtime_dir, Some("default")).await;
        if open.is_none() {
            open = Session::open(&runtime_dir, None).await;
        }
    }
    if let Some(session) = open {
        locals.insert(selected.trim_start_matches("local:").to_owned(), session);
    }
    serve_with(Backend::Routing(Router { runtime_dir, state, selected, locals, remotes: HashMap::new() })).await
}

/// `borg mcp remote NAME`: the same for a borg paired through borg cloud.
pub async fn serve_remote(state: PathBuf, name: &str) -> Result<()> {
    let server = crate::link::server(&state, name)?;
    serve_with(Backend::Remote { state, server, remote: None }).await
}

pub(super) async fn serve_with(mut backend: Backend) -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = message.get("id").cloned() else { continue };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "borg", "version": crate::update::VERSION },
                "instructions": backend.instructions(),
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": backend.tools() }),
            "tools/call" => {
                let call = Call::new(params["name"].as_str().unwrap_or_default(), params["arguments"].clone());
                let reply = backend.call(&call).await;
                json!({ "content": [{ "type": "text", "text": reply.text }], "isError": !reply.ok })
            }
            _ => {
                let error =
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } });
                output.write_all(format!("{error}\n").as_bytes()).await?;
                output.flush().await?;
                continue;
            }
        };
        let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        output.write_all(format!("{response}\n").as_bytes()).await?;
        output.flush().await?;
    }
    Ok(())
}

/// `borg mcp setup [host]`: how to connect Claude Code and Claude Desktop to the borg on
/// `host` over SSH, or to this machine's without one.
pub fn setup(host: Option<&str>) -> String {
    // SSH runs commands without the login shell's PATH, so the installer's folder is added;
    // BatchMode makes a missing key fail at once instead of waiting for a password.
    let remote = r#"PATH="$HOME/.local/bin:$PATH" borg mcp"#;
    let (name, command, args) = match host {
        Some(host) => (
            format!("borg-{host}"),
            "ssh".to_owned(),
            vec!["-o".into(), "BatchMode=yes".into(), host.to_owned(), remote.to_owned()],
        ),
        None => (
            "borg".to_owned(),
            std::env::current_exe().map_or_else(|_| "borg".into(), |exe| exe.display().to_string()),
            vec!["mcp".to_owned()],
        ),
    };
    let quoted: Vec<String> =
        args.iter().map(|arg| if arg.contains(' ') { format!("'{arg}'") } else { arg.clone() }).collect();
    let desktop = json!({ "mcpServers": { &name: { "command": command, "args": args } } });
    format!(
        "Claude Code:\n\n  claude mcp add {name} -- {command} {}\n\nClaude Desktop (Settings → Developer → Edit Config), merge into the file:\n\n{}\n\nThen open borg{}; Claude works in that session and asks before running anything.",
        quoted.join(" "),
        serde_json::to_string_pretty(&desktop).unwrap_or_default(),
        host.map_or_else(String::new, |host| format!(" on {host}"))
    )
}
