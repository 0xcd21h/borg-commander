use super::*;
use borg_core::{AlertLog, Ancestor, COMMAND_CUT, ProcessKey, Response, StartedProcess, WatchFinding, WatchStatus};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::mpsc::Receiver,
};

const NOW: Duration = Duration::from_secs(1_800_000_000);
/// How often the heartbeat goes in the test of the whole sender.
const BEAT_EVERY: Duration = Duration::from_millis(100);
/// Requests the place in that test answers, well over what it needs.
const ANSWERS: usize = 50;
/// How long that test waits for the sender's next request.
const QUIET: Duration = Duration::from_secs(10);

fn moment(after: Duration) -> SystemTime {
    SystemTime::UNIX_EPOCH + NOW + after
}

fn process(command: &str) -> StartedProcess {
    StartedProcess {
        key: ProcessKey { pid: 4242, start: 10 },
        name: "sh".into(),
        program: Some("/bin/sh".into()),
        command: command.split(' ').map(str::to_owned).collect(),
        user: "www-data".into(),
        uid: 33,
        ancestors: vec![Ancestor {
            pid: 40,
            name: "nginx".into(),
            program: Some("/usr/sbin/nginx".into()),
            terminal: false,
            command: "nginx: worker process".into(),
            ..Ancestor::default()
        }],
        ..StartedProcess::default()
    }
}

fn alert_about(finding: WatchFinding, process: StartedProcess) -> Alert {
    let mut log = AlertLog::watching(WatchStatus { source: WatchSource::Kernel, ..WatchStatus::default() });
    let id = log.add(moment(Duration::ZERO), finding, Response::Frozen, process);
    let found = log.find(id).unwrap();
    found.evidence.connections = (0..20).map(|socket| format!("socket:[{socket}]")).collect();
    found.clone()
}

fn alert(command: &str) -> Alert {
    alert_about(WatchFinding::ShellOnSocket { shell: "sh".into() }, process(command))
}

fn alert_event() -> Event {
    Event::Alert(Box::new(AlertReport::of(&alert("sh -i"))))
}

fn evidence() -> Event {
    Event::Evidence { alert: 1, level: Level::Alert, program_hash: Some("ab".repeat(32)), matches: vec![] }
}

fn envelope(body: &str) -> Envelope {
    serde_json::from_str(body).unwrap()
}

const ALL: [Channel; 3] = [Channel::Telegram, Channel::Ntfy, Channel::Webhook];

#[test]
fn every_place_gets_its_own_unbroken_chain_of_messages() {
    let mut outbox = Outbox::default();
    let now = moment(Duration::ZERO);
    for event in [alert_event(), evidence(), Event::Armed] {
        outbox.add("web-01", &event, &ALL, now).unwrap();
    }
    for channel in ALL {
        let bodies: Vec<&str> = outbox
            .waiting
            .iter()
            .filter(|waiting| waiting.channel == channel)
            .map(|waiting| waiting.body.as_str())
            .collect();
        let expected = if channel == Channel::Webhook { 3 } else { 2 };
        assert_eq!(bodies.len(), expected, "{channel:?}: the evidence goes to the webhook alone");
        let mut previous = String::new();
        for (place, body) in bodies.iter().enumerate() {
            let envelope = envelope(body);
            assert_eq!(envelope.seq, place as u64 + 1, "{channel:?}");
            assert_eq!(envelope.previous, previous, "{channel:?}: each names the digest of the one before");
            assert_eq!(envelope.host, "web-01");
            previous = digest(body.as_bytes());
        }
        assert_eq!(outbox.chains[&channel].last, previous);
    }
}

#[test]
fn a_full_outbox_gives_up_on_its_oldest_messages_and_says_how_many() {
    let mut outbox = Outbox::default();
    let now = moment(Duration::ZERO);
    for _ in 0..OUTBOX_LIMIT + 3 {
        outbox.add("web-01", &Event::Armed, &[Channel::Webhook], now).unwrap();
    }
    assert_eq!(outbox.waiting.len(), OUTBOX_LIMIT);
    assert_eq!(outbox.dropped, 3);
    assert_eq!(outbox.waiting[0].seq, 4);
    outbox.add("web-01", &Event::Armed, &[Channel::Webhook], now).unwrap();
    let last = envelope(&outbox.waiting.back().unwrap().body);
    assert_eq!(last.dropped, 3, "the next message tells the receiver what never came");
    assert_eq!(outbox.dropped, 4, "and making room for it gave up on one more");
}

