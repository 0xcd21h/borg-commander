use std::{
    ffi::OsString,
    fs, io,
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use borg_core::{Msg, Password, Platform, Side, VAULT_CONFIG, VAULT_KEY_SEPARATOR, VaultError, VaultMount};
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

const FOLDER: &str = "vaults";
const PROGRAM: &str = "gocryptfs";
const QUIET: &str = "-q";
const IDLE: &str = "-idle";
const INIT: &str = "-init";
const KEY_FROM_INPUT: &str = "-masterkey=stdin";
const END_OF_FLAGS: &str = "--";
const DIRIV: &str = "gocryptfs.diriv";
const WRONG_PASSWORD: i32 = 12;
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(50);
const ERROR_WAIT: Duration = Duration::from_millis(500);
const KEY_BYTES: usize = 32;
const KEY_GROUP: usize = 8;
const NAME_ATTEMPTS: usize = 100;
const PRIVATE_DIR: u32 = 0o700;
const BUSY: &str = "busy";
const LOCALE: (&str, &str) = ("LC_ALL", "C");
const SECONDS: &str = "s";
const MOUNTINFO: &str = "/proc/self/mountinfo";
const MOUNTINFO_END: &str = "-";
const MOUNT_POINT_FIELD: usize = 4;
const GOCRYPTFS_TYPE: &str = "fuse.gocryptfs";
const MAC_FUSE: &str = "/Library/Filesystems/macfuse.fs";
const LINUX_FUSE: &str = "/dev/fuse";
const MAC_FUSE_TYPES: [&str; 2] = ["macfuse", "osxfuse"];
const LIST_MOUNTS: &str = "mount";
const MOUNT_SOURCE_END: &str = " on ";
const MOUNT_OPTIONS_START: &str = " (";
const MOUNT_TYPE_END: char = ',';
const MAC_UNMOUNT: &str = "umount";
const LINUX_UNMOUNTS: [&str; 2] = ["fusermount3", "fusermount"];
const LINUX_UNMOUNT_FLAG: &str = "-u";
const OCTAL_ESCAPE: char = '\\';
const OCTAL_DIGITS: usize = 3;
const OCTAL_RADIX: u32 = 8;
const FSNAME_COMMA: char = ',';
const FSNAME_COMMA_SHOWN: &str = "_";

#[derive(Debug, Clone)]
pub(crate) struct Vaults {
    folder: PathBuf,
    idle: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    Locked,
    Open(PathBuf),
    Inside(VaultMount),
    Gone(PathBuf),
}

impl Found {
    pub(crate) fn message(self, side: Side, generation: u64, dir: PathBuf) -> Msg {
        match self {
            Self::Locked => Msg::VaultFound { side, generation, cipher: dir, open: None },
            Self::Open(root) => Msg::VaultFound { side, generation, cipher: dir, open: Some(root) },
            Self::Inside(vault) => Msg::VaultShown { side, generation, vault },
            Self::Gone(root) => Msg::VaultClosed { root, error: None },
        }
    }

    pub(crate) fn replaces_listing(&self) -> bool {
        !matches!(self, Self::Inside(_))
    }
}

impl Vaults {
    pub(crate) fn new(runtime_dir: &Path, idle: Option<Duration>) -> Self {
        let runtime = fs::canonicalize(runtime_dir).unwrap_or_else(|_| runtime_dir.to_path_buf());
        Self { folder: runtime.join(FOLDER), idle }
    }

    pub(crate) fn check(&self, dir: &Path, known: Option<&VaultMount>) -> Option<Found> {
        if dir.join(VAULT_CONFIG).is_file() {
            return Some(self.mounted_at(dir).map_or(Found::Locked, Found::Open));
        }
        if let Some(vault) = known.filter(|vault| vault.holds(dir)) {
            return (!is_mount(&vault.root)).then(|| self.gone(&vault.root));
        }
        let root = self.root_of(dir)?;
        if !is_mount(&root) {
            return Some(self.gone(&root));
        }
        self.source_of(&root).map(|cipher| Found::Inside(VaultMount { cipher, root }))
    }

    pub(crate) fn open(&self, cipher: &Path, password: &Password) -> Result<VaultMount, VaultError> {
        let program = program()?;
        fuse_ready()?;
        let source = fs::canonicalize(cipher).map_err(failed)?;
        let root = self.mount_point(cipher)?;
        let idle = self.idle.map(|idle| format!("{}{SECONDS}", idle.as_secs()));
        let flags = [QUIET].into_iter().chain(idle.iter().flat_map(|idle| [IDLE, idle.as_str()]));
        let mut args: Vec<OsString> = flags.map(OsString::from).collect();
        args.extend([END_OF_FLAGS.into(), source.into_os_string(), root.clone().into_os_string()]);
        match run(&program, &args, &[password.as_str()]) {
            Ok(()) => Ok(VaultMount { cipher: cipher.to_path_buf(), root }),
            Err(error) => {
                self.release(&root);
                Err(error)
            }
        }
    }

    pub(crate) fn create(&self, cipher: &Path, password: &Password) -> Result<(VaultMount, Password), VaultError> {
        let program = program()?;
        fuse_ready()?;
        match fs::create_dir(cipher) {
            Ok(()) => fs::set_permissions(cipher, fs::Permissions::from_mode(PRIVATE_DIR)).map_err(failed)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(VaultError::Exists),
            Err(error) => return Err(failed(error)),
        }
        let key = new_key().map_err(|error| {
            remove_new(cipher);
            failed(error)
        })?;
        let args: Vec<OsString> = [INIT, QUIET, KEY_FROM_INPUT, END_OF_FLAGS]
            .into_iter()
            .map(OsString::from)
            .chain([cipher.as_os_str().to_owned()])
            .collect();
        if let Err(error) = run(&program, &args, &[password.as_str(), key.as_str()]) {
            remove_new(cipher);
            return Err(error);
        }
        let vault = self.open(cipher, password)?;
        Ok((vault, Password::new(&key)))
    }

    pub(crate) fn close(&self, root: &Path) -> Result<(), VaultError> {
        if !is_mount(root) {
            self.release(root);
            return Ok(());
        }
        let (program, args) = unmount_command(root)?;
        match run(&program, &args, &[]) {
            Ok(()) => {
                self.release(root);
                Ok(())
            }
            Err(VaultError::Failed(error)) if error.to_lowercase().contains(BUSY) => Err(VaultError::Busy),
            Err(error) => Err(error),
        }
    }

    fn gone(&self, root: &Path) -> Found {
        self.release(root);
        Found::Gone(root.to_path_buf())
    }

    fn release(&self, root: &Path) {
        if root.parent() == Some(self.folder.as_path()) {
            let _ = fs::remove_dir(root);
        }
    }

    fn root_of(&self, dir: &Path) -> Option<PathBuf> {
        let first = dir.strip_prefix(&self.folder).ok()?.components().next()?;
        Some(self.folder.join(first))
    }

    fn mount_point(&self, cipher: &Path) -> Result<PathBuf, VaultError> {
        fs::create_dir_all(&self.folder).map_err(failed)?;
        fs::set_permissions(&self.folder, fs::Permissions::from_mode(PRIVATE_DIR)).map_err(failed)?;
        let name = cipher.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        for candidate in candidates(&name).take(NAME_ATTEMPTS) {
            let path = self.folder.join(candidate);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if !is_mount(&path) && is_empty_dir(&path) {
                        return Ok(path);
                    }
                }
                Err(error) => return Err(failed(error)),
            }
        }
        Err(VaultError::Failed(self.folder.display().to_string()))
    }

    fn mounted_at(&self, cipher: &Path) -> Option<PathBuf> {
        let source = fsname(&fs::canonicalize(cipher).ok()?);
        match crate::tools::PLATFORM {
            Platform::Linux => {
                let table = fs::read_to_string(MOUNTINFO).ok()?;
                mountinfo(&table).into_iter().find(|mount| mount.source == source).map(|mount| mount.point)
            }
            Platform::MacOs => mount_point_of(&mount_table()?, &source),
        }
    }

    fn source_of(&self, root: &Path) -> Option<PathBuf> {
        match crate::tools::PLATFORM {
            Platform::Linux => {
                let table = fs::read_to_string(MOUNTINFO).ok()?;
                mountinfo(&table).into_iter().find(|mount| mount.point == root).map(|mount| PathBuf::from(mount.source))
            }
            Platform::MacOs => source_of(&mount_table()?, root).map(PathBuf::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mounted {
    pub(crate) source: String,
    pub(crate) point: PathBuf,
}

pub(crate) fn mountinfo(table: &str) -> Vec<Mounted> {
    table
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let end = fields.iter().position(|field| *field == MOUNTINFO_END)?;
            let (kind, source) = (fields.get(end + 1)?, fields.get(end + 2)?);
            let point = fields.get(MOUNT_POINT_FIELD)?;
            (*kind == GOCRYPTFS_TYPE)
                .then(|| Mounted { source: unescape(source), point: PathBuf::from(unescape(point)) })
        })
        .collect()
}

pub(crate) fn mount_point_of(table: &str, source: &str) -> Option<PathBuf> {
    let prefix = format!("{source}{MOUNT_SOURCE_END}");
    table.lines().find_map(|line| {
        let rest = line.strip_prefix(&prefix)?;
        let (point, options) = rest.rsplit_once(MOUNT_OPTIONS_START)?;
        fuse_options(options).then(|| PathBuf::from(point))
    })
}

pub(crate) fn source_of(table: &str, point: &Path) -> Option<String> {
    let middle = format!("{MOUNT_SOURCE_END}{}{MOUNT_OPTIONS_START}", point.display());
    table.lines().find_map(|line| {
        let (source, options) = line.split_once(&middle)?;
        fuse_options(options).then(|| source.to_owned())
    })
}

fn fuse_options(options: &str) -> bool {
    let kind = options.split(MOUNT_TYPE_END).next().unwrap_or_default();
    MAC_FUSE_TYPES.contains(&kind)
}

pub(crate) fn unescape(field: &str) -> String {
    let mut bytes = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some(index) = rest.find(OCTAL_ESCAPE) {
        bytes.extend_from_slice(&rest.as_bytes()[..index]);
        let code = rest.get(index + 1..index + 1 + OCTAL_DIGITS);
        match code.and_then(|code| u8::from_str_radix(code, OCTAL_RADIX).ok()) {
            Some(byte) => {
                bytes.push(byte);
                rest = &rest[index + 1 + OCTAL_DIGITS..];
            }
            None => {
                bytes.push(OCTAL_ESCAPE as u8);
                rest = &rest[index + 1..];
            }
        }
    }
    bytes.extend_from_slice(rest.as_bytes());
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(crate) fn fsname(cipher: &Path) -> String {
    cipher.to_string_lossy().replace(FSNAME_COMMA, FSNAME_COMMA_SHOWN)
}

pub(crate) fn recovery_key(bytes: &[u8]) -> Zeroizing<String> {
    let hex = Zeroizing::new(borg_link::hex(bytes));
    let groups: Vec<&str> =
        hex.as_bytes().chunks(KEY_GROUP).filter_map(|group| std::str::from_utf8(group).ok()).collect();
    Zeroizing::new(groups.join(VAULT_KEY_SEPARATOR))
}

pub(crate) fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(name.to_owned()).chain((2..).map(move |number| format!("{name} {number}")))
}

pub(crate) fn last_line(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    text.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or_default().to_owned()
}

fn new_key() -> io::Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0; KEY_BYTES]);
    getrandom::fill(bytes.as_mut()).map_err(io::Error::other)?;
    Ok(recovery_key(bytes.as_ref()))
}

