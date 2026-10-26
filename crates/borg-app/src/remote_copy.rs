use std::{
    fs::{self, File, ReadDir},
    io::{Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use borg_core::RemoteMount;
use borg_i18n::tr;
use borg_jobs::{Ask, Control, Decision, Event, Outcome, Progress, Totals};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    agent::{Call, Reply},
    link,
    session::StateDir,
};

const CHUNK: usize = link::CHUNK as usize;
const PAGE: usize = 256;
const DEADLINE: Duration = Duration::from_secs(30);
const CANCEL_POLL: Duration = Duration::from_millis(50);

fn wire(path: &Path) -> String {
    STANDARD.encode(path.as_os_str().as_bytes())
}

fn path(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(std::ffi::OsString::from_vec(STANDARD.decode(value)?));
    if !path.is_absolute() {
        bail!("Transfer paths must be absolute");
    }
    Ok(path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Kind {
    File,
    Directory,
    Symlink,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Node {
    kind: Kind,
    size: u64,
    mode: u32,
    modified: Option<(u64, u32)>,
    link: Option<String>,
}

fn node(path: &Path) -> Result<Option<Node>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let kind = if meta.is_symlink() {
        Kind::Symlink
    } else if meta.is_dir() {
        Kind::Directory
    } else if meta.is_file() {
        Kind::File
    } else {
        bail!(tr!(remote_copy_special_file), path = path.display());
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| (time.as_secs(), time.subsec_nanos()));
    Ok(Some(Node {
        kind,
        size: if kind == Kind::File { meta.len() } else { 0 },
        mode: meta.mode() & 0o777,
        modified,
        link: if kind == Kind::Symlink { Some(wire(&fs::read_link(path)?)) } else { None },
    }))
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    Canonical { path: String },
    Stat { path: String },
    List { path: Option<String> },
    Open { path: String },
    Read,
    Begin { path: String, size: u64, overwrite: bool },
    Write { data: String },
    Finish { node: Node },
    Abort,
    Mkdir { path: String },
    Metadata { path: String, node: Node },
    Symlink { path: String, target: String, overwrite: bool },
}

struct Incoming {
    file: tempfile::NamedTempFile,
    target: PathBuf,
    size: u64,
    written: u64,
    overwrite: bool,
}

#[derive(Default)]
pub struct Server {
    incoming: Option<Incoming>,
    outgoing: Option<File>,
    listing: Option<ReadDir>,
}

fn metadata(file: &File, node: &Node) -> Result<()> {
    if let Some((seconds, nanos)) = node.modified {
        file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::new(seconds, nanos)))?;
    }
    file.set_permissions(fs::Permissions::from_mode(node.mode & 0o777))?;
    Ok(())
}

impl Server {
    pub async fn handle(mut self, args: Value) -> (Self, Reply) {
        tokio::task::spawn_blocking(move || {
            let result = serde_json::from_value(args).map_err(anyhow::Error::from).and_then(|op| self.apply(op));
            let reply = match result {
                Ok(value) => Reply::ok(value.to_string()),
                Err(error) => Reply::error(format!("{error:#}")),
            };
            (self, reply)
        })
        .await
        .expect("transfer worker")
    }

    fn apply(&mut self, op: Op) -> Result<Value> {
        match op {
            Op::Canonical { path: text } => Ok(json!(wire(&fs::canonicalize(path(&text)?)?))),
            Op::Stat { path: text } => Ok(serde_json::to_value(node(&path(&text)?)?)?),
            Op::List { path: text } => {
                if let Some(text) = text {
                    self.listing = Some(fs::read_dir(path(&text)?)?);
                }
                let listing = self.listing.as_mut().context("No open directory")?;
                let names = listing
                    .take(PAGE)
                    .map(|entry| {
                        let entry = entry?;
                        let meta = node(&entry.path())?.context(tr!(remote_copy_entry_gone))?;
                        Ok((wire(Path::new(&entry.file_name())), meta))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let more = names.len() == PAGE;
                if !more {
                    self.listing = None;
                }
                Ok(json!({"names":names,"more":more}))
            }
            Op::Open { path: text } => {
                let file = File::open(path(&text)?)?;
                if !file.metadata()?.is_file() {
                    bail!("Source is not a regular file");
                }
                self.outgoing = Some(file);
                Ok(Value::Null)
            }
            Op::Read => {
                let mut data = vec![0; CHUNK];
                let count = self.outgoing.as_mut().context("No open source file")?.read(&mut data)?;
                if count == 0 {
                    self.outgoing = None;
                }
                Ok(json!(STANDARD.encode(&data[..count])))
            }
            Op::Begin { path: text, size, overwrite } => {
                self.incoming = None;
                let target = path(&text)?;
                let parent = target.parent().context("Destination has no parent folder")?;
                let file = tempfile::Builder::new().prefix(".borg-copy-").tempfile_in(parent)?;
                self.incoming = Some(Incoming { file, target, size, written: 0, overwrite });
                Ok(Value::Null)
            }
            Op::Write { data } => {
                if data.len() > CHUNK.div_ceil(3) * 4 {
                    bail!("Transfer chunk is too large");
                }
                let data = STANDARD.decode(data)?;
                let incoming = self.incoming.as_mut().context("No open destination file")?;
                if data.len() as u64 > incoming.size.saturating_sub(incoming.written) {
                    bail!(tr!(remote_copy_source_changed));
                }
                incoming.file.write_all(&data)?;
                incoming.written += data.len() as u64;
                Ok(Value::Null)
            }
            Op::Finish { node } => {
                let incoming = self.incoming.take().context("No open destination file")?;
                if incoming.written != incoming.size {
                    bail!(tr!(remote_copy_source_changed));
                }
                metadata(incoming.file.as_file(), &node)?;
                incoming.file.as_file().sync_all()?;
                if incoming.overwrite {
                    incoming.file.persist(&incoming.target).map_err(|error| error.error)?;
                } else {
                    incoming.file.persist_noclobber(&incoming.target).map_err(|error| error.error)?;
                }
                Ok(Value::Null)
            }
            Op::Abort => {
                self.incoming = None;
                self.outgoing = None;
                Ok(Value::Null)
            }
            Op::Mkdir { path: text } => {
                let target = path(&text)?;
                match fs::create_dir(&target) {
                    Ok(()) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AlreadyExists
                            && node(&target)?.is_some_and(|node| node.kind == Kind::Directory) => {}
                    Err(error) => return Err(error.into()),
                }
                Ok(Value::Null)
            }
            Op::Metadata { path: text, node } => {
                let target = path(&text)?;
                if fs::symlink_metadata(&target)?.is_symlink() {
                    bail!(tr!(remote_copy_directory_changed));
                }
                metadata(&File::open(target)?, &node)?;
                Ok(Value::Null)
            }
            Op::Symlink { path: text, target, overwrite } => {
                let into = path(&text)?;
                let target = PathBuf::from(std::ffi::OsString::from_vec(STANDARD.decode(target)?));
                let staging = tempfile::Builder::new()
                    .prefix(".borg-copy-")
                    .tempdir_in(into.parent().context("No parent folder")?)?;
                let staged = staging.path().join("link");
                std::os::unix::fs::symlink(target, &staged)?;
                if overwrite {
                    fs::rename(staged, into)?;
                } else {
                    fs::hard_link(staged, into)?;
                }
                Ok(Value::Null)
            }
        }
    }
}

#[derive(Debug)]
struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Copy cancelled")
    }
}
impl std::error::Error for Cancelled {}

struct Endpoint<'a> {
    local: Server,
    remote: Option<link::Remote>,
    session: Option<String>,
    server: Option<link::Server>,
    runtime: &'a tokio::runtime::Handle,
    control: &'a Control,
}