fn busy() -> Refused {
    Refused { status: Some(500), said: "busy".into() }
}

#[test]
fn a_refused_message_waits_longer_each_time_until_borg_gives_up_on_it() {
    assert_eq!(retry_wait(1), Duration::from_secs(5));
    assert_eq!(retry_wait(2), Duration::from_secs(30));
    assert_eq!(retry_wait(5), Duration::from_secs(30 * 60));
    assert_eq!(retry_wait(50), Duration::from_secs(30 * 60), "the longest wait repeats");

    let start = moment(Duration::ZERO);
    let mut outbox = Outbox::default();
    outbox.add("web-01", &Event::Armed, &[Channel::Webhook], start).unwrap();
    assert_eq!(outbox.next_due(&[Channel::Webhook]), Some(start));
    outbox.failed(0, busy(), start);
    assert_eq!(outbox.next_due(&[Channel::Webhook]), Some(start + Duration::from_secs(5)));
    assert_eq!(outbox.errors[&Channel::Webhook].to_string(), "HTTP 500: busy");
    assert!(!outbox.late(start + Duration::from_secs(60)));
    assert!(outbox.late(start + LATE), "a message this old means the place stopped taking them");

    outbox.failed(0, busy(), start + GIVE_UP_AFTER);
    assert!(outbox.waiting.is_empty());
    assert_eq!(outbox.dropped, 1);

    outbox.add("web-01", &Event::Armed, &[Channel::Webhook], start).unwrap();
    outbox.sent(0);
    assert!(outbox.errors.is_empty(), "a place that takes a message again is fine again");
}

