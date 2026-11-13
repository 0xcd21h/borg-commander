//! The `borg` command. Without arguments it opens Borg Commander in this terminal, or attaches to
//! its session in the background service; its subcommands manage sessions and the service, pair
//! computers, copy files between them and serve Borg to agents.

mod activity;
mod admin;
mod agent;
mod app;
mod archive;
mod backup;
mod code_signature;
mod config;
mod delete_check;
mod docker;
mod document;
mod edit_file;
mod entropy_scan;
mod host_asks;
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
mod program;
mod relaunch;
mod remote;
mod remote_console;
mod remote_copy;
mod remote_fs;
mod remote_names;
mod rule_scan;
mod search;
mod service;
mod service_install;
mod session;
mod sessions;
mod size_cache;
mod start_window;
mod startup_items;
mod storage;
mod store;
mod subfolders;
mod tools;
mod update;
mod watch;

use std::{env, ffi::OsString, fs, io::stdout, os::fd::OwnedFd, process::ExitCode};

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

use crate::{
    app::{App, Exit},
    launch::Launch,
    relaunch::Carried,
};

const USAGE_ERROR: u8 = 2;
/// The argument that opens a session by its name or ID.
const SESSION: &str = "--session";
/// The exit code of a Borg that could not run the command it leads a terminal for, as a shell
/// gives for a command it cannot run.
const LEAD_FAILED: u8 = 127;

fn main() -> Result<ExitCode> {
    let args: Vec<OsString> = env::args_os().collect();
    if let [_, flag, program, rest @ ..] = args.as_slice()
        && flag == borg_term::LEAD
    {
        let error = borg_term::lead(program, rest);
        set_language();
        eprintln!("{}", tr!(lead_failed, program = program.to_string_lossy(), error = error));
        return Ok(ExitCode::from(LEAD_FAILED));
    }
    program::remember();
    start(args)
}

#[tokio::main]
async fn start(args: Vec<OsString>) -> Result<ExitCode> {
    set_language();
    let given = args.get(1..).unwrap_or_default();
    let (given, carried) = match relaunch::split_resume(given) {
        Some(rest) => (rest, resume()),
        None => (given, None),
    };
    let args: Vec<String> = given.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
    match args.first().map(String::as_str) {
        None => open(None, None, carried).await,
        Some(SESSION) => match args.get(1) {
            Some(name) => open(Some(name.clone()), None, carried).await,
            None => Ok(usage("usage: borg --session NAME")),
        },
        Some("--remote") => match args.get(1) {
            Some(target) => open(None, Some(target.clone()), None).await,
            None => Ok(usage("usage: borg --remote MACHINE:SESSION")),
        },
        Some(command) => subcommand(command, &args[1..], carried).await,
    }
}

