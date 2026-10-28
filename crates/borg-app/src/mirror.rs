//! An interactive local terminal mirror of a persistent Borg session on a paired machine.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use borg_term::{TermMode, encode_key, encode_mouse, encode_paste};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, EventStream,
        KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EndSynchronizedUpdate, LeaveAlternateScreen},
};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::{io::AsyncWriteExt, sync::mpsc, time::Duration};

use crate::{agent::Call, link, session::StateDir};

const KEEPALIVE: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

fn timeout_message(timeout: Duration, step: &str) -> String {
    borg_i18n::tr!(remote_mirror_step_timeout, seconds = timeout.as_secs(), step = step)
}

async fn connect_step<T>(
    current: &mut &'static str,
    step: &'static str,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    *current = step;
    tokio::time::timeout(CONNECT_TIMEOUT, work)
        .await
        .with_context(|| timeout_message(CONNECT_TIMEOUT, step))?
        .with_context(|| borg_i18n::tr!(remote_mirror_step, step = step))
}

fn keepalive_timer() -> tokio::time::Interval {
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + KEEPALIVE, KEEPALIVE);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    timer
}

async fn keep_alive(remote: &mut link::Remote, session: &str) -> Result<()> {
    checked(remote.call(&Call::new("mirror_view", json!({"session":session}))).await?)?;
    Ok(())
}

fn checked(reply: crate::agent::Reply) -> Result<String> {
    if !reply.ok {
        bail!("{}", reply.text);
    }
    Ok(reply.text)
}

fn control_key(key: &KeyEvent, letter: char, raw: char) -> bool {
    match key.code {
        KeyCode::Char(c) => {
            c == raw || (key.modifiers.contains(KeyModifiers::CONTROL) && c.to_ascii_lowercase() == letter)
        }
        _ => false,
    }
}

fn exit_requested(key: &KeyEvent) -> bool {
    control_key(key, 'g', '\u{7}') || control_key(key, ']', '\u{1d}')
}

fn take_requested(key: &KeyEvent) -> bool {
    control_key(key, 't', '\u{14}')
}

fn banner(output: &mut Vec<u8>, (cols, rows): (u16, u16), controlling: bool) {
    if cols == 0 || rows == 0 {
        return;
    }
    let status = if controlling {
        "REMOTE MIRROR - CONTROL: keys/mouse go to host; a key wakes its screensaver."
    } else {
        "REMOTE MIRROR - VIEW ONLY: keys/mouse ignored. Ctrl-T or click here: take control."
    };
    let exit = "EXIT MIRROR: hold Control and press G (Ctrl-G), or click this line.";
    let write_line = |row: u16, message: &str| {
        let clipped: String = message.chars().take(cols as usize).collect();
        format!("\x1b[{row};1H\x1b[0m\x1b[7m\x1b[2K{clipped}")
    };
    output.extend_from_slice(b"\x1b7");
    if rows > 1 {
        output.extend_from_slice(write_line(rows - 1, status).as_bytes());
    }
    output.extend_from_slice(write_line(rows, exit).as_bytes());
    output.extend_from_slice(b"\x1b[0m\x1b8");
}

#[derive(Default)]
struct Paint {
    banner: Option<((u16, u16), bool)>,
}

impl Paint {
    fn frame(&mut self, bytes: &[u8], reset: bool, size: (u16, u16), controlling: bool, native: bool) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes.len());
        if reset {
            output.extend_from_slice(b"\x1b[?2026h");
            if !native || self.banner.is_none() {
                output.extend_from_slice(b"\x1b[?1049h");
            }
            if !native {
                output.extend_from_slice(b"\x1b[2J\x1b[H");
            }
        }
        output.extend_from_slice(bytes);
        if reset || !native || self.banner != Some((size, controlling)) {
            banner(&mut output, size, controlling);
            self.banner = Some((size, controlling));
        }
        if reset {
            output.extend_from_slice(b"\x1b[?2026l");
        }
        output
    }
}

