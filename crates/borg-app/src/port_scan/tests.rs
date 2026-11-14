use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use borg_core::{PortChoice, PortRange, ScanSpeed};
use tokio::time::sleep;

use super::*;

const LOOPBACK: &str = "127.0.0.1";
const ANSWER_TIME: Duration = Duration::from_millis(100);
const CANARY: u16 = 80;

type Answers = dyn Fn(u16, Duration) -> PortAnswer + Send + Sync;

/// A host made up in the test: `answers` says how a port answers at a time since the scan began.
struct FakeHost {
    answers: Box<Answers>,
    start: Instant,
    asked: AtomicUsize,
    canary_asked: Mutex<Vec<Duration>>,
}

impl FakeHost {
    fn new(answers: impl Fn(u16, Duration) -> PortAnswer + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            answers: Box::new(answers),
            start: Instant::now(),
            asked: AtomicUsize::new(0),
            canary_asked: Mutex::new(Vec::new()),
        })
    }
}

impl Prober for FakeHost {
    fn probe(&self, target: SocketAddr, wait: Duration) -> impl Future<Output = PortAnswer> + Send {
        let elapsed = self.start.elapsed();
        self.asked.fetch_add(1, Ordering::SeqCst);
        if target.port() == CANARY {
            self.canary_asked.lock().unwrap().push(elapsed);
        }
        let answer = (self.answers)(target.port(), elapsed);
        async move {
            sleep(if answer == PortAnswer::Filtered { wait } else { ANSWER_TIME }).await;
            answer
        }
    }
}

fn request(ports: Vec<u16>) -> PortScanRequest {
    PortScanRequest {
        target: LOOPBACK.into(),
        address: LOOPBACK.parse().unwrap(),
        gateway: None,
        choice: PortChoice::Custom(String::new()),
        ports,
        speed: ScanSpeed::Fast,
    }
}

fn no_gap() -> Duration {
    Duration::ZERO
}

async fn scan(host: &Arc<FakeHost>, ports: Vec<u16>, pace: ScanPace, control: &Control) -> PortScanResult {
    run(Arc::clone(host), &request(ports), pace, control, no_gap, |_| {}).await
}

#[tokio::test(start_paused = true)]
async fn every_port_answers_and_the_scan_is_done() {
    let host = FakeHost::new(|port, _| if port % 50 == 0 { PortAnswer::Open } else { PortAnswer::Closed });
    let result = scan(&host, (1..=200).collect(), ScanSpeed::Fast.pace(), &Control::default()).await;
    assert_eq!(result.ending, Ending::Done);
    assert_eq!(result.open, vec![50, 100, 150, 200]);
    assert_eq!((result.closed, result.filtered), (196, 0));
    assert_eq!(result.not_scanned, Vec::new());
}

#[tokio::test(start_paused = true)]
async fn a_canary_that_falls_silent_stops_the_scan_and_keeps_what_it_found() {
    let limited_after = Duration::from_secs(2);
    let host = FakeHost::new(move |port, elapsed| match (elapsed < limited_after, port) {
        (false, _) => PortAnswer::Filtered,
        (true, CANARY) => PortAnswer::Open,
        (true, _) => PortAnswer::Closed,
    });
    let mut reports = Vec::new();
    let pace = ScanSpeed::Fast.pace();
    let control = Control::default();
    let result =
        run(Arc::clone(&host), &request((1..=3_000).collect()), pace, &control, no_gap, |event| reports.push(event))
            .await;
    assert_eq!(result.ending, Ending::RateLimited { port: CANARY });
    assert_eq!(result.open, vec![CANARY], "what answered before stays");
    assert!(result.filtered > 0);
    let last = result.not_scanned.last().copied().expect("the rest is not scanned");
    assert_eq!(last.last, 3_000);
    let counted = result.open.len() + result.closed + result.filtered;
    assert_eq!(counted + result.not_scanned.iter().map(|range| range.count()).sum::<usize>(), 3_000);
    let canary = host.canary_asked.lock().unwrap().clone();
    assert!(canary.len() >= 3, "asked once in the scan, then again and once more: {canary:?}");
    assert!(canary.iter().filter(|at| **at >= limited_after).count() >= 2, "the miss got its retry");
    assert!(!reports.is_empty(), "the job list heard how far it got");
}

#[tokio::test(start_paused = true)]
async fn a_host_that_filters_most_ports_from_the_start_is_scanned_to_the_end() {
    let host = FakeHost::new(|port, _| if port == 22 { PortAnswer::Open } else { PortAnswer::Filtered });
    let result = scan(&host, (1..=300).collect(), ScanSpeed::Fast.pace(), &Control::default()).await;
    assert_eq!(result.ending, Ending::Done);
    assert_eq!(result.open, vec![22]);
    assert_eq!(result.filtered, 299);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_scan_stops_and_a_paused_one_waits() {
    let host = FakeHost::new(|_, _| PortAnswer::Closed);
    let control = Control::default();
    control.cancel();
    let result = scan(&host, (1..=10).collect(), ScanSpeed::Fast.pace(), &control).await;
    assert_eq!(result.ending, Ending::Cancelled);
    assert_eq!(result.not_scanned, vec![PortRange { first: 1, last: 10 }]);

    let control = Arc::new(Control::default());
    control.set_paused(true);
    let paused = Arc::clone(&control);
    let waiting = Arc::clone(&host);
    let before = host.asked.load(Ordering::SeqCst);
    let job = tokio::spawn(async move { scan(&waiting, (1..=10).collect(), ScanSpeed::Fast.pace(), &paused).await });
    sleep(Duration::from_secs(30)).await;
    assert_eq!(host.asked.load(Ordering::SeqCst), before, "nothing is asked while paused");
    control.set_paused(false);
    let result = job.await.unwrap();
    assert_eq!((result.ending, result.closed), (Ending::Done, 10));
}

#[tokio::test(start_paused = true)]
async fn slow_mode_leaves_its_gap_between_connections() {
    let host = FakeHost::new(|_, _| PortAnswer::Closed);
    let slow = ScanSpeed::Slow.pace();
    let started = Instant::now();
    let result = scan(&host, (1..=10).collect(), slow, &Control::default()).await;
    assert_eq!(result.closed, 10);
    assert!(started.elapsed() >= slow.gap * 9, "{:?}", started.elapsed());
    let started = Instant::now();
    scan(&host, (1..=10).collect(), ScanSpeed::Fast.pace(), &Control::default()).await;
    assert!(started.elapsed() < slow.gap * 9, "fast mode asks them together");
}

#[test]
fn the_random_gap_stays_within_its_spread() {
    let spread = Duration::from_millis(200);
    let mut gap = jitter(spread);
    assert!((0..100).map(|_| gap()).all(|wait| wait <= spread));
    assert_eq!(jitter(Duration::ZERO)(), Duration::ZERO);
}

#[tokio::test]
async fn a_listener_the_test_starts_on_loopback_is_open_and_a_freed_port_is_closed() {
    let listener = tokio::net::TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let open = listener.local_addr().unwrap().port();
    let freed = tokio::net::TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let closed = freed.local_addr().unwrap().port();
    drop(freed);
    let control = Control::default();
    let result =
        run(Arc::new(Connector), &request(vec![open, closed]), ScanSpeed::Fast.pace(), &control, no_gap, |_| {}).await;
    assert_eq!(result.ending, Ending::Done);
    assert_eq!(result.open, vec![open]);
    assert_eq!((result.closed, result.filtered), (1, 0));
}
