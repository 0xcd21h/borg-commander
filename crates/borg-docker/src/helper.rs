//! A short-lived container that mounts one volume read only, so Borg can show the files of a
//! volume that the engine keeps out of this machine's reach. It has no network, a read-only
//! root, and no capability beyond reading every file, and it stops by itself after a day.

use anyhow::Result;
use std::{process::Stdio, time::Duration};

use crate::{ACTION_TIMEOUT, PROGRAM, QUERY_TIMEOUT, run};

/// BusyBox 1.38.0, pinned to its multi-platform digest so every engine runs the same build.
pub const IMAGE: &str = "busybox@sha256:a46257e06c82ad85242d0d38077d0a52d9896389ca60a1a59d3463db586e5fa4";
/// What Borg calls the image when it asks to download it.
pub const IMAGE_NAME: &str = "busybox";
/// The name the image keeps on the engine, since an image pulled by digest alone has none and
/// would pass for a dangling build.
const TAG: &str = "borg-commander/volume-helper:busybox-1.38.0";
/// Where a helper sees its volume.
pub const ROOT: &str = "/volume";
/// Marks Borg's helpers, with the volume as its value, so they don't count as the volume's users.
pub(crate) const LABEL: &str = "borg-commander.volume-helper";
/// How long a helper lives when Borg never removes it, such as after a crash.
const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
/// Pulling a few megabytes on a slow connection.
const PULL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The one capability a helper keeps: reading files and folders whatever their permissions.
const READ_ANY_FILE: &str = "DAC_READ_SEARCH";

/// Whether the engine has the helper's image, so opening a volume needs no download.
pub async fn present() -> bool {
    run(&["image", "inspect", "--format", "{{.Id}}", IMAGE], QUERY_TIMEOUT).await.is_ok()
}

/// Downloads the image and names it, so it doesn't pass for a dangling build.
pub async fn pull() -> Result<()> {
    run(&["pull", "--quiet", IMAGE], PULL_TIMEOUT).await?;
    run(&["tag", IMAGE, TAG], QUERY_TIMEOUT).await.map(drop)
}

/// Starts a helper for `volume` and gives its ID.
pub async fn start(volume: &str) -> Result<String> {
    let args = start_args(volume);
    let output = run(&args.iter().map(String::as_str).collect::<Vec<_>>(), ACTION_TIMEOUT).await?;
    Ok(output.trim().to_owned())
}

/// Removes a helper; Docker drops it with its volume mount at once.
pub async fn remove(id: &str) -> Result<()> {
    run(&["rm", "--force", id], ACTION_TIMEOUT).await.map(drop)
}

/// Removes helpers before Borg exits, waiting for Docker to finish.
pub fn remove_now(ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let _ = std::process::Command::new(PROGRAM)
        .args(["rm", "--force"])
        .args(ids)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn start_args(volume: &str) -> Vec<String> {
    let flags = [
        "run",
        "--detach",
        "--rm",
        "--pull",
        "never",
        "--network",
        "none",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--cap-add",
        READ_ANY_FILE,
        "--security-opt",
        "no-new-privileges",
    ];
    let mut args: Vec<String> = flags.into_iter().map(str::to_owned).collect();
    args.extend([
        "--label".into(),
        format!("{LABEL}={volume}"),
        "--mount".into(),
        format!("type=volume,source={volume},target={ROOT},readonly"),
        IMAGE.into(),
        "sleep".into(),
        LIFETIME.as_secs().to_string(),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helper_mounts_its_volume_read_only_with_no_network_and_never_pulls() {
        let args = start_args("blog_uploads").join(" ");
        assert!(args.starts_with("run --detach --rm --pull never --network none --read-only"), "{args}");
        assert!(args.contains("--cap-drop ALL --cap-add DAC_READ_SEARCH --security-opt no-new-privileges"));
        assert!(args.contains("--label borg-commander.volume-helper=blog_uploads"));
        assert!(args.contains("--mount type=volume,source=blog_uploads,target=/volume,readonly"));
        assert!(args.ends_with(&format!("{IMAGE} sleep 86400")), "{args}");
    }
}