/// What the previous Borg carried into this one, which starts afresh when it cannot be had.
fn resume() -> Option<Carried> {
    relaunch::resume().inspect_err(|error| eprintln!("{}", tr!(resume_failed, error = error))).ok()
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
async fn subcommand(command: &str, args: &[String], carried: Option<Carried>) -> Result<ExitCode> {
    if matches!(command, "pair" | "get" | "put" | "mirror") {
        link::ask_in_terminal();
    }
    Ok(match command {
        "sessions" => {
            let Some(state) = session::StateDir::locate() else {
                eprintln!("borg has no folder for sessions: set HOME or BORG_STATE_DIR");
                return Ok(ExitCode::FAILURE);
            };
            finished("borg sessions", sessions::command(&state, args))
        }
        "service" => finished("borg service", service::command(args, carried).await),
        service::GUARD => finished("borg --guard-update", service::guard(args).await),
        "storage" => match storage::command(args) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("borg storage: {error:#}");
                ExitCode::from(USAGE_ERROR)
            }
        },
        "mirror" => {
            let Some(target) = args.first() else { return Ok(usage("usage: borg mirror SERVER:SESSION [--take]")) };
            let force = args.get(1).map(String::as_str) == Some("--take");
            finished("borg mirror", mirror::command(target, force).await)
        }
        "--inspect-process" => {
            activity::helper(args).await?;
            ExitCode::SUCCESS
        }
        "--capture-traffic" => {
            network::capture_helper(args).await?;
            ExitCode::SUCCESS
        }
        "--watch-root" => finished("borg --watch-root", watch::root_helper(args).await),
        "mcp" => return mcp(args).await,
        "pair" => finished("borg pair", link::pair_command(args).await),
        "get" => finished("borg get", link::get_command(args).await),
        "put" => finished("borg put", link::put_command(args).await),
        "unlock" => finished("borg unlock", link::unlock_command(args).await),
        "lock" => finished("borg lock", link::lock_command()),
        #[cfg(target_os = "linux")]
        "--key-agent" => finished("borg --key-agent", link::key_agent_command(args).await),
        "--version" | "-V" => {
            println!("{}{}", update::VERSION_NAME, update::VERSION);
            ExitCode::SUCCESS
        }
        "--help" | "-h" => {
            println!("{}", tr!(cli_usage));
            ExitCode::SUCCESS
        }
        "update" => match tokio::task::spawn_blocking(update::run).await? {
            Ok(()) => {
                service::after_update().await;
                ExitCode::SUCCESS
            }
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

/// Opens Borg in this terminal, on the session asked for or a remote Borg's, or on the one
/// chosen in the start window. A session the background service keeps running is attached to
/// instead; any other registers with the service while it runs. `carried` is what the previous
/// Borg handed over when an update started this one in its place.
async fn open(
    requested_session: Option<String>,
    requested_remote: Option<String>,
    carried: Option<Carried>,
) -> Result<ExitCode> {
    if env::var_os(borg_term::SESSION_MARKER).is_some() {
        eprintln!("{}", tr!(cli_already_running));
        return Ok(ExitCode::FAILURE);
    }
    let resumed = carried.and_then(|carried| {
        app::unpack(carried, tr!(relaunched, version = update::VERSION))
            .inspect_err(|error| eprintln!("{}", tr!(resume_failed, error = error)))
            .ok()
    });
    let remote = match requested_remote {
        Some(target) => Some(remote::resolve(&target).await?),
        None => None,
    };
    let service_child = env::var_os(service::SERVICE_CHILD).is_some();
    let fresh = resumed.is_none() && remote.is_none() && !service_child;
    let starts_service =
        config::ConfigDir::locate().and_then(|dir| dir.load().ok()).is_none_or(|config| config.start_service());
    if fresh && starts_service && session::StateDir::locate().is_some() {
        service::start_on_demand().await;
    }
    let selected_id = if let Some(target) = &remote {
        Some(remote::viewer_id(target))
    } else if let Some(state) = session::StateDir::locate() {
        let chosen = match requested_session.as_deref() {
            Some(id) if service_child || resumed.is_some() => Some(sessions::find(&state, id)?),
            requested => sessions::choose(&state, requested).await?,
        };
        let Some(session) = chosen else { return Ok(ExitCode::SUCCESS) };
        Some(session.id)
    } else {
        None
    };
    if fresh
        && let Some(session) = &selected_id
        && service::running_sessions().await.contains(session)
    {
        service::attach_session(session, false).await?;
        return Ok(ExitCode::SUCCESS);
    }
    let (carried_lock, mut resumed) = resumed.unzip();
    let session_lock = match (carried_lock.flatten(), session::StateDir::locate().zip(selected_id.as_ref())) {
        (Some(lock), _) => Some(lock),
        (None, Some((state, session))) => match sessions::lock(&state, session) {
            Ok(lock) => Some(lock),
            Err(sessions::LockError::Busy(busy)) => {
                eprintln!("{}", busy.explained());
                return Ok(ExitCode::FAILURE);
            }
            Err(sessions::LockError::Io(error)) => return Err(error.into()),
        },
        (None, None) => None,
    };
    let launch = Launch::detect(selected_id.as_deref(), remote)?;
    loop {
        let mut terminal = ratatui::try_init()?;
        let modes = TerminalModes::enter()?;
        let detected = launch::terminal(launch.graphics);
        let exit = App::start(&launch, terminal.size()?, detected, resumed.take())?.run(&mut terminal).await;
        let Ok(Exit::Relaunch(shell)) = exit else {
            drop(modes);
            return exit.map(|_| ExitCode::SUCCESS);
        };
        modes.hand_over();
        let lock = session_lock.as_ref().map(fs::File::try_clone).transpose()?.map(OwnedFd::from);
        let carried = app::pack(lock, shell)?;
        let args: Vec<OsString> = selected_id.iter().flat_map(|id| [SESSION.into(), id.into()]).collect();
        let Err(error) = relaunch::exec(&args, &carried);
        resumed = Some(app::unpack(carried, format!("{error:#}"))?.1);
    }
}

/// The terminal modes Borg sets for itself, set back when it ends.
struct TerminalModes {
    enhanced: bool,
    screen: Screen,
}

/// What happens to Borg's screen when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// The terminal shows what it did before Borg.
    Restore,
    /// The newer Borg starting in this terminal takes the screen as it is.
    Keep,
}

impl TerminalModes {
    fn enter() -> Result<Self> {
        let modes = Self { enhanced: supports_keyboard_enhancement().unwrap_or(false), screen: Screen::Restore };
        execute!(stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        if modes.enhanced {
            execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
        }
        Ok(modes)
    }

    /// Sets the modes back for the newer Borg that starts in this terminal next and sets them
    /// again, keeping the screen it draws on.
    fn hand_over(mut self) {
        self.screen = Screen::Keep;
    }
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout(), EndSynchronizedUpdate, DisableBracketedPaste, DisableMouseCapture);
        match self.screen {
            Screen::Restore => ratatui::restore(),
            Screen::Keep => {
                let _ = crossterm::terminal::disable_raw_mode();
            }
        }
    }
}
