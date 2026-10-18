use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellSignal {
    PromptStart,
    InputStart,
    CommandStart,
    CommandEnd { exit: Option<i32> },
    Cwd { host: String, path: PathBuf },
}
