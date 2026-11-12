//! What the system says about a process that has just started. On Linux everything comes from
//! `/proc`, which the reader takes as a folder, so its tests run against a folder of fixtures
//! on any system.
//!
//! A process is identified by its number together with the moment it started, in clock ticks
//! since the machine booted, exactly as `/proc` reports it. Both are read again before Borg
//! acts on a process, so a number given to another process in the meantime is left alone.

use borg_core::{Ancestor, PRELOAD_VARIABLES, ProcessKey, StartedProcess, StreamKind};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// Where Linux keeps what it knows about each process.
pub const PROC: &str = "/proc";
/// Folders where the system keeps the links programs are started by, such as `awk` for `mawk`.
const LINK_FOLDERS: &[&str] = &["/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"];
/// Links followed from one to the next before Borg gives up on a path.
const LINK_HOPS: usize = 8;
/// Bytes of a program's name the system keeps as the name a process calls itself.
const NAME_BYTES: usize = 15;
/// Where the system says which account numbers it gives people.
pub const LOGIN_DEFS: &str = "/etc/login.defs";
const UID_MIN: &str = "UID_MIN";
/// Bytes of a command line or an environment Borg reads.
const BLOCK: u64 = 64 * 1024;
/// Generations of parents an alert keeps.
pub(crate) const ANCESTORS: usize = 8;
/// What a program's link says when its file has been deleted.
const DELETED: &str = " (deleted)";
/// What the links of the standard streams say when they lead to a socket or a pipe.
const SOCKET: &str = "socket:";
const PIPE: &str = "pipe:";
/// A process's standard input, output and error, as `/proc` links them.
const STREAMS: [&str; 3] = ["fd/0", "fd/1", "fd/2"];
/// Where `/proc` lists the network sockets of a process's network, each with its number in the
/// tenth column.
const NETWORK_TABLES: &[&str] = &["net/tcp", "net/tcp6", "net/udp", "net/udp6"];
const INODE_COLUMN: usize = 9;
/// How long Borg trusts what it found out about a socket. The children of a service share its
/// sockets, so one look serves them all, and the kernel takes far longer than this to give a
/// socket's number out again.
const SOCKET_MEMORY: Duration = Duration::from_secs(60);
/// Sockets Borg remembers at once.
const SOCKETS_REMEMBERED: usize = 4096;
/// Where terminals are.
const TERMINALS: &[&str] = &["/dev/pts/", "/dev/tty", "/dev/console"];
/// The fields of `stat` after the program's name: the state comes first.
const STATE: usize = 0;
const PARENT: usize = 1;
const SESSION: usize = 3;
const TERMINAL: usize = 4;
const FLAGS: usize = 6;
const STARTED_AT: usize = 19;
/// The flag the kernel sets on its own threads, which run no program.
const KERNEL_THREAD: u32 = 0x0020_0000;
/// The states of a process that has ended: a zombie waiting for its parent, and a dead one.
const ENDED: &[char] = &['Z', 'X'];

/// What `/proc/<pid>/stat` says, as far as the watcher needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// The name the process calls itself.
    pub name: String,
    /// One letter: `R` running, `S` sleeping, `T` stopped, `Z` ended and waiting for its parent.
    pub state: char,
    pub parent: u32,
    /// The number of the first process of its session.
    pub session: u32,
    /// The terminal it belongs to, as a device number; zero when it has none.
    pub terminal: u32,
    /// Clock ticks since the machine booted.
    pub started_at: u64,
    /// The kernel's flags for the process.
    pub flags: u32,
}

impl Stat {
    /// Whether it is one of the kernel's own threads, which run no program.
    pub fn kernel_thread(&self) -> bool {
        self.flags & KERNEL_THREAD != 0
    }

    pub fn ended(&self) -> bool {
        ENDED.contains(&self.state)
    }
}

/// `stat` holds the program's name in brackets, and the name may hold brackets and spaces of
/// its own, so the fields after it start at the last bracket.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_owned();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    if fields.len() <= STARTED_AT || fields.get(STATE).is_none_or(|state| state.is_empty()) {
        return None;
    }
    Some(Stat {
        name,
        state: fields[STATE].chars().next()?,
        parent: fields[PARENT].parse().ok()?,
        session: fields[SESSION].parse().ok()?,
        terminal: fields[TERMINAL].parse().ok()?,
        started_at: fields[STARTED_AT].parse().ok()?,
        flags: fields[FLAGS].parse().ok()?,
    })
}

/// The user a process runs as, from the first number on the `Uid` line of `status`.
pub fn parse_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|values| values.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// The first account number the system gives a person, from the `UID_MIN` line of `login.defs`.
pub fn parse_uid_min(text: &str) -> Option<u32> {
    text.lines().map(str::trim).filter(|line| !line.starts_with('#')).find_map(|line| {
        let mut words = line.split_whitespace();
        if words.next() == Some(UID_MIN) { words.next()?.parse().ok() } else { None }
    })
}

