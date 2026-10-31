//! `borg pair`, `borg get` and `borg put`.

use super::{Remote, download_in_session, forget_server, pair, server, servers, state_dir, upload_in_session};
use anyhow::{Context, Result, bail};
use borg_link::{fingerprint, unhex};
use std::path::{Path, PathBuf};

/// `borg pair [LINK | --forget NAME]`: pairs this computer with the borg that shows LINK,
/// or lists the paired ones, or forgets one.
pub async fn pair_command(args: &[String]) -> Result<()> {
    let state = state_dir()?;
    match args {
        [] => {
            let servers = servers(&state);
            if servers.is_empty() {
                println!("No borg is paired with this computer. In borg on a server, press F1 then C for a link.");
            }
            for server in servers {
                println!("{}  {}  {}", server.name, server.relay, fingerprint(&unhex(&server.key).unwrap_or_default()));
            }
            Ok(())
        }
        [flag, name] if flag == "--forget" => {
            if forget_server(&state, name)? {
                println!("Forgot {name}.");
                Ok(())
            } else {
                bail!("no server named {name} is paired")
            }
        }
        [link] => {
            println!("Pairing through borg cloud…");
            let server = pair(&state, link, |code| {
                let [first, second] = code.rows(true);
                println!("The pairing code is\n\n  {first}\n  {second}\n");
                println!("Approve this computer in borg on the server when it shows the same code.");
            })
            .await?;
            let program = crate::agent::program_path();
            let args = ["mcp", "add", &format!("borg-{}", server.name), "--", &program, "mcp", "remote", &server.name];
            let shown: Vec<String> = args.iter().map(|arg| borg_core::shell_quote(arg)).collect();
            println!("\nPaired with {}.\n\nConnect Claude Code to it:\n\n  claude {}\n", server.name, shown.join(" "));
            offer_claude(&args);
            Ok(())
        }
        _ => bail!("usage: borg pair [LINK | --forget NAME]"),
    }
}

/// Offers to run `claude mcp add` for the user, when Claude Code is here and someone answers.
pub(super) fn offer_claude(args: &[&str]) {
    use std::io::{BufRead, IsTerminal, Write};
    let found = std::process::Command::new("claude").arg("--version").output().is_ok_and(|out| out.status.success());
    if !found || !std::io::stdin().is_terminal() {
        return;
    }
    print!("Add it to Claude Code now? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("y") {
        return;
    }
    match std::process::Command::new("claude").args(args).status() {
        Ok(status) if status.success() => println!("Added. Claude Code reaches it in new sessions."),
        _ => println!("Claude Code did not take it; run the line above yourself."),
    }
}

/// Splits `NAME:PATH`, the form `borg get` and `borg put` name a file on a paired server by.
pub(super) fn on_server(text: &str) -> Result<(&str, &str)> {
    text.split_once(':')
        .filter(|(name, path)| !name.is_empty() && !path.is_empty())
        .context("name the file as NAME:PATH")
}

pub(super) fn progress(done: u64, total: u64) {
    use std::io::Write as _;
    let percent = (done * 100).checked_div(total).unwrap_or(100);
    eprint!("\r{percent:>3}%  {} of {}", borg_ui::size_text(done), borg_ui::size_text(total));
    let _ = std::io::stderr().flush();
}

/// `borg get NAME:PATH [TO]`: copies a file from a paired server to this computer.
pub async fn get_command(args: &[String]) -> Result<()> {
    let [from, rest @ ..] = args else { bail!("usage: borg get NAME:PATH [TO]") };
    let (target, path) = on_server(from)?;
    let (name, session) = target.split_once('/').map_or((target, None), |(name, session)| (name, Some(session)));
    let state = state_dir()?;
    let server = server(&state, name)?;
    let mut remote = Remote::open(&state, &server).await?;
    let file_name = Path::new(path).file_name().context("the path names no file")?.to_os_string();
    let mut to = rest.first().map_or_else(|| PathBuf::from("."), PathBuf::from);
    if to.is_dir() {
        to = to.join(file_name);
    }
    let size = download_in_session(&mut remote, session, path, &to, progress).await?;
    eprintln!();
    println!("Downloaded {name}:{path} ({}) to {}", borg_ui::size_text(size), to.display());
    Ok(())
}

/// `borg put FROM NAME:PATH`: copies a file from this computer to a paired server, after the
/// user approves it in borg there.
pub async fn put_command(args: &[String]) -> Result<()> {
    let [from, to] = args else { bail!("usage: borg put FROM NAME:PATH") };
    let (target, path) = on_server(to)?;
    let (name, session) = target.split_once('/').map_or((target, None), |(name, session)| (name, Some(session)));
    let state = state_dir()?;
    let server = server(&state, name)?;
    let mut remote = Remote::open(&state, &server).await?;
    println!("Approve the upload in borg on {name}…");
    let size = upload_in_session(&mut remote, session, Path::new(from), path, progress).await?;
    eprintln!();
    println!("Uploaded {from} ({}) to {name}:{path}", borg_ui::size_text(size));
    Ok(())
}
