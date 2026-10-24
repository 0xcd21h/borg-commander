//! The Docker plugin's link to the `docker` CLI. Going through the CLI rather than the
//! Engine API keeps every setup the user already has working: Docker Desktop, OrbStack,
//! Colima, and remote contexts.

use borg_i18n::tr;
mod model;

use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, ensure};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
};

pub use model::{Action, Container, Details, File, Frames, Health, Mount, Network, State, Stats, parse_size};

const PROGRAM: &str = "docker";
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Stopping waits up to ten seconds for each container before Docker kills it.
const ACTION_TIMEOUT: Duration = Duration::from_secs(120);
/// Copies run as long as they need; the user can watch them in the panel footer.
const COPY_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

fn docker(args: &[&str]) -> Command {
    let mut command = Command::new(PROGRAM);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    command
}

fn spawn_error(error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::NotFound { anyhow!(tr!(docker_missing)) } else { error.into() }
}

async fn read(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes).await?;
    ensure!(bytes.len() as u64 <= OUTPUT_LIMIT, tr!(docker_output_too_large));
    Ok(bytes)
}

async fn run(args: &[&str], timeout: Duration) -> Result<String> {
    tokio::time::timeout(timeout, async {
        let mut child = docker(args).spawn().map_err(spawn_error)?;
        let (stdout, stderr, status) =
            tokio::try_join!(read(child.stdout.take().unwrap()), read(child.stderr.take().unwrap()), async {
                child.wait().await.map_err(anyhow::Error::from)
            })?;
        ensure!(status.success(), "{}", String::from_utf8_lossy(&stderr).trim());
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    })
    .await
    .context(tr!(docker_timed_out))?
}

/// The name of the context the CLI talks to, such as "default", "desktop-linux" or "orbstack".
pub async fn context() -> Result<String> {
    Ok(run(&["context", "show"], QUERY_TIMEOUT).await?.trim().to_owned())
}

/// Every container, running or not.
pub async fn list() -> Result<Vec<Container>> {
    model::parse_list(&run(&["ps", "--all", "--no-trunc", "--format", model::LIST_FORMAT], QUERY_TIMEOUT).await?)
}

pub async fn inspect(id: &str) -> Result<Details> {
    model::parse_details(
        &run(&["inspect", "--type", "container", "--format", model::INSPECT_FORMAT, id], QUERY_TIMEOUT).await?,
    )
}

pub async fn act(action: Action, ids: &[String]) -> Result<()> {
    let mut args = vec![action.verb()];
    args.extend(ids.iter().map(String::as_str));
    run(&args, ACTION_TIMEOUT).await.map(drop)
}

/// Streams resource use of the running containers, calling `sample` about once a second
/// until the stream ends or the future is dropped.
pub async fn watch(mut sample: impl FnMut(Vec<(String, Stats)>)) -> Result<()> {
    let mut child = docker(&["stats", "--no-trunc", "--format", "{{json .}}"]).spawn().map_err(spawn_error)?;
    let errors = tokio::spawn(read(child.stderr.take().unwrap()));
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut frames = Frames::default();
    while let Some(line) = lines.next_line().await? {
        if let Some(frame) = frames.push(&line) {
            sample(frame);
        }
    }
    let status = child.wait().await?;
    let errors = errors.await??;
    ensure!(status.success(), "{}", String::from_utf8_lossy(&errors).trim());
    Ok(())
}

/// Explains why a container's files cannot be reached, in place of Docker's own wording.
fn files_error(error: anyhow::Error) -> anyhow::Error {
    let text = format!("{error:#}");
    if text.contains("executable file not found") || text.contains("no such file or directory: unknown") {
        anyhow!(tr!(docker_no_shell))
    } else if text.contains("is not running") {
        anyhow!(tr!(docker_start_to_browse))
    } else {
        error
    }
}