impl<'a> Endpoint<'a> {
    fn open(mount: Option<RemoteMount>, runtime: &'a tokio::runtime::Handle, control: &'a Control) -> Result<Self> {
        let mut endpoint =
            Self { local: Server::default(), remote: None, session: None, server: None, runtime, control };
        if let Some(mount) = mount {
            let state = StateDir::locate().context("Borg has no state folder")?;
            let server = link::server(state.path(), &mount.machine)?;
            endpoint.remote = Some(runtime.block_on(async {
                tokio::time::timeout(DEADLINE, link::Remote::open(state.path(), &server)).await
            })??);
            endpoint.server = Some(server);
            endpoint.session = Some(mount.session);
        }
        Ok(endpoint)
    }

    fn call(&mut self, op: Op) -> Result<Value> {
        if !self.control.proceed() {
            return Err(Cancelled.into());
        }
        let Some(server) = &self.server else { return self.local.apply(op) };
        if self.remote.is_none() {
            if matches!(op, Op::Abort) {
                return Ok(Value::Null);
            }
            let state = StateDir::locate().context("Borg has no state folder")?;
            self.remote = Some(self.runtime.block_on(async {
                tokio::time::timeout(DEADLINE, link::Remote::open(state.path(), server)).await
            })??);
        }
        let remote = self.remote.as_mut().context("Transfer connection closed")?;
        let mut args = serde_json::to_value(op)?;
        args["session"] = json!(self.session);
        let call = Call::new("file_transfer", args);
        let reply = self.runtime.block_on(async {
            let future = tokio::time::timeout(DEADLINE, remote.call(&call));
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result = &mut future => break result.context(tr!(remote_copy_timeout))?,
                    _ = tokio::time::sleep(CANCEL_POLL) => if self.control.cancelled() { break Err(Cancelled.into()); }
                }
            }
        });
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.remote = None;
                return Err(error);
            }
        };
        if !reply.ok {
            if reply.text.contains("unknown tool") {
                bail!(tr!(remote_copy_upgrade));
            }
            bail!("{}", reply.text);
        }
        Ok(serde_json::from_str(&reply.text)?)
    }

    fn stat(&mut self, target: &Path) -> Result<Option<Node>> {
        Ok(serde_json::from_value(self.call(Op::Stat { path: wire(target) })?)?)
    }

    fn children(&mut self, dir: &Path) -> Result<Vec<(PathBuf, Node)>> {
        let mut children = Vec::new();
        let mut request = Some(wire(dir));
        loop {
            let page = self.call(Op::List { path: request.take() })?;
            for entry in page["names"].as_array().context("Missing directory entries")? {
                let name =
                    std::ffi::OsString::from_vec(STANDARD.decode(entry[0].as_str().context("Invalid filename")?)?);
                let relative = Path::new(&name);
                if relative.components().count() != 1
                    || !matches!(relative.components().next(), Some(std::path::Component::Normal(_)))
                {
                    bail!("Invalid directory entry");
                }
                let meta: Node = serde_json::from_value(entry[1].clone())?;
                children.push((dir.join(relative), meta));
            }
            if page["more"] != true {
                break;
            }
        }
        Ok(children)
    }
}

