//! The borg-relay command: see the library for what the relay does.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, bail};
use tokio::net::TcpListener;

/// Writes a line to the log, the standard error: a closed log must not stop the relay.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

const USAGE: &str = "usage: borg-relay --domain NAME [--email ADDRESS] [--cache DIR] [--listen ADDRESS] [--staging]
       borg-relay --behind-proxy [--listen ADDRESS]
       borg-relay --plain [--listen ADDRESS]
       borg-relay --health [ADDRESS]

  --domain NAME     serve TLS for NAME, with a certificate from Let's Encrypt
  --email ADDRESS   the contact Let's Encrypt writes to about the certificate
  --cache DIR       where the certificate is kept (default /var/lib/borg-relay)
  --listen ADDRESS  where to listen (default [::]:443, or 127.0.0.1:8080 without TLS)
  --staging         use Let's Encrypt's staging service, for trying the setup
  --behind-proxy    no TLS: a proxy in front adds it and reports each client's address
                    (X-Forwarded-For), which limits are counted by
  --plain           no TLS and no proxy, for tests on one machine
  --health          ask the relay at ADDRESS (default 127.0.0.1:8080) whether it runs";
const DEFAULT_CACHE: &str = "/var/lib/borg-relay";
const TLS_LISTEN: &str = "[::]:443";
const PLAIN_LISTEN: &str = "127.0.0.1:8080";

struct Args {
    domain: Option<String>,
    email: Option<String>,
    cache: PathBuf,
    listen: Option<SocketAddr>,
    staging: bool,
    plain: bool,
    behind_proxy: bool,
}

fn args() -> Result<Args> {
    let mut args = Args {
        domain: None,
        email: None,
        cache: PathBuf::from(DEFAULT_CACHE),
        listen: None,
        staging: false,
        plain: false,
        behind_proxy: false,
    };
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| rest.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--domain" => args.domain = Some(value("--domain")?),
            "--email" => args.email = Some(value("--email")?),
            "--cache" => args.cache = PathBuf::from(value("--cache")?),
            "--listen" => {
                args.listen = Some(value("--listen")?.parse().context("--listen takes an address such as 0.0.0.0:443")?)
            }
            "--staging" => args.staging = true,
            "--plain" => args.plain = true,
            "--behind-proxy" => {
                args.plain = true;
                args.behind_proxy = true;
            }
            "--health" => {
                let address = rest.next().unwrap_or_else(|| PLAIN_LISTEN.to_owned());
                std::process::exit(if borg_relay::health(&address) { 0 } else { 1 });
            }
            "--version" | "-V" => {
                println!("borg-relay {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}"),
        }
    }
    if args.domain.is_none() && !args.plain {
        bail!("give --domain, or --behind-proxy to serve behind a proxy that adds TLS");
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match args() {
        Ok(args) => args,
        Err(error) => {
            log!("borg-relay: {error:#}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match serve(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log!("borg-relay: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(args: Args) -> Result<()> {
    let default = if args.plain { PLAIN_LISTEN } else { TLS_LISTEN };
    let address = args.listen.unwrap_or_else(|| default.parse().expect("the default address is valid"));
    let listener = TcpListener::bind(address).await.with_context(|| format!("could not listen on {address}"))?;
    let tls = match (&args.domain, args.plain) {
        (Some(domain), false) => {
            Some(borg_relay::acme(domain, args.email.as_deref(), args.cache.clone(), args.staging))
        }
        _ => None,
    };
    let bound = listener.local_addr().unwrap_or(address);
    log!(
        "borg-relay {} listening on {bound}{}",
        env!("CARGO_PKG_VERSION"),
        if tls.is_some() { " with TLS" } else { "" }
    );
    borg_relay::serve(listener, borg_relay::Options { tls, behind_proxy: args.behind_proxy }).await;
    Ok(())
}