#[test]
fn the_webhook_signature_is_hmac_sha256_over_the_time_and_body() {
    assert_eq!(
        signature("shared secret", 1_800_000_000, r#"{"a":1}"#),
        "t=1800000000,v1=b018913350425d23394ab5e9909daca844a93e6a3263eb12fb02daf340a6e760"
    );
}

#[test]
fn an_alert_reads_as_a_title_and_the_facts_a_person_needs() {
    let mut outbox = Outbox::default();
    outbox.add("web-01", &alert_event(), &[Channel::Telegram], moment(Duration::ZERO)).unwrap();
    let (title, lines) = text(&envelope(&outbox.waiting[0].body)).unwrap();
    assert!(title.starts_with("web-01: "), "{title}");
    assert!(title.contains(WatchRule::ShellOnSocket.label()), "{title}");
    for part in ["/bin/sh", "4242", "www-data", "nginx", "sh -i", "#1"] {
        assert!(lines.contains(part), "{part} in {lines}");
    }
    let evidence = Envelope { event: evidence(), ..envelope(&outbox.waiting[0].body) };
    assert_eq!(text(&evidence), None, "a person gets no hashes on their phone");
    let test = Envelope { seq: 0, event: Event::Test, ..evidence };
    assert!(!text(&test).unwrap().1.contains('#'), "a test is outside the chain");
}

#[test]
fn a_report_keeps_a_message_small() {
    let long = format!("sh -c {} /tmp/.x/run", "x".repeat(TEXT_SENT * 2));
    let report = AlertReport::of(&alert(&long));
    assert!(report.command.len() <= TEXT_SENT, "{} bytes", report.command.len());
    assert!(report.command.starts_with("sh -c xxx") && report.command.contains(COMMAND_CUT));
    assert!(report.command.ends_with("x /tmp/.x/run"), "padding the start hides nothing: {}", report.command);
    assert_eq!(report.connections.len(), CONNECTIONS_SENT);
    assert_eq!(report.parent.as_deref(), Some("nginx"));
    assert_eq!(report.ancestors, ["nginx: worker process"]);
    assert!(report.frozen);
    assert_eq!(AlertReport::of(&alert("sh -i")).command, "sh -i", "a short command goes whole");
    let wide = AlertReport::of(&alert(&"€".repeat(TEXT_SENT))).command;
    assert!(wide.len() <= TEXT_SENT && wide.contains(COMMAND_CUT), "a cut never splits a character");

    let value = format!("{}/tmp/.x/hook.so", " ".repeat(TEXT_SENT * 64));
    let finding = WatchFinding::PreloadSet { program: "/usr/bin/curl".into(), variable: "LD_PRELOAD".into(), value };
    let whole = finding.reason();
    let report = AlertReport::of(&alert_about(finding, process("curl -s example.com")));
    assert!(report.reason.len() <= TEXT_SENT, "a preload value is as long as the environment: {}", report.reason.len());
    assert!(report.reason.starts_with(&whole[..TEXT_SENT / 4]));
    assert!(report.reason.ends_with(&whole[whole.len() - TEXT_SENT / 4..]));
    assert!(report.reason.contains("/tmp/.x/hook.so"), "{}", report.reason);
}

/// Control characters grow sevenfold on their way into the outbox: six bytes each in a
/// message's JSON, and a backslash more in the outbox's own. A full outbox of alerts about
/// commands made of them used to pass the bytes Borg reads, so the sender started from an empty
/// outbox and wrote it over the messages still waiting.
#[test]
fn a_full_outbox_of_long_control_character_commands_reads_back_without_its_oldest() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    let control = "\u{1}".repeat(TEXT_SENT);
    let parent = Ancestor { pid: 40, name: "sh".into(), command: control.clone(), ..Ancestor::default() };
    let noisy = StartedProcess {
        command: vec![control],
        ancestors: vec![parent; crate::watch::facts::ANCESTORS],
        ..process("sh")
    };
    let report = AlertReport::of(&alert_about(WatchFinding::ShellOnSocket { shell: "sh".into() }, noisy));
    let event = Event::Alert(Box::new(report));
    let mut outbox = Outbox::default();
    for _ in 0..OUTBOX_LIMIT {
        outbox.add("web-01", &event, &[Channel::Webhook], moment(Duration::ZERO)).unwrap();
    }
    let newest = outbox.waiting.back().unwrap().seq;
    let one = serde_json::to_vec(&outbox.waiting[0]).unwrap().len();
    write_outbox(state, &mut outbox);
    let written = fs::metadata(outbox_path(state)).unwrap().len() as usize;
    assert!(written <= OUTBOX_BYTES, "{written} bytes");
    assert!(OUTBOX_BYTES - written < 2 * one, "only the messages it takes go: {} left", outbox.waiting.len());
    assert!(outbox.dropped > 0);
    assert_eq!(outbox.waiting.len() + outbox.dropped as usize, OUTBOX_LIMIT, "each one counts as given up on");
    assert_eq!(outbox.waiting.back().unwrap().seq, newest, "the newest message stays");
    assert_eq!(load_outbox(state).unwrap(), outbox, "what is written reads back, and the outbox in hand is the same");
}

#[test]
fn an_outbox_borg_cannot_read_is_kept_aside_and_the_next_heartbeat_says_so() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path();
    let now = moment(Duration::ZERO);
    assert_eq!(load_outbox(state).unwrap(), Outbox::default(), "no outbox yet is an empty one");
    let mut outbox = Outbox::default();
    outbox.add("web-01", &Event::Armed, &[Channel::Webhook], now).unwrap();
    write_outbox(state, &mut outbox);
    assert_eq!(load_outbox(state).unwrap(), outbox, "an outbox Borg reads stays where it is");

    let nonsense = b"not json".to_vec();
    let huge = Waiting { body: "x".repeat(OUTBOX_BYTES), ..outbox.waiting[0].clone() };
    let too_long = serde_json::to_vec(&Outbox { waiting: VecDeque::from([huge]), ..Outbox::default() }).unwrap();
    let first = state.join(WATCH_OUTBOX_ASIDE);
    let second = borg_jobs::unique_by(&first, |_| false);
    let kept = Outbox { kept_aside: true, ..Outbox::default() };
    for (contents, aside) in [(&nonsense, &first), (&too_long, &second)] {
        fs::write(outbox_path(state), contents).unwrap();
        assert_eq!(load_outbox(state).unwrap(), kept);
        assert_eq!(&fs::read(aside).unwrap(), contents, "the old outbox is kept whole");
        assert_eq!(load_outbox(state).unwrap(), kept, "the new one is written at once and remembers it");
    }
    let healthy = Health { source: WatchSource::Kernel, seen: 1, suppressed: 0, examiner: true };
    let (fine, body) = beat("web-01", Some(&healthy), &kept, 0, now);
    assert!(!fine, "the messages waiting in it are lost to every place");
    assert!(body.contains(tr!(notify_beat_kept_aside)), "{body}");
    assert!(!body.contains(WATCH_OUTBOX_ASIDE), "where it went stays on this machine: {body}");
}