fn inside(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The entries of a folder inside a running container.
pub async fn files(id: &str, dir: &Path) -> Result<Vec<File>> {
    let output = run(&["exec", id, "sh", "-c", model::LIST_FILES, "sh", &inside(dir)], QUERY_TIMEOUT)
        .await
        .map_err(files_error)?;
    Ok(model::parse_files(&output))
}

/// Copies a path out of a container. An existing host folder `dest` receives it inside, as
/// `docker cp` does; any other `dest` becomes its new name.
pub async fn copy_out(id: &str, source: &Path, dest: &Path) -> Result<()> {
    let from = format!("{id}:{}", inside(source));
    run(&["cp", &from, &dest.to_string_lossy()], COPY_TIMEOUT).await.map(drop)
}

/// Copies a host path into a container, with the same rule for `dest` as [`copy_out`].
pub async fn copy_in(id: &str, source: &Path, dest: &Path) -> Result<()> {
    let to = format!("{id}:{}", inside(dest));
    run(&["cp", &source.to_string_lossy(), &to], COPY_TIMEOUT).await.map(drop)
}

/// The size and modification time (seconds) of each of `paths` that exists in a container.
pub async fn stat(id: &str, paths: &[PathBuf]) -> Result<std::collections::HashMap<PathBuf, (u64, i64)>> {
    let mut args =
        vec!["exec".to_owned(), id.to_owned(), "sh".into(), "-c".into(), model::STAT_PATHS.into(), "sh".into()];
    args.extend(paths.iter().map(|path| inside(path)));
    let output = run(&args.iter().map(String::as_str).collect::<Vec<_>>(), QUERY_TIMEOUT).await.map_err(files_error)?;
    Ok(model::parse_stat(&output))
}

pub async fn remove(id: &str, paths: &[PathBuf]) -> Result<()> {
    let mut args = vec!["exec".to_owned(), id.to_owned(), "rm".into(), "-rf".into(), "--".into()];
    args.extend(paths.iter().map(|path| inside(path)));
    run(&args.iter().map(String::as_str).collect::<Vec<_>>(), ACTION_TIMEOUT).await.map_err(files_error).map(drop)
}

pub async fn mkdir(id: &str, path: &Path) -> Result<()> {
    run(&["exec", id, "mkdir", "-p", "--", &inside(path)], QUERY_TIMEOUT).await.map_err(files_error).map(drop)
}

/// The exit status `READ_FILE` gives for a folder.
const IS_FOLDER: i32 = 3;
/// Reads up to `$3` bytes of the file `$1`, from its end when `$2` is `tail`.
const READ_FILE: &str =
    r#"[ -d "$1" ] && exit 3; if [ "$2" = tail ]; then tail -c "$3" -- "$1"; else head -c "$3" -- "$1"; fi"#;
const WRITE_FILE: &str = r#"cat > "$1""#;

/// Reads up to `limit` bytes of a file in a running container, from its end when `tail`.
/// A folder gives `None`.
pub async fn read_file(id: &str, path: &Path, limit: u64, tail: bool) -> Result<Option<Vec<u8>>> {
    let (from, limit) = (if tail { "tail" } else { "head" }, limit.to_string());
    let output = tokio::time::timeout(
        QUERY_TIMEOUT,
        docker(&["exec", id, "sh", "-c", READ_FILE, "sh", &inside(path), from, &limit]).output(),
    )
    .await
    .context(tr!(docker_timed_out))?
    .map_err(spawn_error)?;
    match output.status.code() {
        Some(IS_FOLDER) => Ok(None),
        Some(0) => Ok(Some(output.stdout)),
        _ => Err(files_error(anyhow!("{}", String::from_utf8_lossy(&output.stderr).trim()))),
    }
}

/// Prints a file's size on a line, then up to `$3` of its bytes from offset `$2`.
const READ_RANGE: &str = r#"[ -f "$1" ] || { echo "$1 is not a file" >&2; exit 1; }; wc -c < "$1"; tail -c +"$(($2 + 1))" -- "$1" | head -c "$3""#;

/// Up to `limit` bytes of a file in a running container from `offset`, and its size.
pub async fn read_range(id: &str, path: &Path, offset: u64, limit: u64) -> Result<(u64, Vec<u8>)> {
    let (offset, limit) = (offset.to_string(), limit.to_string());
    let output = tokio::time::timeout(
        QUERY_TIMEOUT,
        docker(&["exec", id, "sh", "-c", READ_RANGE, "sh", &inside(path), &offset, &limit]).output(),
    )
    .await
    .context(tr!(docker_timed_out))?
    .map_err(spawn_error)?;
    ensure!(output.status.success(), "{}", files_error(anyhow!("{}", String::from_utf8_lossy(&output.stderr).trim())));
    let newline = output.stdout.iter().position(|&byte| byte == b'\n').context("no size")?;
    let size = String::from_utf8_lossy(&output.stdout[..newline]).trim().parse().context("no size")?;
    Ok((size, output.stdout[newline + 1..].to_vec()))
}

/// Writes a file in a running container, replacing what it holds or creating it. The file
/// is written in place, so an existing one keeps its owner and permissions.
pub async fn write_file(id: &str, path: &Path, bytes: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut command = docker(&["exec", "-i", id, "sh", "-c", WRITE_FILE, "sh", &inside(path)]);
    command.stdin(Stdio::piped());
    let mut child = command.spawn().map_err(spawn_error)?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    stdin.write_all(bytes).await?;
    drop(stdin);
    let output =
        tokio::time::timeout(QUERY_TIMEOUT, child.wait_with_output()).await.context(tr!(docker_timed_out))??;
    ensure!(output.status.success(), "{}", files_error(anyhow!("{}", String::from_utf8_lossy(&output.stderr).trim())));
    Ok(())
}

/// Copies one file out of a container into `into`, for viewing, and returns its host path.
pub async fn fetch(id: &str, path: &Path, into: &Path) -> Result<PathBuf> {
    // Files copied out of a container stay readable only by the user.
    tokio::fs::DirBuilder::new().recursive(true).mode(0o700).create(into).await?;
    copy_out(id, path, into).await?;
    Ok(into.join(path.file_name().unwrap_or_default()))
}