/// The first account number this system gives a person.
pub fn first_person() -> u32 {
    fs::read_to_string(LOGIN_DEFS).ok().as_deref().and_then(parse_uid_min).unwrap_or(borg_core::FIRST_PERSON)
}

/// The preload variables in an environment, which `/proc` gives as `name=value` with a zero
/// byte after each.
pub fn preloads(environ: &[u8]) -> Vec<(String, String)> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, value)| PRELOAD_VARIABLES.contains(name) && !value.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// What a standard stream's link leads to.
pub fn stream_kind(target: &str) -> StreamKind {
    if target.starts_with(SOCKET) {
        StreamKind::Socket
    } else if target.starts_with(PIPE) {
        StreamKind::Pipe
    } else if TERMINALS.iter().any(|terminal| target.starts_with(terminal)) {
        StreamKind::Terminal
    } else if target.starts_with('/') {
        StreamKind::File
    } else {
        StreamKind::Other
    }
}

/// The number a socket's link names: `12345` in `socket:[12345]`.
pub fn socket_inode(target: &str) -> Option<u64> {
    target.strip_prefix(SOCKET)?.strip_prefix('[')?.strip_suffix(']')?.parse().ok()
}

/// Whether a network table from `/proc`, such as `net/tcp`, lists the socket with this number.
pub fn lists_socket(table: &str, inode: u64) -> bool {
    table
        .lines()
        .skip(1)
        .any(|line| line.split_whitespace().nth(INODE_COLUMN).and_then(|field| field.parse().ok()) == Some(inode))
}

/// What Borg found out lately about sockets, by their numbers.
#[derive(Debug, Default)]
struct SocketKinds {
    kinds: BTreeMap<u64, (StreamKind, Instant)>,
}

impl SocketKinds {
    fn known(&self, inode: u64, now: Instant) -> Option<StreamKind> {
        self.kinds.get(&inode).filter(|(_, at)| now.duration_since(*at) < SOCKET_MEMORY).map(|(kind, _)| *kind)
    }

    fn remember(&mut self, inode: u64, kind: StreamKind, now: Instant) {
        if self.kinds.len() >= SOCKETS_REMEMBERED {
            self.kinds.retain(|_, (_, at)| now.duration_since(*at) < SOCKET_MEMORY);
        }
        if self.kinds.len() >= SOCKETS_REMEMBERED {
            self.kinds.clear();
        }
        self.kinds.insert(inode, (kind, now));
    }
}

/// The program a process runs, and whether its file has been deleted since it started.
pub fn program_of(link: &str) -> (PathBuf, bool) {
    match link.strip_suffix(DELETED) {
        Some(path) => (PathBuf::from(path), true),
        None => (PathBuf::from(link), false),
    }
}

/// A command line, which `/proc` gives as arguments with a zero byte after each.
pub fn command_of(cmdline: &[u8]) -> Vec<String> {
    cmdline
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect()
}

/// Reads what the system knows about processes. Everything it reads may vanish between one
/// read and the next, so every answer is optional rather than an error.
#[derive(Debug, Clone)]
pub struct Processes {
    root: PathBuf,
    sockets: Arc<Mutex<SocketKinds>>,
}

impl Default for Processes {
    fn default() -> Self {
        Self::at(PROC)
    }
}

