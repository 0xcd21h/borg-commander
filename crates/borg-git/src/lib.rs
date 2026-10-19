mod model;

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Output as ProcessOutput, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

pub use model::{Output, Repository, Status, Tool};

const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
const STATUS_TIMEOUT: Duration = Duration::from_secs(15);
const LOG_COUNT: &str = "30";

async fn read(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes).await?;
    ensure!(bytes.len() as u64 <= OUTPUT_LIMIT, "Git output exceeds 8 MiB");
    Ok(bytes)
}

async fn command(dir: &Path, args: &[&str], paths: &[PathBuf]) -> Result<ProcessOutput> {
    let mut command = Command::new("git");
    command
        .args(["--no-pager", "--literal-pathspecs", "-c", "core.fsmonitor=false", "-c", "color.ui=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !paths.is_empty() {
        command.arg("--").args(paths);
    }
    let mut child = command.spawn().context("Git is not available; install Git or disable the plugin")?;
    let (stdout, stderr, status) =
        tokio::try_join!(read(child.stdout.take().unwrap()), read(child.stderr.take().unwrap()), async {
            child.wait().await.map_err(Into::into)
        })?;
    Ok(ProcessOutput { stdout, stderr, status })
}

fn successful(output: ProcessOutput) -> Result<Vec<u8>> {
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(output.stdout)
}

pub async fn inspect(dir: &Path) -> Result<Option<Repository>> {
    tokio::time::timeout(STATUS_TIMEOUT, async {
        let root = command(dir, &["rev-parse", "--show-toplevel"], &[]).await?;
        if !root.status.success() {
            let error = String::from_utf8_lossy(&root.stderr);
            if error.contains("not a git repository") || error.contains("must be run in a work tree") {
                return Ok(None);
            }
            successful(root)?;
            return Ok(None);
        }
        let mut bytes = root.stdout;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        let root = PathBuf::from(OsString::from_vec(bytes));
        let canonical = tokio::fs::canonicalize(dir).await?;
        let prefix = canonical.strip_prefix(&root).context("Git directory is outside its repository")?.to_path_buf();
        let status = successful(
            command(dir, &["status", "--porcelain=v1", "-z", "--branch", "--untracked-files=all", "--", "."], &[])
                .await?,
        )?;
        Repository::parse(root, &status).map(|repository| Some(repository.at_directory(dir.to_path_buf(), prefix)))
    })
    .await
    .context("Git status timed out")?
}

pub async fn run(dir: &Path, tool: Tool, paths: &[PathBuf]) -> Result<Output> {
    ensure!(!tool.needs_selection() || !paths.is_empty(), "Select files first");
    let paths = paths
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.strip_prefix(dir).map(PathBuf::from).context("Selected path is outside the current folder")
            } else {
                Ok(path.clone())
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let (name, args): (_, &[&str]) = match tool {
        Tool::Status => ("Git status.txt", &["status", "--short", "--branch", "--untracked-files=normal"]),
        Tool::Diff => ("Git changes.diff", &["diff", "--no-ext-diff", "--no-textconv"]),
        Tool::StagedDiff => ("Git staged changes.diff", &["diff", "--cached", "--no-ext-diff", "--no-textconv"]),
        Tool::Log => ("Git commits.txt", &["log", "-n", LOG_COUNT, "--date=short", "--format=%h %ad %s"]),
        Tool::Stage => ("", &["add"]),
        Tool::Unstage => ("", &["restore", "--staged"]),
    };
    let has_head = if matches!(tool, Tool::Unstage | Tool::Log) {
        command(dir, &["rev-parse", "--verify", "HEAD"], &[]).await?.status.success()
    } else {
        true
    };
    if tool == Tool::Log && !has_head {
        return Ok(Output::Document { name, text: b"No commits yet.\n".to_vec() });
    }
    let args =
        if tool == Tool::Unstage && !has_head { &["rm", "--cached", "--ignore-unmatch", "-r", "-f"][..] } else { args };
    let selected = if tool.needs_selection() { paths.as_slice() } else { &[] };
    let mut text = successful(command(dir, args, selected).await?)?;
    if tool.changes_index() {
        return Ok(Output::IndexUpdated);
    }
    if text.is_empty() {
        text.extend_from_slice(b"No changes. Untracked files must be staged before they appear in a diff.\n");
    }
    Ok(Output::Document { name, text })
}

use std::os::unix::ffi::OsStringExt;