fn remove_new(cipher: &Path) {
    for name in [VAULT_CONFIG, DIRIV] {
        let _ = fs::remove_file(cipher.join(name));
    }
    let _ = fs::remove_dir(cipher);
}

fn program() -> Result<PathBuf, VaultError> {
    crate::tools::find(PROGRAM).ok_or_else(|| {
        let installer = match crate::tools::PLATFORM {
            Platform::Linux => crate::tools::installer(),
            Platform::MacOs => None,
        };
        VaultError::NoTool(installer)
    })
}

fn fuse_ready() -> Result<(), VaultError> {
    let device = match crate::tools::PLATFORM {
        Platform::Linux => LINUX_FUSE,
        Platform::MacOs => MAC_FUSE,
    };
    if Path::new(device).exists() { Ok(()) } else { Err(VaultError::NoFuse(crate::tools::PLATFORM)) }
}

fn unmount_command(root: &Path) -> Result<(PathBuf, Vec<OsString>), VaultError> {
    let missing = || VaultError::Failed(LINUX_UNMOUNTS.join(" "));
    match crate::tools::PLATFORM {
        Platform::MacOs => {
            let program = crate::tools::find(MAC_UNMOUNT).ok_or_else(|| VaultError::Failed(MAC_UNMOUNT.to_owned()))?;
            Ok((program, vec![root.as_os_str().to_owned()]))
        }
        Platform::Linux => {
            let program = LINUX_UNMOUNTS.into_iter().find_map(crate::tools::find).ok_or_else(missing)?;
            Ok((program, vec![LINUX_UNMOUNT_FLAG.into(), root.as_os_str().to_owned()]))
        }
    }
}