impl Processes {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), sockets: Arc::default() }
    }

    fn folder(&self, pid: u32) -> PathBuf {
        self.root.join(pid.to_string())
    }

    fn text(&self, pid: u32, name: &str) -> Option<String> {
        fs::read_to_string(self.folder(pid).join(name)).ok()
    }

    /// At most [`BLOCK`] bytes of one of a process's files, which may be closed to Borg.
    fn block(&self, pid: u32, name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let read =
            fs::File::open(self.folder(pid).join(name)).and_then(|file| file.take(BLOCK).read_to_end(&mut bytes));
        if read.is_err() { Vec::new() } else { bytes }
    }

    fn link(&self, pid: u32, name: &str) -> Option<String> {
        fs::read_link(self.folder(pid).join(name)).ok().map(|path| path.to_string_lossy().into_owned())
    }

    pub fn stat(&self, pid: u32) -> Option<Stat> {
        self.text(pid, "stat").as_deref().and_then(parse_stat)
    }

    /// The process with this number, as it is now.
    pub fn key(&self, pid: u32) -> Option<ProcessKey> {
        Some(ProcessKey { pid, start: self.stat(pid)?.started_at })
    }

    /// Whether this is still the same process, and not another one that took its number.
    pub fn running(&self, process: ProcessKey) -> bool {
        self.key(process.pid) == Some(process)
    }

    /// The user a process runs as, from its own report of it.
    pub fn user(&self, pid: u32) -> Option<u32> {
        self.text(pid, "status").as_deref().and_then(parse_uid).or_else(|| self.owner(pid))
    }

    /// Who owns a process's folder, which is one look at the file system rather than a read.
    fn owner(&self, pid: u32) -> Option<u32> {
        fs::metadata(self.folder(pid)).ok().map(|metadata| metadata.uid())
    }

    /// The name of the terminal a process belongs to, when it has one: the first of its
    /// standard streams that leads to a terminal.
    fn terminal(&self, pid: u32, stat: &Stat) -> Option<String> {
        if stat.terminal == 0 {
            return None;
        }
        let named = STREAMS
            .into_iter()
            .filter_map(|stream| self.link(pid, stream))
            .find(|target| stream_kind(target) == StreamKind::Terminal);
        Some(named.unwrap_or_else(|| stat.terminal.to_string()))
    }

    fn stream(&self, pid: u32, stream: &str) -> StreamKind {
        let Some(target) = self.link(pid, stream) else { return StreamKind::default() };
        match socket_inode(&target) {
            Some(inode) => self.socket(pid, inode),
            None => stream_kind(&target),
        }
    }

    /// What a process's standard input, output and error lead to.
    fn streams(&self, pid: u32) -> [StreamKind; 3] {
        STREAMS.map(|stream| self.stream(pid, stream))
    }

    /// Whether a socket of a process reaches the network or stays on the machine. A socket Borg
    /// cannot look up counts as a network one, so no rule misses a remote shell for want of a
    /// read.
    fn socket(&self, pid: u32, inode: u64) -> StreamKind {
        let now = Instant::now();
        if let Some(kind) = self.sockets.lock().ok().and_then(|kinds| kinds.known(inode, now)) {
            return kind;
        }
        let mut read_any = false;
        let listed = NETWORK_TABLES.iter().filter_map(|table| self.text(pid, table)).any(|table| {
            read_any = true;
            lists_socket(&table, inode)
        });
        if !read_any {
            return StreamKind::Socket;
        }
        let kind = if listed { StreamKind::Socket } else { StreamKind::LocalSocket };
        if let Ok(mut kinds) = self.sockets.lock() {
            kinds.remember(inode, kind, now);
        }
        kind
    }

    /// One process in the chain that started another.
    fn ancestor(&self, pid: u32) -> Option<(Ancestor, u32)> {
        let stat = self.stat(pid)?;
        let program = self.link(pid, "exe").map(|link| program_of(&link).0);
        let ancestor = Ancestor {
            pid,
            name: stat.name.clone(),
            program,
            terminal: stat.terminal != 0,
            command: command_of(&self.block(pid, "cmdline")).join(" "),
            streams: self.streams(pid),
        };
        Some((ancestor, stat.parent))
    }

    /// The process that started the first one of the session a process's terminal belongs to.
    /// It says who opened the terminal: `sshd` or `login` when somebody logged in, or the
    /// program that opened it for itself.
    fn session_parent(&self, stat: &Stat) -> Option<Ancestor> {
        if stat.terminal == 0 || stat.session == 0 {
            return None;
        }
        let leader = self.stat(stat.session)?;
        self.ancestor(leader.parent).map(|(ancestor, _)| ancestor)
    }

    /// The link in a system folder, as the process sees its files, that the process was started
    /// by: one named as the process calls itself, or as its command's first word where that
    /// name agrees, which leads to its program.
    pub fn started_by_link(&self, pid: u32, name: &str, command: &[String], program: &Path) -> Option<PathBuf> {
        let root = self.folder(pid).join("root");
        let first = command.first().and_then(|word| Path::new(word).file_name()).and_then(|word| word.to_str());
        let explains =
            |candidate: &&str| *candidate == name || (name.len() == NAME_BYTES && candidate.starts_with(name));
        [name].into_iter().chain(first).filter(explains).find_map(|candidate| {
            LINK_FOLDERS.iter().map(|folder| Path::new(folder).join(candidate)).find(|link| {
                let inside = root.join(link.strip_prefix("/").unwrap_or(link));
                fs::symlink_metadata(inside).is_ok_and(|metadata| metadata.file_type().is_symlink())
                    && resolve(&root, link).as_deref() == Some(program)
            })
        })
    }

    /// Whether a process whose program Borg cannot find is on its way out: gone, ended, or so far
    /// into ending that its memory is gone, which takes its command with it.
    fn exiting(&self, pid: u32, stat: &Stat) -> bool {
        let same = self.stat(pid).is_some_and(|now| now.started_at == stat.started_at && !now.ended());
        !same || command_of(&self.block(pid, "cmdline")).is_empty()
    }

    /// Everything the watcher's rules need about a process that has just started, and about
    /// the processes that led to it. `at` is when Borg heard of it. Nothing for one of the
    /// kernel's threads, for a process that ended before Borg could read it, and for one Borg
    /// may not read, since there is nothing to judge.
    pub fn started(&self, pid: u32, at: SystemTime) -> Option<StartedProcess> {
        let stat = self.stat(pid)?;
        if stat.kernel_thread() || stat.ended() {
            return None;
        }
        let (program, program_deleted) = match fs::read_link(self.folder(pid).join("exe")) {
            Ok(link) => {
                let (path, deleted) = program_of(&link.to_string_lossy());
                (Some(path), deleted)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if self.exiting(pid, &stat) {
                    return None;
                }
                (None, false)
            }
            Err(_) => return None,
        };
        let uid = self.user(pid)?;
        let command = command_of(&self.block(pid, "cmdline"));
        let link = program.as_deref().and_then(|program| self.started_by_link(pid, &stat.name, &command, program));
        let ancestors = self.ancestors(stat.parent);
        let [input, output, errors] = self.streams(pid);
        Some(StartedProcess {
            key: ProcessKey { pid, start: stat.started_at },
            name: stat.name.clone(),
            program,
            program_deleted,
            command,
            user: crate::watch::user_name(uid),
            uid,
            terminal: self.terminal(pid, &stat),
            input,
            output,
            errors,
            session_parent: self.session_parent(&stat),
            preloads: preloads(&self.block(pid, "environ")),
            link,
            ancestors,
            at,
        })
    }

    /// A process and the ones that led to it, the process first, up to [`ANCESTORS`] of them.
    pub fn ancestors(&self, parent: u32) -> Vec<Ancestor> {
        let mut ancestors = Vec::new();
        let mut parent = parent;
        while ancestors.len() < ANCESTORS && parent > 0 {
            let Some((ancestor, grandparent)) = self.ancestor(parent) else { break };
            ancestors.push(ancestor);
            if grandparent == parent {
                break;
            }
            parent = grandparent;
        }
        ancestors
    }

    /// Whether a process runs the file a path leads to, as the process sees its files: a path
    /// from its root folder, or from its working folder when the path is relative. Nothing when
    /// either is gone.
    pub fn runs(&self, pid: u32, path: &Path) -> Option<bool> {
        let running = fs::metadata(self.folder(pid).join("exe")).ok()?;
        let (from, path) = match path.strip_prefix("/") {
            Ok(inside) => ("root", inside),
            Err(_) => ("cwd", path),
        };
        let asked = fs::metadata(self.folder(pid).join(from).join(path)).ok()?;
        Some(running.dev() == asked.dev() && running.ino() == asked.ino())
    }

    /// Every process the folder lists, by number.
    pub fn all(&self) -> Vec<u32> {
        let Ok(entries) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut pids: Vec<u32> =
            entries.filter_map(Result::ok).filter_map(|entry| entry.file_name().to_str()?.parse().ok()).collect();
        pids.sort_unstable();
        pids
    }

    /// The processes of one user, by number. Ownership of the process's folder is what the
    /// system says, and one look at it is cheap enough to do several times a second.
    pub fn of_user(&self, uid: u32) -> Vec<u32> {
        self.all().into_iter().filter(|pid| self.owner(*pid) == Some(uid)).collect()
    }

    /// Which process started which, for everything running now.
    pub fn parents(&self) -> BTreeMap<u32, u32> {
        self.all().into_iter().filter_map(|pid| self.stat(pid).map(|stat| (pid, stat.parent))).collect()
    }

    /// A process and everything below it, parents before their children, so stopping them in
    /// this order leaves nothing running that could start more.
    pub fn tree(&self, pid: u32) -> Vec<u32> {
        descendants(&self.parents(), pid)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Where a path leads inside a process's root folder, following links from one to the next. A
/// link that points to an absolute path points inside that root, as the process sees it.
fn resolve(root: &Path, path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..LINK_HOPS {
        let inside = root.join(current.strip_prefix("/").ok()?);
        match fs::read_link(inside) {
            Ok(target) if target.is_absolute() => current = normalized(&target),
            Ok(target) => current = normalized(&current.parent()?.join(target)),
            Err(_) => return Some(current),
        }
    }
    None
}

/// A path with its `.` and `..` worked out.
fn normalized(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    clean
}

/// `pid` and everything descended from it, each generation after the one above it.
pub fn descendants(parents: &BTreeMap<u32, u32>, pid: u32) -> Vec<u32> {
    let mut tree = vec![pid];
    let mut index = 0;
    while let Some(&parent) = tree.get(index) {
        for (&child, &of) in parents {
            if of == parent && !tree.contains(&child) {
                tree.push(child);
            }
        }
        index += 1;
    }
    tree
}

#[cfg(test)]
mod tests;