/// The whole sender, from the file in the state folder to a place on this machine.
#[test]
fn a_sender_that_cannot_read_its_outbox_keeps_it_aside_and_carries_on() {
    let folder = tempfile::tempdir().unwrap();
    let state = folder.path().to_path_buf();
    fs::write(outbox_path(&state), b"not json").unwrap();
    let (address, received) = server(vec![200; ANSWERS]);
    let settings = Settings {
        webhook: Some(Webhook { url: format!("{address}/hook"), secret: None }),
        heartbeat: Some(Heartbeat { url: format!("{address}/ping"), every: BEAT_EVERY }),
        ..Settings::default()
    };
    let notifier = Notifier::start(settings, "web-01".into(), state.clone()).unwrap();
    notifier.health(Health { source: WatchSource::Kernel, seen: 1, suppressed: 0, examiner: true });
    notifier.send(Event::Armed);
    let (mut sent, mut failed, mut fine_again) = (None, None, None);
    while sent.is_none() || fine_again.is_none() {
        let request = received.recv_timeout(QUIET).expect("the sender went quiet");
        match request.line.as_str() {
            "POST /hook HTTP/1.1" => sent = Some(envelope(&request.body)),
            "POST /ping/fail HTTP/1.1" => failed = Some(request.body),
            "POST /ping HTTP/1.1" if failed.is_some() => fine_again = Some(request.body),
            _ => {}
        }
    }
    drop(notifier);
    assert_eq!(sent.unwrap().seq, 1, "messages still leave, numbered from 1 again");
    let failed = failed.unwrap();
    assert!(
        failed.contains(tr!(notify_beat_kept_aside)),
        "the first heartbeat that saw the watcher says why: {failed}"
    );
    assert!(!fine_again.unwrap().contains(tr!(notify_beat_kept_aside)), "and once said, it is old news");
    assert_eq!(fs::read(state.join(WATCH_OUTBOX_ASIDE)).unwrap(), b"not json");
    assert!(!load_outbox(&state).unwrap().kept_aside);
}

#[test]
fn the_heartbeat_says_something_is_wrong_when_alerts_could_go_missing() {
    let now = moment(Duration::ZERO);
    let healthy = Health { source: WatchSource::Kernel, seen: 10, suppressed: 0, examiner: true };
    let outbox = Outbox::default();
    assert!(beat("web-01", None, &outbox, 0, now).0, "a watcher that is starting is fine");
    let (fine, body) = beat("web-01", Some(&healthy), &outbox, 0, now);
    assert!(fine);
    assert!(body.contains("web-01"), "{body}");
    let stopped = Health { examiner: false, ..healthy.clone() };
    assert!(!beat("web-01", Some(&stopped), &outbox, 0, now).0, "the examiner stopped");
    let gave_up = Outbox { dropped: 2, ..Outbox::default() };
    assert!(!beat("web-01", Some(&healthy), &gave_up, 1, now).0, "a message was given up on since the last beat");
    assert!(beat("web-01", Some(&healthy), &gave_up, 2, now).0, "and once said, it is old news");
    let mut stuck = Outbox::default();
    stuck.add("web-01", &Event::Armed, &[Channel::Webhook], now).unwrap();
    stuck.errors.insert(Channel::Webhook, Refused { status: Some(503), said: "down at api/hooks/k3y".into() });
    stuck.errors.insert(Channel::Ntfy, Refused::said("io: Connection refused"));
    let (fine, body) = beat("web-01", Some(&healthy), &stuck, 0, now + LATE);
    assert!(!fine, "a place stopped taking messages");
    assert!(body.contains("HTTP 503"), "{body}");
    assert!(body.contains(tr!(notify_beat_no_answer)), "{body}");
    assert!(!body.contains("k3y") && !body.contains("refused"), "what a place said stays on this machine: {body}");
}

