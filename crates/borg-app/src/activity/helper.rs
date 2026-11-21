//! Reading another user's process as administrator, or sending it a signal or a new priority,
//! through Borg's helper, with its output kept short.

use super::{
    ADMIN_TIMEOUT, ERROR_LIMIT, OUTPUT_LIMIT, ProcessScan, ScanRequest, capture, census, integrity, process_files,
    regions, scan_process, scan_with_rules, search_memory,
};
use crate::{memory_search::Query, rule_scan::RuleSet};
use anyhow::{Context, Result, anyhow, bail};
use borg_core::{ActivityData, ActivityRequest, MemoryRegion, ProcessKey};
use borg_i18n::tr;
use serde::de::DeserializeOwned;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

/// The longest signal or priority change the helper reads from its arguments.
const CONTROL_LIMIT: usize = 64;
pub(super) const HELPER_FLAG: &str = "--inspect-process";
/// An agent's scan of a process: integrity, rules or both in one run, so the password is asked
/// for once.
const SCAN: &str = "scan";
/// The longest scan request the helper reads: rule text at its longest with every byte escaped.
const SCAN_REQUEST_LIMIT: usize = 8 * borg_rules::INLINE_LIMIT;
/// The entropy scan of the processes Borg could not read.
pub(super) const ENTROPY: &str = "entropy";