async fn frames(
    mut remote: link::Remote,
    session: &str,
    controlling: Arc<AtomicBool>,
    mode: Arc<AtomicU32>,
    native: bool,
    mut initial: Option<String>,
) -> Result<()> {
    let mut after = 0_u64;
    let mut output = tokio::io::stdout();
    let mut previous_size = None;
    let mut paint = Paint::default();
    loop {
        let size = crossterm::terminal::size()?;
        if native && previous_size != Some(size) {
            after = 0;
            previous_size = Some(size);
        }
        let reply = match initial.take() {
            Some(reply) => reply,
            None => checked(
            remote
                .call(&Call::new("mirror_frame", json!({ "session": session, "after": after, "wait_ms": 1000, "cols":size.0,"rows":size.1.saturating_sub(2).max(1) })))
                .await.context("Mirror display connection failed")?,
            )?,
        };
        let frame: Value = serde_json::from_str(&reply).context("unreadable mirror frame")?;
        if let Some(controller) = frame["controller"].as_bool() {
            controlling.store(controller, Ordering::Relaxed);
        }
        if let Some(bits) = frame["mode"].as_u64().and_then(|bits| u32::try_from(bits).ok()) {
            mode.store(bits, Ordering::Relaxed);
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(frame["data"].as_str().unwrap_or_default())?;
        let painted = paint.frame(&bytes, frame["reset"] == true, size, controlling.load(Ordering::Relaxed), native);
        if !painted.is_empty() {
            output.write_all(&painted).await?;
            output.flush().await?;
        }
        after = frame["sequence"].as_u64().context("mirror frame has no sequence")?;
        if bytes.is_empty() && frame["long_poll"] != true {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn take_control(control: &mut link::Remote, session: &str, controlling: &AtomicBool) -> Result<()> {
    let reply = control
        .call(&Call::new("mirror_take", json!({ "session": session, "force": true })))
        .await
        .context("Mirror control connection failed")?;
    if reply.ok {
        controlling.store(true, Ordering::Relaxed);
    }
    Ok(())
}

struct MirrorTask(tokio::task::JoinHandle<Result<()>>);

impl Drop for MirrorTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum MirrorInput {
    Bytes(Vec<u8>),
    Event(Event),
}

async fn send_inputs(
    mut remote: link::Remote,
    session: String,
    controlling: Arc<AtomicBool>,
    mut incoming: mpsc::Receiver<MirrorInput>,
) -> Result<()> {
    let mut pending = None;
    let mut keepalive = keepalive_timer();
    loop {
        let first = match pending.take() {
            Some(input) => Some(input),
            None => tokio::select! {
                input = incoming.recv() => input,
                _ = keepalive.tick() => {
                    keep_alive(&mut remote, &session).await.context("Mirror input connection failed")?;
                    continue;
                }
            },
        };
        let Some(first) = first else {
            break;
        };
        let (mut bytes, mut events) = match first {
            MirrorInput::Bytes(bytes) => (bytes, Vec::new()),
            MirrorInput::Event(event) => (Vec::new(), vec![event]),
        };
        let mut weight = bytes.len() + serde_json::to_vec(&events)?.len();
        while weight < 128 * 1024 && events.len() < 128 {
            let Ok(next) = incoming.try_recv() else {
                break;
            };
            let cost = match &next {
                MirrorInput::Bytes(data) => data.len(),
                MirrorInput::Event(event) => serde_json::to_vec(event)?.len(),
            };
            if weight + cost > 128 * 1024 || !bytes.is_empty() && bytes.len() + cost > 64 * 1024 {
                pending = Some(next);
                break;
            }
            weight += cost;
            match next {
                MirrorInput::Bytes(data) => bytes.extend(data),
                MirrorInput::Event(event) => events.push(event),
            }
        }
        if !controlling.load(Ordering::Relaxed) {
            continue;
        }
        let args = if events.is_empty() {
            json!({"session":session,"data":base64::engine::general_purpose::STANDARD.encode(bytes)})
        } else {
            json!({"session":session,"events":events})
        };
        let reply = remote.call(&Call::new("mirror_input", args)).await.context("Mirror input connection failed")?;
        if !reply.ok {
            controlling.store(false, Ordering::Relaxed);
        }
    }
    Ok(())
}

async fn forward(input: &mpsc::Sender<MirrorInput>, controlling: &AtomicBool, bytes: &[u8]) -> Result<()> {
    if !controlling.load(Ordering::Relaxed) {
        return Ok(());
    }
    for chunk in bytes.chunks(32 * 1024) {
        input.send(MirrorInput::Bytes(chunk.to_vec())).await.context("Mirror input disconnected")?;
    }
    Ok(())
}

async fn forward_event(input: &mpsc::Sender<MirrorInput>, controlling: &AtomicBool, event: Event) -> Result<()> {
    if !controlling.load(Ordering::Relaxed) {
        return Ok(());
    }
    if let Event::Paste(text) = event {
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + 16 * 1024).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            input
                .send(MirrorInput::Event(Event::Paste(text[start..end].into())))
                .await
                .context("Mirror input disconnected")?;
            start = end;
        }
    } else {
        input.send(MirrorInput::Event(event)).await.context("Mirror input disconnected")?;
    }
    Ok(())
}

pub async fn command(target: &str, force: bool) -> Result<()> {
    struct Terminal;
    impl Drop for Terminal {
        fn drop(&mut self) {
            let _ = execute!(
                std::io::stdout(),
                EndSynchronizedUpdate,
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _terminal = Terminal;
    crossterm::terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    let mut input = EventStream::new();
    command_with_input(target, force, &mut input).await
}

pub async fn command_with_input(target: &str, force: bool, input: &mut EventStream) -> Result<()> {
    let connection = connect(target, force).await?;
    run(connection, input).await
}

pub struct Connection {
    control: link::Remote,
    stream: link::Remote,
    input_remote: link::Remote,
    session: String,
    native: bool,
    controlling: Arc<AtomicBool>,
    initial: String,
}

pub async fn connect(target: &str, force: bool) -> Result<Connection> {
    let mut step = borg_i18n::tr!(remote_mirror_opening);
    tokio::time::timeout(STARTUP_TIMEOUT, connect_inner(target, force, &mut step))
        .await
        .with_context(|| timeout_message(STARTUP_TIMEOUT, step))?
}

async fn connect_inner(target: &str, force: bool, step: &mut &'static str) -> Result<Connection> {
    let (machine, name) = target
        .split_once(':')
        .filter(|(machine, name)| !machine.is_empty() && !name.is_empty())
        .context("usage: borg mirror SERVER:SESSION [--take]")?;
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
    let server = link::server(state.path(), machine)?;
    let (mut control, mut stream, input_remote) = connect_step(step, borg_i18n::tr!(remote_mirror_opening), async {
        tokio::try_join!(
            link::Remote::open(state.path(), &server),
            link::Remote::open(state.path(), &server),
            link::Remote::open(state.path(), &server)
        )
    })
    .await?;
    let catalog = checked(
        connect_step(step, borg_i18n::tr!(remote_mirror_sessions), async {
            control.call(&Call::new("list_sessions", json!({}))).await
        })
        .await?,
    )?;
    let session = catalog
        .lines()
        .find_map(|line| {
            let (id, label) = line.split_once(char::is_whitespace)?;
            (id == name || label.trim() == name).then(|| id.to_owned())
        })
        .with_context(|| format!("no session named {name} on {machine}"))?;
    let (cols, rows) = crossterm::terminal::size()?;
    let (view, initial) = connect_step(step, borg_i18n::tr!(remote_mirror_screen), async {
        let view = Call::new("mirror_view", json!({"session":session}));
        let frame = Call::new(
            "mirror_frame",
            json!({"session":session,"after":0,"wait_ms":0,"cols":cols,"rows":rows.saturating_sub(2).max(1)}),
        );
        tokio::try_join!(control.call(&view), stream.call(&frame))
    })
    .await?;
    if !view.ok && view.text.contains("unknown tool") {
        bail!(
            "The running Borg on {machine} cannot mirror regular sessions. Update and restart both host and client with regular-session mirror support. {}",
            view.text
        );
    }
    let view = checked(view)?;
    let native = serde_json::from_str::<Value>(&view).ok().is_some_and(|value| value["native_input"] == true);
    let mut initial: Value = serde_json::from_str(&checked(initial)?).context("Unreadable mirror frame")?;
    let frame = initial.as_object_mut().context("Mirror frame is not an object")?;
    let take = connect_step(step, borg_i18n::tr!(remote_mirror_control), async {
        control.call(&Call::new("mirror_take", json!({ "session": session, "force": force }))).await
    })
    .await?;
    let controlling = Arc::new(AtomicBool::new(take.ok));
    if !controlling.load(Ordering::Relaxed)
        && (force || !take.text.to_ascii_lowercase().contains("another viewer controls"))
    {
        bail!("{}", take.text);
    }
    frame.insert("controller".into(), Value::Bool(take.ok));
    Ok(Connection { control, stream, input_remote, session, native, controlling, initial: initial.to_string() })
}

pub async fn run(connection: Connection, input: &mut EventStream) -> Result<()> {
    let Connection { mut control, stream, input_remote, session, native, controlling, initial } = connection;
    let mut current_size = None;
    let mut resize = tokio::time::interval(Duration::from_millis(250));
    let mut keepalive = keepalive_timer();
    // Older services omit `mode`; these are the terminal modes Borg itself uses for input.
    let fallback =
        TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE | TermMode::BRACKETED_PASTE;
    let mode = Arc::new(AtomicU32::new(fallback.bits()));
    let frame_session = session.clone();
    let frame_control = controlling.clone();
    let frame_mode = mode.clone();
    let mut displaying = MirrorTask(tokio::spawn(async move {
        frames(stream, &frame_session, frame_control, frame_mode, native, Some(initial)).await
    }));
    let (input_tx, input_rx) = mpsc::channel(64);
    let mut sending =
        MirrorTask(tokio::spawn(send_inputs(input_remote, session.clone(), controlling.clone(), input_rx)));
    let result = loop {
        tokio::select! {
            result = &mut displaying.0 => break result.context("mirror display task failed")?,
            result = &mut sending.0 => break result.context("mirror input task failed")?,
            _ = keepalive.tick() => {
                keep_alive(&mut control, &session).await.context("Mirror control connection failed")?;
            }
            _ = resize.tick() => {
                if !controlling.load(Ordering::Relaxed) {
                    current_size = None;
                    continue;
                }
                let size = crossterm::terminal::size()?;
                if current_size != Some(size) {
                    current_size = Some(size);
                    let reply = control.call(&Call::new("mirror_resize", json!({
                        "session": session, "cols": size.0, "rows": size.1.saturating_sub(2).max(8)
                    }))).await.context("Mirror control connection failed")?;
                    if !reply.ok {
                        controlling.store(false, Ordering::Relaxed);
                        current_size = None;
                    }
                }
            }
            event = input.next() => {
                let Some(event) = event else { break Ok(()); };
                let event = event?;
                let remote_mode = TermMode::from_bits_truncate(mode.load(Ordering::Relaxed));
                match event {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if exit_requested(&key) { break Ok(()); }
                        if take_requested(&key) {
                            take_control(&mut control, &session, &controlling).await?;
                            current_size = None;
                        } else if native {
                            forward_event(&input_tx, &controlling, Event::Key(key)).await?;
                        } else if let Some(bytes) = encode_key(&key, remote_mode) {
                            forward(&input_tx, &controlling, &bytes).await?;
                        }
                    }
                    Event::Paste(text) => {
                        if native {
                            forward_event(&input_tx, &controlling, Event::Paste(text)).await?;
                        } else {
                            let bytes = encode_paste(&text, remote_mode);
                            forward(&input_tx, &controlling, &bytes).await?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let (_, rows) = crossterm::terminal::size()?;
                        let clicked = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
                        if clicked && mouse.row == rows.saturating_sub(1) { break Ok(()); }
                        if clicked && mouse.row == rows.saturating_sub(2) && !controlling.load(Ordering::Relaxed) {
                            take_control(&mut control, &session, &controlling).await?;
                            current_size = None;
                        } else if mouse.row < rows.saturating_sub(2) {
                            if native {
                                forward_event(&input_tx, &controlling, Event::Mouse(mouse)).await?;
                            } else if let Some(bytes) = encode_mouse(mouse, remote_mode) {
                                forward(&input_tx, &controlling, &bytes).await?;
                            }
                        }
                    }
                    Event::Resize(_, _) => current_size = None,
                    _ => {}
                }
            }
        }
    };
    drop(displaying);
    drop(sending);
    let _ = tokio::time::timeout(
        Duration::from_millis(500),
        control.call(&Call::new("mirror_release", json!({ "session": session }))),
    )
    .await;
    result
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Paint, exit_requested, take_requested};

    #[test]
    fn native_updates_keep_the_exit_banner_without_repainting_it_on_every_frame() {
        let mut paint = Paint::default();
        let first = paint.frame(b"\x1b[?2026h\x1b[2Jhost\x1b[?2026l", true, (120, 40), true, true);
        assert!(String::from_utf8_lossy(&first).contains("EXIT MIRROR"));
        let mut viewer = borg_term::Emulator::new(borg_term::GridSize { cols: 120, rows: 40 }, 0);
        viewer.feed(&first);
        let update = b"\x1b[2;2Hnext";
        let painted = paint.frame(update, false, (120, 40), true, true);
        assert_eq!(painted, update);
        viewer.feed(&painted);
        assert!(viewer.screen().live_text()[39].starts_with("EXIT MIRROR"));
        assert!(paint.frame(b"", false, (120, 40), true, true).is_empty());
        let changed = paint.frame(b"", false, (120, 40), false, true);
        assert!(String::from_utf8_lossy(&changed).contains("VIEW ONLY"));
        assert!(String::from_utf8_lossy(&paint.frame(b"", false, (100, 30), false, true)).contains("EXIT MIRROR"));
        let reset = paint.frame(b"\x1b[?2026h\x1b[2J\x1b[?2026l", true, (100, 30), false, true);
        assert!(String::from_utf8_lossy(&reset).contains("EXIT MIRROR"));
        assert!(!reset.windows(8).any(|bytes| bytes == b"\x1b[?1049h"));
    }

    #[test]
    fn pty_updates_restore_the_banner_because_the_host_can_paint_over_it() {
        let mut paint = Paint::default();
        paint.frame(b"host", true, (120, 40), true, false);
        let update = paint.frame(b"\x1b[2Jhost", false, (120, 40), true, false);
        assert!(String::from_utf8_lossy(&update).contains("EXIT MIRROR"));
    }

    #[test]
    fn documented_control_keys_exit_or_take_the_mirror() {
        let control = KeyModifiers::CONTROL;
        assert!(exit_requested(&KeyEvent::new(KeyCode::Char('g'), control)));
        assert!(exit_requested(&KeyEvent::new(KeyCode::Char(']'), control)));
        assert!(take_requested(&KeyEvent::new(KeyCode::Char('t'), control)));
        assert!(!exit_requested(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)));
    }
}
