//! Runs a port scan as a job: TCP connections to each port at the mode's pace, a canary port asked
//! again every few seconds, and progress for the job list. The job's control pauses and cancels
//! it, and it ends on its own when the canary falls silent.

use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use borg_core::{Canary, Ending, PortAnswer, PortScanEvent, PortScanRequest, PortScanResult, ScanPace, Tally, Watch};
use borg_jobs::Control;
use tokio::{
    net::TcpStream,
    task::JoinSet,
    time::{Instant, sleep_until, timeout},
};

#[cfg(test)]
mod tests;

/// How often the job list hears how far the scan got.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// How often a waiting scan looks at its control for a pause or a cancel.
const CONTROL_EVERY: Duration = Duration::from_millis(100);

/// Asks one port whether it takes connections.
pub(crate) trait Prober: Send + Sync + 'static {
    fn probe(&self, target: SocketAddr, wait: Duration) -> impl Future<Output = PortAnswer> + Send;
}

/// The real prober: a TCP connection, which a refusal answers as closed and silence as filtered.
pub(crate) struct Connector;

impl Prober for Connector {
    async fn probe(&self, target: SocketAddr, wait: Duration) -> PortAnswer {
        match timeout(wait, TcpStream::connect(target)).await {
            Ok(Ok(_)) => PortAnswer::Open,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => PortAnswer::Closed,
            Ok(Err(_)) | Err(_) => PortAnswer::Filtered,
        }
    }
}

/// What a finished probe was for.
enum Asked {
    /// The port at this place in the request's list.
    Port(usize),
    Canary,
}

/// A random wait from nothing to `spread`, for the gaps of a slow scan.
pub(crate) fn jitter(spread: Duration) -> impl FnMut() -> Duration + Send {
    move || {
        let Some(range) = u64::try_from(spread.as_micros()).ok().filter(|range| *range > 0) else {
            return Duration::ZERO;
        };
        Duration::from_micros(getrandom::u64().unwrap_or_default() % (range + 1))
    }
}

/// Scans the request's ports at `pace` until every one answered, the control cancels it or the
/// canary falls silent. `report` hears the progress.
pub(crate) async fn run<P: Prober>(
    prober: Arc<P>,
    request: &PortScanRequest,
    pace: ScanPace,
    control: &Control,
    mut gap: impl FnMut() -> Duration,
    mut report: impl FnMut(PortScanEvent),
) -> PortScanResult {
    let mut tally = Tally::new(request.ports.clone());
    let mut watch = Watch::default();
    let mut running: JoinSet<(Asked, PortAnswer)> = JoinSet::new();
    let address = request.address;
    let ask = |running: &mut JoinSet<(Asked, PortAnswer)>, asked: Asked, port: u16| {
        let prober = Arc::clone(&prober);
        let target = SocketAddr::new(address, port);
        running.spawn(async move { (asked, prober.probe(target, pace.timeout).await) });
    };
    let mut next = 0;
    let start = Instant::now();
    let mut launch_at = start;
    let mut canary_at = start + pace.canary_every;
    let mut progress_at = start + PROGRESS_EVERY;
    let mut canary_busy = false;
    let mut canary_now = false;
    let ending = loop {
        if control.cancelled() {
            break Ending::Cancelled;
        }
        let now = Instant::now();
        let paused = control.is_paused();
        let room = |running: &JoinSet<(Asked, PortAnswer)>, canary_busy: bool| {
            running.len() - usize::from(canary_busy) < pace.in_flight
        };
        if paused {
            canary_at = now + pace.canary_every;
        } else {
            while next < tally.ports().len() && room(&running, canary_busy) && now >= launch_at {
                ask(&mut running, Asked::Port(next), tally.ports()[next]);
                next += 1;
                launch_at = now + pace.gap + gap();
            }
            if let Some(port) = watch.canary().filter(|_| !canary_busy && (canary_now || now >= canary_at)) {
                ask(&mut running, Asked::Canary, port);
                canary_busy = true;
                canary_now = false;
                canary_at = now + pace.canary_every;
            }
        }
        if now >= progress_at {
            report(PortScanEvent::Progress { done: tally.done(), open: tally.open() });
            progress_at = now + PROGRESS_EVERY;
        }
        if next >= tally.ports().len() && running.len() == usize::from(canary_busy) && !canary_now {
            break Ending::Done;
        }
        let launching = (!paused && next < tally.ports().len() && room(&running, canary_busy)).then_some(launch_at);
        let wake = [Some(progress_at), Some(now + CONTROL_EVERY), launching].into_iter().flatten().min().unwrap_or(now);
        tokio::select! {
            Some(Ok((asked, answer))) = running.join_next() => match asked {
                Asked::Port(index) => {
                    tally.record(index, answer);
                    canary_now |= watch.record(tally.ports()[index], answer);
                }
                Asked::Canary => {
                    canary_busy = false;
                    match watch.canary_answered(answer) {
                        Canary::Fine => {}
                        Canary::Retry => canary_now = true,
                        Canary::Stop { port } => break Ending::RateLimited { port },
                    }
                }
            },
            _ = sleep_until(wake) => {}
        }
    };
    running.abort_all();
    tally.result(ending)
}

/// The address a name stands for: its first IPv4 address, or else its first address.
pub(crate) async fn resolve(name: &str) -> Result<IpAddr, String> {
    const ANY_PORT: u16 = 0;
    let found: Vec<IpAddr> = tokio::net::lookup_host((name, ANY_PORT))
        .await
        .map_err(|error| error.to_string())?
        .map(|socket| socket.ip())
        .collect();
    found
        .iter()
        .find(|address| address.is_ipv4())
        .or(found.first())
        .copied()
        .ok_or_else(|| borg_i18n::tr!(port_scan_no_address).to_owned())
}
