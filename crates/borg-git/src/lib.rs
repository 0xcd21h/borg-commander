use borg_i18n::tr;
mod model;

use std::{
    collections::BTreeSet,
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
    ensure!(bytes.len() as u64 <= OUTPUT_LIMIT, "{}", tr!(git_output_too_large));
    Ok(bytes)
}

/// Settings that keep Git from starting a program only because a setting names one.
const NO_PROGRAMS: [&str; 4] = ["-c", "core.fsmonitor=false", "-c", "log.showSignature=false"];
const FILTER_SECTION: &str = "filter.";
/// The settings of a filter that name a program Git runs on the files it reads.
const FILTER_PROGRAMS: [&str; 3] = ["clean", "smudge", "process"];
/// What `git config --get-regexp` exits with when no setting matches.
const NO_MATCH: i32 = 1;

/// Settings that turn off the filters named in `names`, the NUL-separated names of a
/// repository's own filter settings. A repository somebody else made can name any program as
/// a filter, and Git would run it on the files it compares while Borg only looks at a folder.
fn without_filters(names: &[u8]) -> Result<Vec<String>> {
    let names = std::str::from_utf8(names).context(tr!(git_filter_name_unreadable))?;
    let drivers: BTreeSet<&str> = names
        .split('\0')
        .filter_map(|name| name.strip_prefix(FILTER_SECTION)?.rsplit_once('.'))
        .map(|(driver, _)| driver)
        .collect();
    let settings = drivers.into_iter().flat_map(|driver| {
        let programs = FILTER_PROGRAMS.iter().map(move |program| format!("{FILTER_SECTION}{driver}.{program}="));
        programs.chain([format!("{FILTER_SECTION}{driver}.required=false")])
    });
    Ok(settings.flat_map(|setting| ["-c".to_owned(), setting]).collect())
}

/// The settings to look at the repository at `dir` with, so that nothing its own configuration
/// names is run. Filters set up for the user, as Git LFS is, stay as they are.
async fn looking_only(dir: &Path) -> Result<Vec<String>> {
    let listed =
        command(dir, &[], &["config", "--local", "--includes", "-z", "--name-only", "--get-regexp", "^filter\\."], &[])
            .await?;
    if listed.status.code() == Some(NO_MATCH) && listed.stdout.is_empty() {
        return Ok(Vec::new());
    }
    without_filters(&successful(listed)?)
}

async fn command(dir: &Path, settings: &[String], args: &[&str], paths: &[PathBuf]) -> Result<ProcessOutput> {
    let mut command = Command::new("git");
    command
        .args(["--no-pager", "--literal-pathspecs", "-c", "color.ui=false"])
        .args(NO_PROGRAMS)
        .args(settings)
        .arg("-C")
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
    let mut child = command.spawn().context(tr!(git_not_installed))?;
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
        let root = command(dir, &[], &["rev-parse", "--show-toplevel"], &[]).await?;
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
        let prefix = canonical.strip_prefix(&root).context(tr!(git_outside_repository))?.to_path_buf();
        let settings = looking_only(dir).await?;
        let status = ["status", "--porcelain=v1", "-z", "--branch", "--untracked-files=all", "--", "."];
        let status = successful(command(dir, &settings, &status, &[]).await?)?;
        Repository::parse(root, &status).map(|repository| Some(repository.at_directory(dir.to_path_buf(), prefix)))
    })
    .await
    .context(tr!(git_status_timed_out))?
}

pub async fn run(dir: &Path, tool: Tool, paths: &[PathBuf]) -> Result<Output> {
    ensure!(!tool.needs_selection() || !paths.is_empty(), "{}", tr!(git_select_files_first));
    let paths = paths
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.strip_prefix(dir).map(PathBuf::from).context(tr!(git_path_outside_folder))
            } else {
                Ok(path.clone())
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let (name, args): (_, &[&str]) = match tool {
        Tool::Status => (tr!(git_status_document), &["status", "--short", "--branch", "--untracked-files=normal"]),
        Tool::Diff => (tr!(git_diff_document), &["diff", "--no-ext-diff", "--no-textconv"]),
        Tool::StagedDiff => (tr!(git_staged_diff_document), &["diff", "--cached", "--no-ext-diff", "--no-textconv"]),
        Tool::Log => (tr!(git_log_document), &["log", "-n", LOG_COUNT, "--date=short", "--format=%h %ad %s"]),
        Tool::Stage => ("", &["add"]),
        Tool::Unstage => ("", &["restore", "--staged"]),
    };
    let has_head = if matches!(tool, Tool::Unstage | Tool::Log) {
        command(dir, &[], &["rev-parse", "--verify", "HEAD"], &[]).await?.status.success()
    } else {
        true
    };
    if tool == Tool::Log && !has_head {
        return Ok(Output::Document { name, text: tr!(git_no_commits).as_bytes().to_vec() });
    }
    let args =
        if tool == Tool::Unstage && !has_head { &["rm", "--cached", "--ignore-unmatch", "-r", "-f"][..] } else { args };
    let selected = if tool.needs_selection() { paths.as_slice() } else { &[] };
    let settings = if tool.changes_index() { Vec::new() } else { looking_only(dir).await? };
    let mut text = successful(command(dir, &settings, args, selected).await?)?;
    if tool.changes_index() {
        return Ok(Output::IndexUpdated);
    }
    if text.is_empty() {
        text.extend_from_slice(tr!(git_no_changes).as_bytes());
    }
    Ok(Output::Document { name, text })
}

use std::os::unix::ffi::OsStringExt;

#[cfg(test)]
mod tests;