#[test]
fn a_place_taken_out_of_the_configuration_leaves_nothing_waiting() {
    let now = moment(Duration::ZERO);
    let mut outbox = Outbox::default();
    outbox.add("web-01", &Event::Armed, &[Channel::Webhook, Channel::Ntfy], now).unwrap();
    outbox.failed(0, busy(), now);
    outbox.keep_only(&[Channel::Ntfy]);
    assert_eq!(outbox.waiting.len(), 1);
    assert_eq!(outbox.waiting[0].channel, Channel::Ntfy);
    assert!(outbox.errors.is_empty());
    assert_eq!(outbox.dropped, 0, "taking a place out is no failure");
    let healthy = Health { source: WatchSource::Kernel, seen: 1, suppressed: 0, examiner: true };
    assert!(!beat("web-01", Some(&healthy), &outbox, 0, now + LATE).0, "a place still named and stuck still fails");
    outbox.keep_only(&[]);
    assert!(beat("web-01", Some(&healthy), &outbox, 0, now + LATE).0);
}

#[test]
fn alerts_go_only_to_https_addresses_or_to_this_machine() {
    for safe in [
        "https://example.com/hook",
        "HTTPS://example.com",
        "http://127.0.0.1:8099/hook",
        "http://localhost/x",
        "http://[::1]:80/",
    ] {
        assert!(safe_address(safe), "{safe}");
    }
    for unsafe_address in [
        "http://example.com/hook",
        "http://10.0.0.5:8080/hook",
        "ftp://example.com",
        "example.com/hook",
        "https://",
        "",
    ] {
        assert!(!safe_address(unsafe_address), "{unsafe_address}");
    }
    assert_eq!(ntfy_parts("https://ntfy.sh/my-topic/"), Some(("https://ntfy.sh", "my-topic")));
    assert_eq!(ntfy_parts("https://example.com/ntfy/alerts"), Some(("https://example.com/ntfy", "alerts")));
    for no_topic in ["https://ntfy.sh", "https://ntfy.sh/", "https://ntfy.sh/topic?x=1", "ntfy.sh/topic"] {
        assert_eq!(ntfy_parts(no_topic), None, "{no_topic}");
    }
    let config = |text: &str| toml::from_str::<crate::config::Config>(text).map_err(|error| error.to_string());
    let plain = config("[watch.notify.webhook]\nurl = \"http://example.com/hook\"\n").unwrap_err();
    assert!(plain.contains(UNSAFE_ADDRESS), "{plain}");
    let beat = config("[watch.notify.heartbeat]\nurl = \"http://hc-ping.com/x\"\n").unwrap_err();
    assert!(beat.contains(UNSAFE_ADDRESS), "{beat}");
    let topicless = config("[watch.notify.ntfy]\nurl = \"https://ntfy.sh\"\n").unwrap_err();
    assert!(topicless.contains(NO_TOPIC), "{topicless}");
    assert!(config("[watch.notify.webhook]\nurl = \"http://127.0.0.1:8099/hook\"\n").is_ok());
}

#[test]
fn only_alerts_at_the_level_asked_for_are_sent() {
    let settings = Settings::default();
    assert!(settings.takes(Level::Alert) && settings.takes(Level::Warning));
    assert!(!settings.takes(Level::Notice), "notices stay on the page unless asked for");
    let everything = Settings { level: Level::Notice, ..Settings::default() };
    assert!(everything.takes(Level::Notice));
    assert!(!Settings::default().any());
    let beat_only = Settings {
        heartbeat: Some(Heartbeat { url: "https://hc-ping.com/x".into(), every: Duration::from_secs(300) }),
        ..Settings::default()
    };
    assert!(beat_only.any() && beat_only.channels().is_empty());
}

