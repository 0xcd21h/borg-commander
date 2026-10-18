use std::{
    fs, io,
    path::{Path, PathBuf},
};

use portable_pty::CommandBuilder;

const ENV_MARKER: &str = "BORG_COMMANDER";
const ENV_INTEGRATION_DIR: &str = "BORG_INTEGRATION_DIR";
const ENV_USER_ZDOTDIR: &str = "BORG_USER_ZDOTDIR";
const ENV_ZDOTDIR: &str = "ZDOTDIR";
const ENV_TERM: &str = "TERM";
const TERM_NAME: &str = "xterm-256color";

const BASH_RC: Script = Script { name: "bashrc.bash", body: include_str!("../shell/bashrc.bash") };
const ZSH_ENV: Script = Script { name: ".zshenv", body: include_str!("../shell/zshenv.zsh") };
const ZSH_INTEGRATION: Script = Script { name: "integration.zsh", body: include_str!("../shell/integration.zsh") };
const FISH_INTEGRATION: Script = Script { name: "integration.fish", body: include_str!("../shell/integration.fish") };

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
            Self::Bash => &[BASH_RC],
            Self::Zsh => &[ZSH_ENV, ZSH_INTEGRATION],
            Self::Fish => &[FISH_INTEGRATION],
            Self::Other => &[],
        }
    }
}

pub fn shell_command(program: &Path, cwd: &Path, runtime_dir: &Path) -> io::Result<CommandBuilder> {
    let kind = ShellKind::detect(program);
    let integration_dir = install(kind, runtime_dir)?;

    let mut cmd = CommandBuilder::new(program);
    cmd.cwd(cwd);
    cmd.env(ENV_MARKER, env!("CARGO_PKG_VERSION"));
    cmd.env(ENV_TERM, TERM_NAME);
    cmd.env(ENV_INTEGRATION_DIR, &integration_dir);

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

fn install(kind: ShellKind, runtime_dir: &Path) -> io::Result<PathBuf> {
    let dir = runtime_dir.join(env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(&dir)?;
    kind.scripts().iter().try_for_each(|script| fs::write(dir.join(script.name), script.body))?;
    Ok(dir)
}
