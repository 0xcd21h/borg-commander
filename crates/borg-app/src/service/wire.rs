//! What a Borg and the service say to each other once the Borg has registered its session:
//! one JSON object per line, each way.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::{Call, Reply};

/// The request mode a Borg registers its session with.
pub(super) const REGISTER: &str = "register";

/// Where a session's Borg runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// In a terminal of its own, with the service coordinating.
    Terminal,
    /// Inside the service, which keeps it running without a terminal.
    Background,
}

/// What a Borg tells the service about itself when it registers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Registration {
    pub session: String,
    pub pid: u32,
    /// The socket of the Borg's own mirror, for mirrors of a session in a terminal.
    pub mirror: PathBuf,
    pub place: Place,
}

/// From the service to a registered Borg.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ToMember {
    /// A call for the session, from a paired computer through borg cloud, its origin kept.
    Call { id: u64, call: Call },
    /// The pairing link the Borg asked for, or why there is none.
    Link { link: Result<String, String> },
    /// The computers paired with this machine.
    Computers { names: Vec<String> },
    /// How many paired computers' connections reach the session now.
    Agents { count: usize },
}

/// From a registered Borg to the service.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum FromMember {
    Reply {
        id: u64,
        reply: Reply,
    },
    /// The user is working in this session.
    Active,
    /// The user wants a pairing link.
    Pair,
    /// The user forgets every paired computer.
    Forget,
}

/// A message as the line that carries it.
pub(super) fn line(message: &impl Serialize) -> serde_json::Result<String> {
    Ok(format!("{}\n", serde_json::to_string(message)?))
}