#[test]
fn the_configuration_names_the_places_alerts_go() {
    let config: crate::config::Config = toml::from_str(
        r#"
        [watch.notify]
        name = "web-01"
        level = "alert"

        [watch.notify.telegram]
        token = "123:abc"
        chat = "42"

        [watch.notify.webhook]
        url = "https://example.com/borg"
        secret = "s"

        [watch.notify.heartbeat]
        url = "https://hc-ping.com/x"
        minutes = 0
        "#,
    )
    .unwrap();
    let settings = config.watch().notify;
    assert_eq!(settings.name.as_deref(), Some("web-01"));
    assert_eq!(settings.level, Level::Alert);
    assert_eq!(settings.channels(), [Channel::Telegram, Channel::Webhook]);
    assert_eq!(settings.heartbeat.unwrap().every, Duration::from_secs(60), "at least a minute between beats");
    let unknown = toml::from_str::<crate::config::Config>("[watch.notify]\nemail = \"me@example.com\"\n");
    assert!(unknown.is_err(), "a misspelled place is an error, not silence");
}

/// What a local server was sent.
#[derive(Debug)]
struct Request {
    line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

/// A server on this machine that answers each request with the next status and the path it was
/// asked for, as a place that echoes its address would, and says what it got.
fn server(statuses: Vec<u16>) -> (String, Receiver<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        for status in statuses {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (key, value) = header.split_once(':').unwrap();
                headers.push((key.trim().to_owned(), value.trim().to_owned()));
            }
            let length = headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                .map_or(0, |(_, value)| value.parse().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
            let answer = format!(
                "HTTP/1.1 {status} Whatever\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{path}",
                path.len()
            );
            (&stream).write_all(answer.as_bytes()).unwrap();
            let _ = sent.send(Request {
                line: line.trim_end().to_owned(),
                headers,
                body: String::from_utf8(body).unwrap(),
            });
        }
    });
    (address, received)
}

#[test]
fn the_webhook_gets_the_message_exactly_with_its_kind_and_signature() {
    let (address, received) = server(vec![200]);
    let settings = Settings {
        webhook: Some(Webhook { url: format!("{address}/hook"), secret: Some("s".into()) }),
        ..Settings::default()
    };
    let mut outbox = Outbox::default();
    outbox.add("web-01", &alert_event(), &settings.channels(), moment(Duration::ZERO)).unwrap();
    let body = outbox.waiting[0].body.clone();
    deliver::send(&deliver::agent(), &settings, Channel::Webhook, &body).unwrap();
    let request = received.recv().unwrap();
    assert_eq!(request.line, "POST /hook HTTP/1.1");
    assert_eq!(request.body, body);
    assert_eq!(request.header("X-Borg-Event"), Some("alert"));
    let header = request.header("X-Borg-Signature").unwrap();
    let at: u64 = header.trim_start_matches("t=").split(',').next().unwrap().parse().unwrap();
    assert_eq!(header, signature("s", at, &body), "the receiver can check it with the shared secret");
}

#[test]
fn ntfy_gets_the_topic_title_and_priority_as_json_with_its_token() {
    let (address, received) = server(vec![200]);
    let topic = format!("{address}/borg-web-01");
    deliver::to_ntfy(&deliver::agent(), &topic, Some("tk_secret"), "web-01: alert, Shell", "sh -i", 5).unwrap();
    let request = received.recv().unwrap();
    assert_eq!(request.line, "POST / HTTP/1.1", "ntfy takes JSON at its root");
    assert_eq!(request.header("Authorization"), Some("Bearer tk_secret"));
    let json: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(json["topic"], "borg-web-01");
    assert_eq!(json["title"], "web-01: alert, Shell");
    assert_eq!(json["priority"], 5);
}

#[test]
fn telegram_gets_the_chat_and_the_text() {
    let (address, received) = server(vec![200]);
    deliver::to_telegram(&deliver::agent(), &format!("{address}/bot123/sendMessage"), "42", "web-01: test").unwrap();
    let request = received.recv().unwrap();
    assert_eq!(request.line, "POST /bot123/sendMessage HTTP/1.1");
    let json: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(json["chat_id"], "42");
    assert_eq!(json["text"], "web-01: test");
}