pub struct CopyJob {
    pub from: Option<RemoteMount>,
    pub to: Option<RemoteMount>,
    pub sources: Vec<PathBuf>,
    pub target: PathBuf,
}

struct Item {
    source: PathBuf,
    target: PathBuf,
    node: Node,
    finish: bool,
}

struct Copy<'a> {
    from: Endpoint<'a>,
    to: Endpoint<'a>,
    control: &'a Control,
    emit: &'a mut dyn FnMut(Event),
    done: Totals,
    total: Totals,
    skipped: u64,
    overwrite: Option<bool>,
    skip_errors: bool,
    existing: std::collections::HashMap<PathBuf, Node>,
    listed: std::collections::HashSet<PathBuf>,
}

impl CopyJob {
    pub fn run(self, runtime: &tokio::runtime::Handle, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
        let result = (|| {
            let from = Endpoint::open(self.from.clone(), runtime, control)?;
            let to = Endpoint::open(self.to.clone(), runtime, control)?;
            let mut copy = Copy {
                from,
                to,
                control,
                emit,
                done: Totals::default(),
                total: Totals::default(),
                skipped: 0,
                overwrite: None,
                skip_errors: false,
                existing: Default::default(),
                listed: Default::default(),
            };
            let same_host =
                self.from.as_ref().map(|mount| &mount.machine) == self.to.as_ref().map(|mount| &mount.machine);
            let result = copy.run(self.sources, self.target, same_host);
            Ok::<_, anyhow::Error>(match result {
                Ok(()) => Outcome::Done { done: copy.done, skipped: copy.skipped },
                Err(error) if error.is::<Cancelled>() => Outcome::Cancelled { done: copy.done },
                Err(error) => Outcome::Failed(format!("{error:#}")),
            })
        })();
        result.unwrap_or_else(|error| {
            if error.is::<Cancelled>() {
                Outcome::Cancelled { done: Totals::default() }
            } else {
                Outcome::Failed(format!("{error:#}"))
            }
        })
    }
}

