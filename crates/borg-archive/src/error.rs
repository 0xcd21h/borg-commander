use borg_i18n::tr;

/// Why an archive could not be listed or read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", tr!(archive_unknown))]
    Unknown,
    #[error("{}", tr!(archive_needs_password))]
    NeedsPassword,
    #[error("{}", tr!(archive_wrong_password))]
    WrongPassword,
    #[error("{}", tr!(archive_overlapping))]
    Overlapping,
    #[error("{}", tr!(archive_too_many))]
    TooMany,
    #[error("{}", tr!(archive_no_tool))]
    NoTool,
    #[error("{}", tr!(archive_tool_failed, tool = .0, error = .1))]
    Tool(String, String),
    #[error("{}", tr!(archive_damaged, error = .0))]
    Damaged(String),
    /// The reader was asked to stop.
    #[error("{}", tr!(archive_stopped))]
    Stopped,
    #[error(transparent)]
    Io(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::Interrupted && error.get_ref().is_some_and(|inner| inner.is::<Halt>()) {
            Self::Stopped
        } else {
            Self::Io(error)
        }
    }
}

/// Carried inside an I/O error when a reader stops because it was asked to.
#[derive(Debug)]
pub(crate) struct Halt;

impl std::fmt::Display for Halt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(tr!(archive_stopped))
    }
}

impl std::error::Error for Halt {}

pub(crate) fn halted() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, Halt)
}