#[test]
fn the_heartbeat_adds_fail_when_something_is_wrong_and_a_refusal_says_why() {
    let (address, received) = server(vec![200, 200, 503]);
    let agent = deliver::agent();
    deliver::heartbeat(&agent, &format!("{address}/ping/1"), true, "fine").unwrap();
    deliver::heartbeat(&agent, &format!("{address}/ping/1/"), false, "examiner stopped").unwrap();
    assert_eq!(received.recv().unwrap().line, "POST /ping/1 HTTP/1.1");
    let failing = received.recv().unwrap();
    assert_eq!(failing.line, "POST /ping/1/fail HTTP/1.1");
    assert_eq!(failing.body, "examiner stopped");
    let refused = deliver::heartbeat(&agent, &format!("{address}/ping/1"), true, "fine").unwrap_err();
    assert_eq!(refused.to_string(), "HTTP 503: /…", "the check's key stays hidden");
}

#[test]
fn messages_to_one_place_go_in_order_and_wait_behind_a_refusal() {
    let (address, received) = server(vec![200, 500]);
    let settings = Settings { webhook: Some(Webhook { url: address, secret: None }), ..Settings::default() };
    let channels = settings.channels();
    let mut outbox = Outbox::default();
    for _ in 0..3 {
        outbox.add("web-01", &Event::Armed, &channels, SystemTime::now()).unwrap();
    }
    assert!(send_due(&settings, &deliver::agent(), &channels, &mut outbox, &|| true));
    let first = envelope(&received.recv().unwrap().body);
    let second = envelope(&received.recv().unwrap().body);
    assert_eq!((first.seq, second.seq), (1, 2));
    assert_eq!(outbox.waiting.len(), 2, "the refused message and the one behind it wait");
    assert_eq!(outbox.waiting[0].seq, 2);
    assert_eq!(outbox.waiting[0].failures, 1);
    assert!(
        !send_due(&settings, &deliver::agent(), &channels, &mut outbox, &|| true),
        "nothing is due before the wait is over"
    );
    assert!(
        !send_due(&settings, &deliver::agent(), &channels, &mut outbox, &|| false),
        "a replaced sender sends nothing"
    );
}

#[test]
fn a_secret_never_shows_in_what_went_wrong() {
    let settings =
        Settings { telegram: Some(Telegram { token: "123:SECRET".into(), chat: "42".into() }), ..Settings::default() };
    let unreachable =
        Envelope { host: "web-01".into(), seq: 1, at: 0, previous: String::new(), dropped: 0, event: Event::Test };
    let body = serde_json::to_string(&unreachable).unwrap();
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let api = format!("http://{closed}");
    let error = deliver::send_through(&deliver::agent(), &settings, Channel::Telegram, &body, &api).unwrap_err();
    assert!(!error.to_string().contains("SECRET"), "{error}");
    let (address, received) = server(vec![401]);
    let error = deliver::send_through(&deliver::agent(), &settings, Channel::Telegram, &body, &address).unwrap_err();
    assert_eq!(received.recv().unwrap().line, "POST /bot123:SECRET/sendMessage HTTP/1.1");
    assert_eq!(error.to_string(), "HTTP 401: /bot…/sendMessage");

    let (address, received) = server(vec![404, 403]);
    let webhook = Settings {
        webhook: Some(Webhook { url: format!("{address}/api/webhooks/1/s3cret-path"), secret: Some("k".into()) }),
        ntfy: Some(Ntfy { url: format!("{address}/hidden-topic"), token: Some("tk_SECRET".into()) }),
        ..Settings::default()
    };
    let error = deliver::send(&deliver::agent(), &webhook, Channel::Webhook, &body).unwrap_err();
    assert_eq!(received.recv().unwrap().line, "POST /api/webhooks/1/s3cret-path HTTP/1.1");
    assert_eq!(error.to_string(), "HTTP 404: /…", "a webhook's path is its key on Discord and Slack");
    let error = deliver::send(&deliver::agent(), &webhook, Channel::Ntfy, &body).unwrap_err();
    assert_eq!(received.recv().unwrap().line, "POST / HTTP/1.1");
    assert!(!error.to_string().contains("hidden-topic") && !error.to_string().contains("SECRET"), "{error}");
}
