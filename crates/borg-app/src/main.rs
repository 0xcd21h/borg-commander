//! The `borg` command. Without arguments it opens Borg Commander in this terminal, or attaches to
//! its session in the background service; its subcommands manage sessions and the service, pair
//! computers, copy files between them and serve Borg to agents.

mod activity;
mod admin;
mod agent;
mod app;
mod backup;
mod code_signature;
mod config;
mod docker;
mod document;
mod edit_file;
mod index;
mod integrity;
mod launch;
mod link;
mod memory_census;
mod memory_search;
mod memory_strings;
mod mirror;
mod native_mirror;
mod network;
mod peek;
mod plugins;
mod preview;
mod remote;
mod remote_console;
mod remote_copy;
mod remote_fs;
mod remote_names;
mod search;
mod service;
mod service_install;
mod session;
mod sessions;
mod size_cache;
mod store;
mod subfolders;
mod update;

use std::{env, io::stdout, process::ExitCode};

use anyhow::Result;
use borg_i18n::tr;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EndSynchronizedUpdate, supports_keyboard_enhancement},
};
use ratatui::DefaultTerminal;

use crate::{app::App, launch::Launch};

const USAGE_ERROR: u8 = 2;

#[tokio::main]
async fn main() -> Result<ExitCode> {
    set_language();
    match env::args().nth(1).as_deref() {
        None => open(None, None).await,
        Some("--session") => match env::args().nth(2) {
            Some(name) => open(Some(name), None).await,
            None => Ok(usage("usage: borg --session NAME")),
        },
        Some("--remote") => match env::args().nth(2) {
            Some(target) => open(None, Some(target)).await,
            None => Ok(usage("usage: borg --remote MACHINE:SESSION")),
        },
        Some(command) => subcommand(command).await,
    }
}

/// Speaks the language the settings or `BORG_LANGUAGE` ask for, or else the system's.
fn set_language() {
    let configured = config::ConfigDir::locate().and_then(|dir| dir.load().ok()).and_then(|config| config.language);
    let requested = env::var("BORG_LANGUAGE").ok().or(configured);
    let system = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .find(|value| !value.is_empty());
    borg_i18n::set_locale(borg_i18n::detect(requested.as_deref(), system.as_deref()));
}

/// A subcommand, which does its work and exits.
async fn subcommand(command: &str) -> Result<ExitCode> {
    let args: Vec<String> = env::args().skip(2).collect();
    Ok(match command {
        "sessions" => {
            let Some(state) = session::StateDir::locate() else {
                eprintln!("borg has no folder for sessions: set HOME or BORG_STATE_DIR");
                return Ok(ExitCode::FAILURE);
            };
            finished("borg sessions", sessions::command(&state, &args))
        }
        "service" => finished("borg service", service::command(&args).await),
        "mirror" => {
            let Some(target) = args.first() else { return Ok(usage("usage: borg mirror SERVER:SESSION [--take]")) };
            let force = args.get(1).map(String::as_str) == Some("--take");
            finished("borg mirror", mirror::command(target, force).await)
        }
        "--inspect-process" => {
            activity::helper(&args).await?;
            ExitCode::SUCCESS
        }
        "mcp" => return mcp(&args).await,
        "pair" => finished("borg pair", link::pair_command(&args).await),
        "get" => finished("borg get", link::get_command(&args).await),
        "put" => finished("borg put", link::put_command(&args).await),
        "--version" | "-V" => {
            println!("borg {}", update::VERSION);
            ExitCode::SUCCESS
        }
        "--help" | "-h" => {
            println!("{}", tr!(cli_usage));
            ExitCode::SUCCESS
        }
        "update" => match tokio::task::spawn_blocking(update::run).await? {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}", tr!(cli_update_failed, error = error));
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("{}", tr!(cli_unknown_argument, other = other, USAGE = tr!(cli_usage)));
            ExitCode::from(USAGE_ERROR)
        }
    })
}

/// `borg mcp`: serves Borg to an agent over standard input and output, prints the setup for
/// one, or serves a paired computer's Borg.
async fn mcp(args: &[String]) -> Result<ExitCode> {
    match args.first().map(String::as_str) {
        Some("setup") => println!("{}", agent::setup(args.get(1).map(String::as_str))),
        Some("remote") => {
            let Some(name) = args.get(1) else { return Ok(usage("usage: borg mcp remote NAME")) };
            let Some(state) = session::StateDir::locate().map(|dir| dir.path().to_path_buf()) else {
                eprintln!("borg has no folder for its keys: set HOME or BORG_STATE_DIR");
                return Ok(ExitCode::FAILURE);
            };
            agent::serve_remote(state, name).await?;
        }
        _ => {
            let local = launch::runtime_dir();
            let runtime = if agent::Session::open(&local, None).await.is_some() {
                local
            } else if service::available().await {
                service::runtime_dir().unwrap_or_else(|_| launch::runtime_dir())
            } else {
                local
            };
            agent::serve(runtime).await?
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The exit code for a subcommand's result, saying what went wrong after its name.
fn finished(name: &str, result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{name}: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn usage(text: &str) -> ExitCode {
    eprintln!("{text}");
    ExitCode::from(USAGE_ERROR)
}

/// Opens Borg in this terminal, on the session asked for or a remote Borg's. A session the
/// background service keeps is attached to instead.
async fn open(requested_session: Option<String>, requested_remote: Option<String>) -> Result<ExitCode> {
    if env::var_os(borg_term::SESSION_MARKER).is_some() {
        eprintln!("{}", tr!(cli_already_running));
        return Ok(ExitCode::FAILURE);
    }
    let remote = match requested_remote {
        Some(target) => Some(remote::resolve(&target).await?),
        None => None,
    };
    let selected_id = if let Some(target) = &remote {
        Some(remote::viewer_id(target))
    } else {
        session::StateDir::locate()
            .map(|state| sessions::choose(&state, requested_session.as_deref()))
            .transpose()?
            .map(|session| session.id)
    };
    if remote.is_none()
        && env::var_os("BORG_SERVICE_CHILD").is_none()
        && service::available().await
        && let Some(session) = &selected_id
    {
        service::attach_session(session, false).await?;
        return Ok(ExitCode::SUCCESS);
    }
    let _session_lock = session::StateDir::locate()
        .zip(selected_id.as_ref())
        .map(|(state, session)| sessions::lock(&state, session))
        .transpose()?;
    let launch = Launch::detect(selected_id.as_deref(), remote)?;
    let mut terminal = ratatui::try_init()?;

    let modes = TerminalModes { enhanced: supports_keyboard_enhancement().unwrap_or(false) };
    execute!(stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    if modes.enhanced {
        execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    }

    let detected = launch::terminal(launch.graphics);
    let result = run(&launch, &mut terminal, detected).await;
    drop(modes);
    result.map(|()| ExitCode::SUCCESS)
}

struct TerminalModes {
    enhanced: bool,
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout(), EndSynchronizedUpdate, DisableBracketedPaste, DisableMouseCapture);
        ratatui::restore();
    }
}

async fn run(launch: &Launch, terminal: &mut DefaultTerminal, detected: launch::Terminal) -> Result<()> {
    App::start(launch, terminal.size()?, detected)?.run(terminal).await
}
