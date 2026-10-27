use std::{
    fs,
    io::{self, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

use portable_pty::CommandBuilder;

pub const SESSION_MARKER: &str = "BORG_COMMANDER";
const ENV_INTEGRATION_DIR: &str = "BORG_INTEGRATION_DIR";
const ENV_USER_ZDOTDIR: &str = "BORG_USER_ZDOTDIR";
const ENV_ZDOTDIR: &str = "ZDOTDIR";
const ENV_TERM: &str = "TERM";
const ENV_CD_FILE: &str = "BORG_CD_FILE";
const PRIVATE_FILE: u32 = 0o600;
const PRIVATE_DIR: u32 = 0o700;

/// A key no terminal sends. Shells that announce [`crate::ShellSignal::SilentCd`] bind it
/// to change into the folder named in their request file, then redraw the prompt in place,
/// so the change never appears as a typed command.
pub const SILENT_CD_KEY: &[u8] = b"\x1b[2468~";
const TERM_NAME: &str = "xterm-256color";

const BASH_RC: Script = Script { name: "bashrc.bash", body: include_str!("../shell/bashrc.bash") };
const ZSH_ENV: Script = Script { name: ".zshenv", body: include_str!("../shell/zshenv.zsh") };
const ZSH_INTEGRATION: Script = Script { name: "integration.zsh", body: include_str!("../shell/integration.zsh") };
const FISH_INTEGRATION: Script = Script { name: "integration.fish", body: include_str!("../shell/integration.fish") };
const CONFIRM_EXIT: Script = Script { name: "confirm-exit.sh", body: include_str!("../shell/confirm-exit.sh") };
const CONFIRM_EXIT_READ: Script =
    Script { name: "confirm-exit-read.sh", body: include_str!("../shell/confirm-exit-read.sh") };

struct Script {
    name: &'static str,
    body: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Other,
}

impl ShellKind {
    pub fn detect(program: &Path) -> Self {
        match program.file_name().and_then(|name| name.to_str()) {
            Some("bash") => Self::Bash,
            Some("zsh") => Self::Zsh,
            Some("fish") => Self::Fish,
            _ => Self::Other,
        }
    }

    fn scripts(self) -> &'static [Script] {
        match self {
            Self::Bash => &[BASH_RC, CONFIRM_EXIT, CONFIRM_EXIT_READ],
            Self::Zsh => &[ZSH_ENV, ZSH_INTEGRATION, CONFIRM_EXIT, CONFIRM_EXIT_READ],
            Self::Fish => &[FISH_INTEGRATION, CONFIRM_EXIT_READ],
            Self::Other => &[],
        }
    }
}

pub fn shell_command(program: &Path, cwd: &Path, runtime_dir: &Path) -> io::Result<CommandBuilder> {
    let kind = ShellKind::detect(program);
    let integration_dir = install(kind, runtime_dir)?;

    let mut cmd = CommandBuilder::new(program);
    cmd.cwd(cwd);
    cmd.env(SESSION_MARKER, env!("CARGO_PKG_VERSION"));
    cmd.env(ENV_TERM, TERM_NAME);
    cmd.env(ENV_INTEGRATION_DIR, &integration_dir);
    cmd.env(ENV_CD_FILE, cd_request_file(runtime_dir));

    match kind {
        ShellKind::Bash => cmd.args([Path::new("--rcfile"), &integration_dir.join(BASH_RC.name), Path::new("-i")]),
        ShellKind::Zsh => {
            if let Some(user) = std::env::var_os(ENV_ZDOTDIR) {
                cmd.env(ENV_USER_ZDOTDIR, user);
            }
            cmd.env(ENV_ZDOTDIR, &integration_dir);
        }
        ShellKind::Fish => {
            let source = format!("source '{}'", integration_dir.join(FISH_INTEGRATION.name).display());
            cmd.args(["--init-command", source.as_str()]);
        }
        ShellKind::Other => {}
    }

    Ok(cmd)
}

/// The user's shell: `$SHELL` when it names an executable, otherwise the account's login
/// shell. Sessions such as `docker exec`, cron, or `su` often start without `SHELL`, and
/// falling back to `/bin/sh` there would lose the shell integration.
pub fn user_shell() -> PathBuf {
    PathBuf::from(CommandBuilder::new_default_prog().get_shell())
}

/// Where this process asks its shell to change directory (see [`SILENT_CD_KEY`]).
pub fn cd_request_file(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(env!("CARGO_PKG_VERSION")).join(format!("cd-{}", std::process::id()))
}

/// Writes the folder for the shell to change into. The file is replaced, never written
/// through, and readable only by its owner.
pub fn request_cd(file: &Path, dir: &Path) -> io::Result<()> {
    let staged = file.with_extension("new");
    match fs::remove_file(&staged) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut out = fs::OpenOptions::new().write(true).create_new(true).mode(PRIVATE_FILE).open(&staged)?;
    out.write_all(dir.as_os_str().as_bytes())?;
    drop(out);
    fs::rename(staged, file)
}

fn install(kind: ShellKind, runtime_dir: &Path) -> io::Result<PathBuf> {
    let dir = runtime_dir.join(env!("CARGO_PKG_VERSION"));
    fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR).create(&dir)?;
    kind.scripts().iter().try_for_each(|script| {
        let path = dir.join(script.name);
        let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&path)?;
        out.write_all(script.body.as_bytes())?;
        // A script written by an older borg keeps its mode, so close it here too.
        fs::set_permissions(&path, fs::Permissions::from_mode(PRIVATE_FILE))
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_shell_is_an_executable_even_without_a_shell_variable() {
        let shell = user_shell();
        let mode = fs::metadata(&shell).map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()));
        assert!(mode.is_ok_and(|mode| mode & 0o111 != 0), "{} is not executable", shell.display());
    }
}
