use std::{
    fs::{self, File},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tempfile::TempDir;

const FIXED_EPOCH_SECS: u64 = 1_614_834_360;

pub fn fixed_mtime() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(FIXED_EPOCH_SECS)
}

const WORK: &str = "work";
const HOME: &str = "home";
const CONFIG: &str = "config";
const RUNTIME: &str = "run";
const STATE: &str = "state";
const EXECUTABLE_MODE: u32 = 0o755;
const CONFIG_FILE: &str = "config.toml";
const SPLASH_KEY: &str = "startup-animation";
const QUIET_CONFIG: &str = "startup-animation = false\n";

const SHELL_RC: [(&str, &str); 4] = [
    (".bashrc", "PS1='$ '\n"),
    (".zshenv", "skip_global_compinit=1\n"),
    (".zshrc", "PROMPT='$ '\nRPROMPT=''\nPROMPT_EOL_MARK=''\n"),
    (".config/fish/config.fish", "set -g fish_greeting ''\nfunction fish_prompt\n    printf '$ '\nend\n"),
];

pub struct Fixture {
    _guard: TempDir,
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> io::Result<Self> {
        let guard = tempfile::Builder::new().prefix("borg-e2e-").tempdir()?;
        let root = guard.path().canonicalize()?;
        let fixture = Self { _guard: guard, root };
        [WORK, HOME, CONFIG, RUNTIME, STATE].iter().try_for_each(|dir| fs::create_dir_all(fixture.root.join(dir)))?;
        SHELL_RC.iter().try_for_each(|(name, body)| write_file(&fixture.home().join(name), body.as_bytes()))?;
        fixture.configure("")?;
        Ok(fixture)
    }

    pub fn configure(&self, body: &str) -> io::Result<()> {
        let quiet = if body.lines().any(|line| line.trim_start().starts_with(SPLASH_KEY)) { "" } else { QUIET_CONFIG };
        write_file(&self.config_file(), format!("{quiet}{body}").as_bytes())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config().join(CONFIG_FILE)
    }

    pub fn work(&self) -> PathBuf {
        self.root.join(WORK)
    }

    pub fn home(&self) -> PathBuf {
        self.root.join(HOME)
    }

    pub fn config(&self) -> PathBuf {
        self.root.join(CONFIG)
    }

    pub fn runtime(&self) -> PathBuf {
        self.root.join(RUNTIME)
    }

    pub fn state(&self) -> PathBuf {
        self.root.join(STATE)
    }

    pub fn dir(&self, relative: &str) -> io::Result<PathBuf> {
        let path = self.work().join(relative);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn file(&self, relative: &str, contents: &str) -> io::Result<PathBuf> {
        let path = self.work().join(relative);
        write_file(&path, contents.as_bytes())?;
        Ok(path)
    }

    pub fn executable(&self, relative: &str, contents: &str) -> io::Result<PathBuf> {
        let path = self.file(relative, contents)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(EXECUTABLE_MODE))?;
        Ok(path)
    }

    pub fn many(&self, relative: &str, count: usize) -> io::Result<PathBuf> {
        let dir = self.dir(relative)?;
        (0..count).try_for_each(|i| File::create(dir.join(format!("item_{i:06}.dat"))).map(drop))?;
        Ok(dir)
    }

    pub fn freeze(&self) -> io::Result<()> {
        freeze(&self.work())
    }
}

fn write_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn freeze(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::read_dir(path)?.filter_map(Result::ok).try_for_each(|entry| freeze(&entry.path()))?;
    }
    File::open(path)?.set_modified(fixed_mtime())
}