fn mount_table() -> Option<String> {
    let program = crate::tools::find(LIST_MOUNTS)?;
    let output = Command::new(program).env(LOCALE.0, LOCALE.1).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run(program: &Path, args: &[OsString], lines: &[&str]) -> Result<(), VaultError> {
    let mut child = Command::new(program)
        .args(args)
        .env(LOCALE.0, LOCALE.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(failed)?;
    let errors = collect_errors(&mut child);
    if let Some(mut input) = child.stdin.take() {
        for line in lines {
            let line = Zeroizing::new(format!("{line}\n"));
            input.write_all(line.as_bytes()).map_err(failed)?;
        }
    }
    let status = wait(&mut child)?;
    let output = errors.recv_timeout(ERROR_WAIT).unwrap_or_default();
    match status.code() {
        Some(0) => Ok(()),
        Some(WRONG_PASSWORD) => Err(VaultError::WrongPassword),
        code => {
            let line = last_line(&output);
            let shown = if line.is_empty() { code.map(|code| code.to_string()).unwrap_or_default() } else { line };
            Err(VaultError::Failed(shown))
        }
    }
}

fn collect_errors(child: &mut Child) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    if let Some(mut errors) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = errors.read_to_end(&mut output);
            let _ = sender.send(output);
        });
    }
    receiver
}

fn wait(child: &mut Child) -> Result<std::process::ExitStatus, VaultError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(failed)? {
            return Ok(status);
        }
        if started.elapsed() > RUN_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(VaultError::Failed(format!("{RUN_TIMEOUT:?}")));
        }
        std::thread::sleep(POLL);
    }
}

fn is_mount(path: &Path) -> bool {
    let parent = path.parent().and_then(|parent| fs::metadata(parent).ok());
    match (fs::metadata(path), parent) {
        (Ok(here), Some(parent)) => here.dev() != parent.dev(),
        _ => false,
    }
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn failed(error: io::Error) -> VaultError {
    VaultError::Failed(error.to_string())
}