pub(crate) async fn bounded_output(mut command: Command, timeout: Duration) -> Result<Vec<u8>> {
    command
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().context(tr!(activity_unavailable))?;
    let stderr = child.stderr.take().context(tr!(activity_unavailable))?;
    tokio::time::timeout(timeout, async move {
        let (bytes, error) = tokio::try_join!(read_output(stdout, OUTPUT_LIMIT), read_output(stderr, ERROR_LIMIT))?;
        let status = child.wait().await?;
        if !status.success() {
            bail!("{}", String::from_utf8_lossy(&error).trim());
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| anyhow!(tr!(activity_inspection_timeout)))?
}

pub(super) async fn read_output(reader: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        bail!(tr!(activity_output_limit));
    }
    Ok(bytes)
}

pub(super) fn helper_args(request: &ActivityRequest) -> Result<Vec<String>> {
    let mut args = vec![HELPER_FLAG.into()];
    match request {
        request @ ActivityRequest::Search { snapshot: None, .. } => {
            args.extend(["search".into(), serde_json::to_string(request)?]);
        }
        ActivityRequest::Regions { process, .. } => {
            args.extend(["regions".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Files { process, .. } => {
            args.extend(["files".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Integrity { process, .. } => {
            args.extend(["integrity".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Rules { process, .. } => {
            args.extend(["rules".into(), process.pid.to_string(), process.start.to_string()]);
            args.extend(crate::rule_scan::folder().map(|folder| folder.display().to_string()));
        }
        ActivityRequest::Capture { process, region, seek, .. } => args.extend([
            "capture".into(),
            process.pid.to_string(),
            process.start.to_string(),
            region.start.to_string(),
            region.end.to_string(),
            u8::from(*seek).to_string(),
        ]),
        ActivityRequest::Census { process, region, .. } => args.extend([
            "census".into(),
            process.pid.to_string(),
            process.start.to_string(),
            region.start.to_string(),
            region.end.to_string(),
        ]),
        ActivityRequest::Control { process, control, .. } => args.extend([
            "control".into(),
            process.pid.to_string(),
            process.start.to_string(),
            serde_json::to_string(control)?,
        ]),
        _ => bail!(tr!(activity_request_invalid)),
    }
    Ok(args)
}

pub(super) async fn elevate(request: ActivityRequest) -> Result<ActivityData> {
    elevated(helper_args(&request)?).await
}

/// Scans a process as administrator, with every check the request asks for in one run.
pub(super) async fn elevate_scan(request: &ScanRequest) -> Result<ProcessScan> {
    elevated(vec![HELPER_FLAG.into(), SCAN.into(), serde_json::to_string(request)?]).await
}

/// Runs Borg's helper with `args` as administrator and reads what it answers.
pub(super) async fn elevated<T: DeserializeOwned>(args: Vec<String>) -> Result<T> {
    let exe = crate::program::path()?;
    #[cfg(target_os = "macos")]
    let command = {
        let words =
            std::iter::once(exe.to_str().context(tr!(activity_unavailable))?).chain(args.iter().map(String::as_str));
        let script = crate::admin::apple_script(words)?;
        let mut command = Command::new("/usr/bin/osascript");
        command.arg("-e").arg(script);
        command
    };
    #[cfg(target_os = "linux")]
    let command = {
        let mut command = Command::new("pkexec");
        command.arg(exe).args(args);
        command
    };
    let bytes = bounded_output(command, ADMIN_TIMEOUT)
        .await
        .map_err(|e| anyhow!("{}", tr!(activity_admin_unavailable, error = e)))?;
    let result: Result<T, String> = serde_json::from_slice(&bytes)?;
    result.map_err(anyhow::Error::msg)
}

pub async fn helper(args: &[String]) -> Result<()> {
    if let [kind, request] = args
        && kind == SCAN
    {
        let result = scan_helper(request).await.map_err(|error| error.to_string());
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    let result = async {
        let kind = args.first().context(tr!(activity_request_invalid))?;
        if kind == "search" && args.len() == 2 {
            if args[1].len() > 4096 {
                bail!(tr!(activity_request_invalid));
            }
            let request: ActivityRequest = serde_json::from_str(&args[1])?;
            if let ActivityRequest::Search { process, query, mode, match_case, snapshot: None, start, .. } = request {
                let query = Query { text: query, mode, match_case };
                return search_memory(process, query, None, start, Arc::new(AtomicBool::new(false)), None)
                    .await
                    .map(ActivityData::Search);
            }
            bail!(tr!(activity_request_invalid));
        }
        if kind == ENTROPY && args.len() == 2 {
            if args[1].len() > super::entropy::JOB_LIMIT {
                bail!(tr!(activity_request_invalid));
            }
            let job: super::entropy::Job = serde_json::from_str(&args[1])?;
            return Ok(ActivityData::Entropy(super::entropy::scan_as_helper(job).await));
        }

        let number =
            |i: usize| args.get(i).context(tr!(activity_request_invalid))?.parse::<u64>().map_err(anyhow::Error::from);
        let process = ProcessKey { pid: number(1)?.try_into()?, start: number(2)? };
        if process.pid == 0 {
            bail!(tr!(activity_request_invalid));
        }
        match (kind.as_str(), args.len()) {
            ("regions", 3) => Ok(ActivityData::Regions(regions(process).await?)),
            ("files", 3) => Ok(ActivityData::Files(process_files::list(process).await?)),
            ("integrity", 3) => {
                Ok(ActivityData::Integrity(integrity(process, Arc::new(AtomicBool::new(false)), None).await?))
            }
            ("rules", 3 | 4) => {
                let rules = RuleSet::Installed { folder: args.get(3).map(PathBuf::from) };
                let cancelled = Arc::new(AtomicBool::new(false));
                Ok(ActivityData::Rules(scan_with_rules(process, rules, cancelled, None).await?.report()))
            }
            ("capture", 6) => {
                let start = number(3)?;
                let end = number(4)?;
                let seek = match number(5)? {
                    0 => false,
                    1 => true,
                    _ => bail!(tr!(activity_request_invalid)),
                };
                if end <= start {
                    bail!(tr!(activity_request_invalid));
                }
                let region = MemoryRegion { start, end, permissions: "r--".into(), ..Default::default() };
                Ok(ActivityData::Snapshot(capture(process, region, seek, true).await?))
            }
            ("control", 4) => {
                let control = args[3].as_str();
                if control.len() > CONTROL_LIMIT {
                    bail!(tr!(activity_request_invalid));
                }
                Ok(ActivityData::Control(super::control::control(process, serde_json::from_str(control)?)?))
            }
            ("census", 5) => {
                let (start, end) = (number(3)?, number(4)?);
                let maps = regions(process).await?;
                let region =
                    maps.iter().find(|r| r.start == start && r.end == end).context(tr!(activity_region_invalid))?;
                let mapped = maps.iter().filter(|r| r.readable()).map(|r| (r.start, r.end)).collect();
                let cancelled = Arc::new(AtomicBool::new(false));
                Ok(ActivityData::Census(census(process, region.clone(), mapped, cancelled).await?))
            }
            _ => bail!(tr!(activity_request_invalid)),
        }
    }
    .await
    .map_err(|error: anyhow::Error| error.to_string());
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

async fn scan_helper(request: &str) -> Result<ProcessScan> {
    if request.len() > SCAN_REQUEST_LIMIT {
        bail!(tr!(activity_request_invalid));
    }
    let request: ScanRequest = serde_json::from_str(request)?;
    if request.process.pid == 0 {
        bail!(tr!(activity_request_invalid));
    }
    scan_process(request, Arc::new(AtomicBool::new(false))).await
}
