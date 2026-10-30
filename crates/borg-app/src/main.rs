use borg_i18n::tr;
mod activity;
mod agent;
mod app;
mod config;
mod docker;
mod document;
mod edit_file;
mod index;
mod launch;
mod link;
mod memory_census;
mod memory_search;
mod memory_strings;
mod mirror;
mod native_mirror;
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
mod update;

use std::{env, io::stdout, process::ExitCode};

use anyhow::Result;
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
    let configured = config::ConfigDir::locate().and_then(|dir| dir.load().ok()).and_then(|config| config.language);
    let requested = env::var("BORG_LANGUAGE").ok().or(configured);
    let system = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .find(|value| !value.is_empty());
    borg_i18n::set_locale(borg_i18n::detect(requested.as_deref(), system.as_deref()));
    let mut requested_session = None;
    let mut requested_remote = None;
    match env::args().nth(1).as_deref() {
        None => {}
        Some("--session") => {
            let Some(name) = env::args().nth(2) else {
                eprintln!("usage: borg --session NAME");
                return Ok(ExitCode::from(USAGE_ERROR));
            };
            requested_session = Some(name);
        }
        Some("--remote") => {
            let Some(target) = env::args().nth(2) else {
                eprintln!("usage: borg --remote MACHINE:SESSION");
                return Ok(ExitCode::from(USAGE_ERROR));
            };
            requested_remote = Some(target);
        }
        Some("sessions") => {
            let Some(state) = session::StateDir::locate() else {
                eprintln!("borg has no folder for sessions: set HOME or BORG_STATE_DIR");
                return Ok(ExitCode::FAILURE);
            };
            let args: Vec<String> = env::args().skip(2).collect();
            return Ok(match sessions::command(&state, &args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("borg sessions: {error:#}");
                    ExitCode::FAILURE
                }
            });
        }
        Some("service") => {
            let args: Vec<String> = env::args().skip(2).collect();
            return Ok(match service::command(&args).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("borg service: {error:#}");
                    ExitCode::FAILURE
                }
            });
        }
        Some("mirror") => {
            let Some(target) = env::args().nth(2) else {
                eprintln!("usage: borg mirror SERVER:SESSION [--take]");
                return Ok(ExitCode::from(USAGE_ERROR));
            };
            let force = env::args().nth(3).as_deref() == Some("--take");
            return Ok(match mirror::command(&target, force).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("borg mirror: {error:#}");
                    ExitCode::FAILURE
                }
            });
        }
        Some("--inspect-process") => {
            activity::helper(&env::args().skip(2).collect::<Vec<_>>()).await?;
            return Ok(ExitCode::SUCCESS);
        }
        Some("mcp") => {
            match env::args().nth(2).as_deref() {
                Some("setup") => println!("{}", agent::setup(env::args().nth(3).as_deref())),
                Some("remote") => {
                    let Some(name) = env::args().nth(3) else {
                        eprintln!("usage: borg mcp remote NAME");
                        return Ok(ExitCode::from(USAGE_ERROR));
                    };
                    let state = session::StateDir::locate().map(|dir| dir.path().to_path_buf());
                    let Some(state) = state else {
                        eprintln!("borg has no folder for its keys: set HOME or BORG_STATE_DIR");
                        return Ok(ExitCode::FAILURE);
                    };
                    agent::serve_remote(state, &name).await?;
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
            return Ok(ExitCode::SUCCESS);
        }
        Some(command @ ("pair" | "get" | "put")) => {
            let args: Vec<String> = env::args().skip(2).collect();
            let done = match command {
                "pair" => link::pair_command(&args).await,
                "get" => link::get_command(&args).await,
                _ => link::put_command(&args).await,
            };
            return Ok(match done {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("borg {command}: {error:#}");
                    ExitCode::FAILURE
                }
            });
        }
        Some("--version" | "-V") => {
            println!("borg {}", update::VERSION);
            return Ok(ExitCode::SUCCESS);
        }
        Some("--help" | "-h") => {
            println!("{}", tr!(cli_usage));
            return Ok(ExitCode::SUCCESS);
        }
        Some("update") => {
            return Ok(match tokio::task::spawn_blocking(update::run).await? {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{}", tr!(cli_update_failed, error = error));
                    ExitCode::FAILURE
                }
            });
        }
        Some(other) => {
            eprintln!("{}", tr!(cli_unknown_argument, other = other, USAGE = tr!(cli_usage)));
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    }
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
