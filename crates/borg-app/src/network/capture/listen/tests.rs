use super::*;
use borg_core::{Heard, ListenProtocol};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::Mutex};

const ETHERNET: u32 = 1;
/// The file the stand-in tcpdump creates at `$READY` once it wrote its packets.
const READY: &str = "ready";
/// How often a test looks whether the stand-in tcpdump wrote its packets.
const READY_POLL: Duration = Duration::from_millis(10);
/// How long the stand-in tcpdump has to write its packets, however busy the machine is.
const READY_LIMIT: Duration = Duration::from_secs(60);
/// How long a timed listen in these tests lasts.
const WINDOW: Duration = Duration::from_secs(1);

/// A pcap stream holding one ARP request from 192.0.2.31 at 02:00:00:00:00:31.
fn arp_pcap() -> Vec<u8> {
    let mut frame = vec![0xff; 6];
    frame.extend([0x02, 0, 0, 0, 0, 0x31, 0x08, 0x06]);
    frame.extend([0, 1, 0x08, 0, 6, 4, 0, 1]);
    frame.extend([0x02, 0, 0, 0, 0, 0x31, 192, 0, 2, 31]);
    frame.extend([0, 0, 0, 0, 0, 0, 192, 0, 2, 1]);
    let length = (frame.len() as u32).to_le_bytes();
    let mut pcap = Vec::new();
    pcap.extend(0xa1b2_c3d4_u32.to_le_bytes());
    pcap.extend(2_u16.to_le_bytes());
    pcap.extend(4_u16.to_le_bytes());
    pcap.extend([0; 8]);
    pcap.extend(65_535_u32.to_le_bytes());
    pcap.extend(ETHERNET.to_le_bytes());
    pcap.extend([0; 8]);
    pcap.extend(length);
    pcap.extend(length);
    pcap.extend(frame);
    pcap
}

/// A stand-in for tcpdump that runs `script` with the pcap stream at `$PCAP` and the file that
/// says it wrote it at `$READY`.
fn fake_tcpdump(folder: &Path, script: &str) -> PathBuf {
    let pcap = folder.join("heard.pcap");
    std::fs::write(&pcap, arp_pcap()).unwrap();
    let ready = folder.join(READY);
    let program = folder.join("tcpdump");
    let variables = format!("PCAP='{}'\nREADY='{}'", pcap.display(), ready.display());
    std::fs::write(&program, format!("#!/bin/sh\n{variables}\n{script}\n")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// Keeps a paused clock still until what it gives is dropped, so only the test moves it,
/// however long the stand-in tcpdump takes to start.
fn hold_clock() -> std::sync::mpsc::Sender<()> {
    let (release, held) = std::sync::mpsc::channel();
    tokio::task::spawn_blocking(move || held.recv());
    release
}

/// Waits until the stand-in tcpdump in `folder` says it wrote its packets, and tells whether
/// it did within `READY_LIMIT`.
async fn written(folder: &Path) -> bool {
    let ready = folder.join(READY);
    let waited = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        while !ready.exists() {
            if started.elapsed() > READY_LIMIT {
                return false;
            }
            std::thread::sleep(READY_POLL);
        }
        true
    });
    waited.await.unwrap()
}

fn request(seconds: Option<u64>) -> ListenRequest {
    ListenRequest { interface: "en0".into(), seconds, administrator: false }
}

/// Runs the listener on `program` and gives its result with every event it reported.
async fn listened(
    program: &Path,
    request: ListenRequest,
    control: Arc<Control>,
) -> (Result<(), CaptureFailure>, Vec<ListenEvent>) {
    let events = Mutex::new(Vec::new());
    let result = run(program, &request, control, |event| events.lock().unwrap().push(event)).await;
    (result, events.into_inner().unwrap())
}

fn hosts(events: &[ListenEvent]) -> Vec<(ListenProtocol, Option<std::net::IpAddr>)> {
    events
        .iter()
        .flat_map(|event| match event {
            ListenEvent::Heard { heard, .. } => heard.clone(),
            _ => Vec::new(),
        })
        .filter_map(|heard| match heard {
            Heard::Host(host) => Some((host.protocol, host.address)),
            Heard::Switch(_) => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn the_packets_tcpdump_keeps_are_heard_until_the_time_is_up() {
    let folder = tempfile::tempdir().unwrap();
    let program = fake_tcpdump(folder.path(), "cat \"$PCAP\"\n: > \"$READY\"\nexec sleep 30");
    let _held = hold_clock();
    let time_is_up = async {
        assert!(written(folder.path()).await, "the stand-in tcpdump never wrote its packets");
        tokio::time::advance(WINDOW).await;
    };
    let listening = listened(&program, request(Some(WINDOW.as_secs())), Arc::default());
    let ((result, events), ()) = tokio::join!(listening, time_is_up);
    assert_eq!(result, Ok(()));
    assert_eq!(events.first(), Some(&ListenEvent::Started));
    assert_eq!(hosts(&events), [(ListenProtocol::Arp, Some("192.0.2.31".parse().unwrap()))]);
}

#[tokio::test]
async fn a_cancelled_job_stops_tcpdump_and_says_so() {
    let folder = tempfile::tempdir().unwrap();
    let program = fake_tcpdump(folder.path(), "cat \"$PCAP\"\nexec sleep 30");
    let control = Arc::new(Control::default());
    let events = Mutex::new(Vec::new());
    let report = |event: ListenEvent| {
        if !hosts(std::slice::from_ref(&event)).is_empty() {
            control.cancel();
        }
        events.lock().unwrap().push(event);
    };
    let result = run(&program, &request(None), Arc::clone(&control), report).await;
    assert_eq!(result, Err(CaptureFailure::Cancelled));
    assert_eq!(hosts(&events.into_inner().unwrap()).len(), 1, "what was heard before still counts");
}

#[tokio::test]
async fn tcpdump_refused_the_interface_is_denied() {
    let folder = tempfile::tempdir().unwrap();
    let refusal = "echo \"tcpdump: en0: You don't have permission to capture on that device\" >&2\nexit 1";
    let program = fake_tcpdump(folder.path(), refusal);
    let (result, _) = listened(&program, request(Some(30)), Arc::default()).await;
    assert_eq!(result, Err(CaptureFailure::Denied));
}

#[test]
fn listening_asks_tcpdump_for_whole_packets_through_its_filter() {
    let args = Tcpdump::listening("en0").args();
    assert_eq!(args[..6], ["-i", "en0", "-U", "-n", "-s", "0"]);
    assert_eq!(args[args.len() - 2..], ["--", borg_core::LISTEN_FILTER]);
}
