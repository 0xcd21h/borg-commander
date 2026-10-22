//! The Docker plugin's link to the `docker` CLI. Going through the CLI rather than the
//! Engine API keeps every setup the user already has working: Docker Desktop, OrbStack,
//! Colima, and remote contexts.

use borg_i18n::tr;
mod model;

use std::{io, process::Stdio, time::Duration};

use anyhow::{Context, Result, anyhow, ensure};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
};

pub use model::{Action, Container, Details, Frames, Health, Mount, Network, State, Stats, parse_size};

const PROGRAM: &str = "docker";
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Stopping waits up to ten seconds for each container before Docker kills it.
const ACTION_TIMEOUT: Duration = Duration::from_secs(120);

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
