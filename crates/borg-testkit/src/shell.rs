use std::{env, path::PathBuf};

const ENV_PATH: &str = "PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub const ALL: [Self; 3] = [Self::Bash, Self::Zsh, Self::Fish];

    pub fn program(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }

    pub fn locate(self) -> Option<PathBuf> {
        let path = env::var_os(ENV_PATH)?;
        env::split_paths(&path).map(|dir| dir.join(self.program())).find(|candidate| candidate.is_file())
    }

    pub fn available() -> Vec<(Self, PathBuf)> {
        Self::ALL.into_iter().filter_map(|shell| shell.locate().map(|path| (shell, path))).collect()
    }
}