impl Copy<'_> {
    fn check(&self) -> Result<()> {
        if self.control.proceed() { Ok(()) } else { Err(Cancelled.into()) }
    }

    fn run(&mut self, sources: Vec<PathBuf>, target: PathBuf, same_host: bool) -> Result<()> {
        let target =
            path(self.to.call(Op::Canonical { path: wire(&target) })?.as_str().context("Invalid destination path")?)?;
        if !self.to.stat(&target)?.is_some_and(|node| node.kind == Kind::Directory) {
            bail!(tr!(remote_copy_existing_folder));
        }
        let mut pending = sources
            .into_iter()
            .map(|source| {
                let into = target.join(source.file_name().unwrap_or_default());
                (source, into, false, None::<Node>)
            })
            .collect::<Vec<_>>();
        let mut items = Vec::new();
        while let Some((source, target, finish, known)) = pending.pop() {
            self.check()?;
            let root = known.is_none();
            let node = match known {
                Some(node) => node,
                None => self.from.stat(&source)?.context(tr!(remote_copy_source_gone))?,
            };
            let identity = if same_host && root {
                let resolve = if node.kind == Kind::Directory {
                    source.as_path()
                } else {
                    source.parent().context("Source has no parent")?
                };
                let canonical = path(
                    self.from.call(Op::Canonical { path: wire(resolve) })?.as_str().context("Invalid source path")?,
                )?;
                if node.kind == Kind::Directory {
                    canonical
                } else {
                    canonical.join(source.file_name().context("Source has no filename")?)
                }
            } else {
                source.clone()
            };
            if same_host
                && root
                && (target == identity || node.kind == Kind::Directory && target.starts_with(&identity))
            {
                bail!(tr!(remote_copy_same_path));
            }
            if !finish {
                self.total.files += u64::from(node.kind != Kind::Directory);
                self.total.bytes = self.total.bytes.saturating_add(node.size);
                (self.emit)(Event::Scanning(self.total));
            }
            if node.kind == Kind::Directory && !finish {
                pending.push((source.clone(), target.clone(), true, Some(node.clone())));
                items.push(Item { source: source.clone(), target: target.clone(), node: node.clone(), finish: false });
                let mut children = self.from.children(&source)?;
                children.sort_by(|a, b| a.0.cmp(&b.0));
                for (child, node) in children.into_iter().rev() {
                    let into = target.join(child.file_name().context("Source has no filename")?);
                    pending.push((child, into, false, Some(node)));
                }
            } else {
                items.push(Item { source, target, node, finish });
            }
        }
        let mut skipped_dirs = Vec::<PathBuf>::new();
        for mut item in items {
            if skipped_dirs.iter().any(|dir| item.target.starts_with(dir)) {
                continue;
            }
            self.check()?;
            let before = self.done;
            loop {
                match self.item(&mut item) {
                    Ok(()) => break,
                    Err(error) if error.is::<Cancelled>() => return Err(error),
                    Err(error) => {
                        self.done = before;
                        if let Some(parent) = item.target.parent() {
                            self.listed.remove(parent);
                        }
                        let _ = self.to.call(Op::Abort);
                        let decision = if self.skip_errors {
                            Decision::Skip
                        } else {
                            (self.emit)(Event::Asking(Ask::Failure {
                                path: item.source.clone(),
                                message: format!("{error:#}"),
                            }));
                            self.control.decide()
                        };
                        match decision {
                            Decision::Cancel => return Err(Cancelled.into()),
                            Decision::Retry => continue,
                            Decision::SkipAll => self.skip_errors = true,
                            _ => {}
                        }
                        self.skipped += 1;
                        if item.node.kind == Kind::Directory {
                            skipped_dirs.push(item.target.clone());
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn item(&mut self, item: &mut Item) -> Result<()> {
        if item.finish {
            self.to.call(Op::Metadata { path: wire(&item.target), node: item.node.clone() })?;
            return Ok(());
        }
        let existing = if item.target.parent().is_some_and(|parent| self.listed.contains(parent)) {
            self.existing.remove(&item.target)
        } else {
            self.to.stat(&item.target)?
        };
        if item.node.kind == Kind::Directory {
            self.to.call(Op::Mkdir { path: wire(&item.target) })?;
            if existing.is_some() {
                for (path, node) in self.to.children(&item.target)? {
                    self.existing.insert(path, node);
                }
            }
            self.listed.insert(item.target.clone());
            return Ok(());
        }
        let mut overwrite = false;
        if let Some(existing) = existing {
            let decision = match self.overwrite {
                Some(true) => Decision::Overwrite,
                Some(false) => Decision::Skip,
                None => {
                    (self.emit)(Event::Asking(Ask::Conflict {
                        source: item.source.clone(),
                        target: item.target.clone(),
                        sizes: (item.node.size, existing.size),
                        newer: item.node.modified > existing.modified,
                    }));
                    self.control.decide()
                }
            };
            match decision {
                Decision::Cancel => return Err(Cancelled.into()),
                Decision::Skip | Decision::SkipAll => {
                    self.skipped += 1;
                    if decision == Decision::SkipAll {
                        self.overwrite = Some(false);
                    }
                    return Ok(());
                }
                Decision::KeepBoth => {
                    let mut failure = None;
                    let target = borg_jobs::unique_by(&item.target, |candidate| match self.to.stat(candidate) {
                        Ok(node) => node.is_some(),
                        Err(error) => {
                            failure = Some(error);
                            false
                        }
                    });
                    if let Some(error) = failure {
                        return Err(error);
                    }
                    item.target = target;
                }
                Decision::OverwriteAll => {
                    self.overwrite = Some(true);
                    overwrite = true;
                }
                _ => overwrite = true,
            }
        }
        if item.node.kind == Kind::Symlink {
            self.to.call(Op::Symlink {
                path: wire(&item.target),
                target: item.node.link.clone().context("Missing symlink target")?,
                overwrite,
            })?;
        } else {
            self.from.call(Op::Open { path: wire(&item.source) })?;
            self.to.call(Op::Begin { path: wire(&item.target), size: item.node.size, overwrite })?;
            let mut received = 0;
            loop {
                let data = self.from.call(Op::Read)?;
                let data = data.as_str().context("Invalid file data")?;
                if data.is_empty() {
                    break;
                }
                let bytes = STANDARD.decode(data)?;
                self.to.call(Op::Write { data: data.into() })?;
                received += bytes.len() as u64;
                (self.emit)(Event::Progress(Progress {
                    done: Totals { bytes: self.done.bytes + received, ..self.done },
                    total: self.total,
                    current: item.source.clone(),
                }));
            }
            let after = self.from.stat(&item.source)?.context(tr!(remote_copy_source_gone))?;
            if received != item.node.size || after.size != item.node.size || after.modified != item.node.modified {
                bail!(tr!(remote_copy_source_changed));
            }
            self.to.call(Op::Finish { node: item.node.clone() })?;
        }
        self.done.files += 1;
        self.done.bytes += item.node.size;
        (self.emit)(Event::Progress(Progress { done: self.done, total: self.total, current: item.target.clone() }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: PathBuf, target: PathBuf, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        CopyJob { from: None, to: None, sources: vec![source], target }.run(runtime.handle(), control, emit)
    }

    #[test]
    fn recursive_copy_preserves_empty_folders_links_raw_names_modes_and_times() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::create_dir(&target).unwrap();
        #[cfg(target_os = "linux")]
        let raw = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
        #[cfg(not(target_os = "linux"))]
        let raw = std::ffi::OsString::from("資料");
        let file = source.join(&raw);
        fs::write(&file, b"contents").unwrap();
        let meta = File::open(&file).unwrap();
        meta.set_permissions(fs::Permissions::from_mode(0o751)).unwrap();
        meta.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::new(123456789, 123456))).unwrap();
        std::os::unix::fs::symlink(&raw, source.join("link")).unwrap();
        let outcome = run(source.clone(), target.clone(), &Control::default(), &mut |_| {});
        assert!(matches!(outcome, Outcome::Done { done: Totals { files: 2, bytes: 8 }, .. }), "{outcome:?}");
        let copied = target.join("source");
        assert!(copied.join("empty").is_dir());
        assert_eq!(fs::read(copied.join(&raw)).unwrap(), b"contents");
        assert_eq!(fs::read_link(copied.join("link")).unwrap(), PathBuf::from(raw));
        let copied = node(&copied.join(file.file_name().unwrap())).unwrap().unwrap();
        let original = node(&file).unwrap().unwrap();
        assert_eq!(copied.mode, original.mode);
        assert_eq!(copied.modified, original.modified);
    }

    #[test]
    fn conflicts_keep_both_skip_overwrite_and_cancel_without_partial_destinations() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        let source = source.join("data");
        let bytes = vec![0x93; CHUNK * 3 + 7];
        fs::write(&source, &bytes).unwrap();
        fs::write(target.join("data"), b"original").unwrap();
        let control = Control::default();
        let outcome = run(source.clone(), target.clone(), &control, &mut |event| match event {
            Event::Asking(Ask::Conflict { .. }) => control.answer(Decision::Overwrite),
            Event::Progress(progress) if progress.done.bytes > 0 => control.cancel(),
            _ => {}
        });
        assert!(matches!(outcome, Outcome::Cancelled { .. }), "{outcome:?}");
        assert_eq!(fs::read(target.join("data")).unwrap(), b"original");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
        for decision in [Decision::Skip, Decision::KeepBoth, Decision::OverwriteAll] {
            let control = Control::default();
            let outcome = run(source.clone(), target.clone(), &control, &mut |event| {
                if matches!(event, Event::Asking(Ask::Conflict { .. })) {
                    control.answer(decision);
                }
            });
            assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
            if decision == Decision::KeepBoth {
                assert_eq!(fs::read(target.join("data (2)")).unwrap(), bytes);
            }
        }
        assert_eq!(fs::read(target.join("data")).unwrap(), bytes);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
    }

    #[test]
    fn copy_refuses_descendants_and_destination_symlink_directories() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        let outcome = run(source.clone(), source.join("nested/.."), &Control::default(), &mut |_| {});
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(temp.path(), target.join("source")).unwrap();
        let control = Control::default();
        let outcome = run(source, target, &control, &mut |event| {
            if matches!(event, Event::Asking(Ask::Failure { .. })) {
                control.answer(Decision::Skip);
            }
        });
        assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }), "{outcome:?}");
        assert!(!temp.path().join("nested").exists());
    }

    #[tokio::test]
    async fn server_drops_staged_uploads_rejects_oversize_chunks_and_never_clobbers_races() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("data");
        let mut server = Server::default();
        let begin = || Op::Begin { path: wire(&target), size: 3, overwrite: false };
        server.apply(begin()).unwrap();
        assert!(server.apply(Op::Write { data: STANDARD.encode(b"toolong") }).is_err());
        server.apply(Op::Abort).unwrap();
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        server.apply(begin()).unwrap();
        server.apply(Op::Write { data: STANDARD.encode(b"new") }).unwrap();
        fs::write(&target, b"raced").unwrap();
        let meta = node(&target).unwrap().unwrap();
        assert!(server.apply(Op::Finish { node: meta }).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"raced");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        server.apply(begin()).unwrap();
        drop(server);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        let server = Server::default();
        let (_, reply) = server.handle(json!({"op":"stat","path":wire(Path::new("relative"))})).await;
        assert!(!reply.ok);
    }
}
