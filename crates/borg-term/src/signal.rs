use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellSignal {
    PromptStart,
    InputStart,
    CommandStart,
    CommandEnd {
        exit: Option<i32>,
    },
    Cwd {
        host: String,
        path: PathBuf,
    },
    /// The shell's integration can change directory without a command line; see
    /// [`crate::SILENT_CD_KEY`]. Shells announce it at every prompt.
    SilentCd,
    ExitRequested {
        token: String,
    },
}
