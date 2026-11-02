//! What an agent asks to capture, read from its call: the traffic, the interface, how long, how
//! much of each packet, and how many rows the summary lists.

use super::LIST_LIMIT;
use borg_core::{CaptureScope, LONGEST_CAPTURE, SHORTEST_CAPTURE, Transport};
use serde_json::Value;
use std::net::SocketAddr;

/// How many rows each list of a capture's summary shows unless the call asks otherwise.
pub(crate) const SUMMARY_ROWS: usize = 20;
const TCP: &str = "tcp";
const UDP: &str = "udp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureCall {
    pub scope: CaptureScope,
    pub interface: Option<String>,
    pub seconds: u64,
    pub headers_only: bool,
    pub limit: usize,
}

/// Reads a capture call's arguments, or says what is wrong with them.
pub fn capture_call(args: &Value) -> Result<CaptureCall, String> {
    let has = |key: &str| !args[key].is_null();
    let (port, connection, process) = (has("port"), has("local") || has("remote"), has("pid"));
    if [port, connection, process].into_iter().filter(|&given| given).count() > 1 {
        return Err("capture takes one scope: port, local and remote, or pid".into());
    }
    let transport = match args["transport"].as_str().unwrap_or(TCP) {
        TCP => Transport::Tcp,
        UDP => Transport::Udp,
        other => return Err(format!("capture has no transport {other}: use tcp or udp")),
    };
    let scope = if port {
        let port = args["port"].as_u64().and_then(|port| u16::try_from(port).ok()).filter(|&port| port > 0);
        CaptureScope::Port { transport, port: port.ok_or("port must be a number from 1 to 65535")? }
    } else if connection {
        let address = |key: &str| args[key].as_str().and_then(|text| text.trim().parse::<SocketAddr>().ok());
        let (Some(local), Some(remote)) = (address("local"), address("remote")) else {
            return Err("local and remote must both be addresses with ports, such as 192.0.2.10:50123".into());
        };
        CaptureScope::Connection { transport, local, remote }
    } else if process {
        let pid = args["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()).filter(|&pid| pid > 0);
        CaptureScope::Process { pid: pid.ok_or("pid must be the PID of a running process")? }
    } else {
        CaptureScope::Everything
    };
    let seconds = match &args["seconds"] {
        Value::Null => SHORTEST_CAPTURE,
        value => value
            .as_u64()
            .filter(|seconds| (1..=LONGEST_CAPTURE).contains(seconds))
            .ok_or_else(|| format!("seconds must be from 1 to {LONGEST_CAPTURE}"))?,
    };
    Ok(CaptureCall {
        scope,
        interface: args["interface"].as_str().map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned),
        seconds,
        headers_only: args["headers_only"] == true,
        limit: summary_rows(args),
    })
}

/// How many rows each list of a capture's summary shows, as the call's `limit` asks.
pub fn summary_rows(args: &Value) -> usize {
    args["limit"].as_u64().map_or(SUMMARY_ROWS, |limit| limit.clamp(1, LIST_LIMIT as u64) as usize)
}

/// The packet a read asks for by its number, counted from 1, when it asks for one.
pub fn packet_number(args: &Value) -> Result<Option<usize>, String> {
    match &args["packet"] {
        Value::Null => Ok(None),
        value => value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .filter(|&number| number > 0)
            .map(Some)
            .ok_or_else(|| "packet must be a packet's number, counted from 1".to_owned()),
    }
}
