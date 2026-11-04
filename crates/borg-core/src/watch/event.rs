//! What Borg knows about a process the moment it starts: the program it runs, who started it,
//! what its standard streams are, and the preload variables in its environment. The app fills
//! this in from the system; everything that judges it works on this alone.

use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::ProcessKey;

/// What a process's standard input or output leads to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    Terminal,
    Pipe,
    File,
    Socket,
    /// Something else, or nothing Borg could read.
    #[default]
    Other,
}

impl StreamKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => borg_i18n::tr!(watch_stream_terminal),
            Self::Pipe => borg_i18n::tr!(watch_stream_pipe),
            Self::File => borg_i18n::tr!(watch_stream_file),
            Self::Socket => borg_i18n::tr!(watch_stream_socket),
            Self::Other => borg_i18n::tr!(watch_stream_other),
        }
    }
}

/// A process the new one descends from, the parent first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ancestor {
    pub pid: u32,
    pub name: String,
    pub program: Option<PathBuf>,
    pub terminal: bool,
    /// Its command as one line, empty where Borg could not read it.
    #[serde(default)]
    pub command: String,
}

impl Ancestor {
    /// The name of its program's file, or the name it calls itself.
    pub fn program_name(&self) -> &str {
        program_name(self.program.as_deref()).unwrap_or(&self.name)
    }
}

/// A process that has just started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartedProcess {
    pub key: ProcessKey,
    /// What the process calls itself.
    pub name: String,
    pub program: Option<PathBuf>,
    /// Its program's file was deleted while it runs.
    #[serde(default)]
    pub program_deleted: bool,
    pub command: Vec<String>,
    pub user: String,
    pub uid: u32,
    /// The terminal it belongs to, when it has one.
    pub terminal: Option<String>,
    pub input: StreamKind,
    pub output: StreamKind,
    /// What its standard error leads to.
    #[serde(default)]
    pub errors: StreamKind,
    /// The process that started the first one of its terminal's session: `sshd`, `login` or
    /// `tmux` when somebody logged in to that terminal, or whatever program opened it.
    #[serde(default)]
    pub session_parent: Option<Ancestor>,
    /// Preload variables in its environment, each as its name and value.
    #[serde(default)]
    pub preloads: Vec<(String, String)>,
    /// The link in a system folder it was started by, which gives it its name: `/usr/bin/awk`
    /// for `/usr/bin/mawk` on Debian, for one.
    #[serde(default)]
    pub link: Option<PathBuf>,
    pub ancestors: Vec<Ancestor>,
    pub at: SystemTime,
}

impl Default for StartedProcess {
    fn default() -> Self {
        Self {
            key: ProcessKey { pid: 0, start: 0 },
            name: String::new(),
            program: None,
            program_deleted: false,
            command: Vec::new(),
            user: String::new(),
            uid: 0,
            terminal: None,
            input: StreamKind::default(),
            output: StreamKind::default(),
            errors: StreamKind::default(),
            session_parent: None,
            preloads: Vec::new(),
            link: None,
            ancestors: Vec::new(),
            at: SystemTime::UNIX_EPOCH,
        }
    }
}

impl StartedProcess {
    pub fn parent(&self) -> Option<&Ancestor> {
        self.ancestors.first()
    }

    /// The name of its program's file, or the name it calls itself.
    pub fn program_name(&self) -> &str {
        program_name(self.program.as_deref()).unwrap_or(&self.name)
    }

    /// The folder its program lies in.
    pub fn program_folder(&self) -> Option<&Path> {
        self.program.as_deref().and_then(Path::parent)
    }

    pub fn has_terminal(&self) -> bool {
        self.terminal.is_some()
    }

    /// What its standard input, output and error lead to.
    pub fn streams(&self) -> [StreamKind; 3] {
        [self.input, self.output, self.errors]
    }

    /// Whether it is a copy of its parent that has not started a program of its own: the same
    /// program with the same command. Looking at the process list finds these copies along with
    /// the programs that start.
    pub fn copy_of_parent(&self) -> bool {
        self.parent().is_some_and(|parent| {
            parent.program.is_some() && parent.program == self.program && parent.command == self.command.join(" ")
        })
    }

    /// Its command as one line, as a person would read it.
    pub fn command_line(&self) -> String {
        if self.command.is_empty() { self.program_name().to_owned() } else { self.command.join(" ") }
    }
}

fn program_name(program: Option<&Path>) -> Option<&str> {
    program?.file_name()?.to_str()
}
