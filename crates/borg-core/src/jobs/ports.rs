//! The port scan as a job: its gauge counts the ports tried, its details name the host, the
//! mode and the open ports, and opening it once it ends shows the open ports.

use std::time::Instant;

use borg_i18n::tr;

use super::{Detail, Gauge, How, JobKind, Link, Reveal, Unit, Value, add, advance, end};
use crate::{AppState, Ending, PortScanRecord, PortScanRequest, ScanSpeed};

/// What the job says about the scan it stands for.
pub(crate) struct Scanning {
    target: String,
    speed: ScanSpeed,
    ports: u64,
}

impl Scanning {
    pub(crate) fn of(request: &PortScanRequest) -> Self {
        Self { target: request.label(), speed: request.speed, ports: request.ports.len() as u64 }
    }
}

fn details(scanning: &Scanning, done: u64, open: u64) -> Vec<Detail> {
    vec![
        Detail::new(tr!(@ port_scan_host_label), Value::Text(scanning.target.clone())),
        Detail::new(tr!(@ port_scan_mode_label), Value::Text(scanning.speed.label().to_owned())),
        Detail::new(tr!(@ port_scan_ports_label), Value::Of { done, total: scanning.ports, unit: Unit::Ports }),
        Detail::new(tr!(@ job_detail_open), Value::Number(open)),
    ]
}

/// Adds the scan of `request`, shown in the active panel, and returns its id.
pub(crate) fn begin_port_scan(state: &mut AppState, request: &PortScanRequest, now: Instant) -> u64 {
    let scanning = Scanning::of(request);
    let id = add(state, JobKind::PortScan, scanning.target.clone(), state.active, Link::Runner, now);
    port_scan_progress(state, id, &scanning, 0, 0);
    id
}

/// How many ports of the scan answered, and how many are open.
pub(crate) fn port_scan_progress(state: &mut AppState, id: u64, scanning: &Scanning, done: usize, open: usize) {
    let (done, open) = (done as u64, open as u64);
    advance(state, id, Gauge::of(done, scanning.ports, Unit::Ports), details(scanning, done, open));
}

/// How a scan that ended as `ending` finished, and the line that says so.
fn outcome(ending: &Ending, open: usize) -> (How, String) {
    match ending {
        Ending::Done => (How::Done, tr!(job_port_scan_done, open = open)),
        Ending::Cancelled => (How::Cancelled, tr!(job_port_scan_stopped, open = open)),
        Ending::RateLimited { .. } => (How::Failed, tr!(job_port_scan_limited, open = open)),
        Ending::Failed(error) => (How::Failed, error.clone()),
    }
}

/// Ends the scan's job with what `record` found. Opening it then shows the open ports, unless
/// the scan failed before it found any.
pub(crate) fn end_port_scan(state: &mut AppState, id: u64, record: &PortScanRecord, now: Instant) {
    let (how, summary) = outcome(&record.ending, record.open.len());
    if !matches!(record.ending, Ending::Failed(_))
        && let Some(job) = super::find_mut(state, id)
    {
        job.reveal = Some(Reveal::PortScan(Box::new(record.clone())));
    }
    end(state, id, how, summary, now);
}
