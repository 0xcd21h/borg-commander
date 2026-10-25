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
    terminal::LeaveAlternateScreen,
};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::{io::AsyncWriteExt, sync::mpsc, time::Duration};

use crate::{agent::Call, link, session::StateDir};

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

async fn banner(output: &mut tokio::io::Stdout, controlling: bool) -> Result<()> {
    let (cols, rows) = crossterm::terminal::size()?;
    if cols == 0 || rows == 0 {
        return Ok(());
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
    output.write_all(b"\x1b7").await?;
    if rows > 1 {
        output.write_all(write_line(rows - 1, status).as_bytes()).await?;
    }
    output.write_all(write_line(rows, exit).as_bytes()).await?;
    output.write_all(b"\x1b[0m\x1b8").await?;
    Ok(())
}

async fn frames(
    mut remote: link::Remote,
    session: &str,
    controlling: Arc<AtomicBool>,
    mode: Arc<AtomicU32>,
) -> Result<()> {
    let mut after = 0_u64;
    let mut output = tokio::io::stdout();
    loop {
        let reply = checked(
            remote
                .call(&Call::new("mirror_frame", json!({ "session": session, "after": after, "wait_ms": 1000 })))
                .await?,
        )?;
        let frame: Value = serde_json::from_str(&reply).context("unreadable mirror frame")?;
        if let Some(controller) = frame["controller"].as_bool() {
            controlling.store(controller, Ordering::Relaxed);
        }
        if let Some(bits) = frame["mode"].as_u64().and_then(|bits| u32::try_from(bits).ok()) {
            mode.store(bits, Ordering::Relaxed);
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(frame["data"].as_str().unwrap_or_default())?;
        if frame["reset"] == true {
            output.write_all(b"\x1b[?1049h\x1b[2J\x1b[H").await?;
        }
        if !bytes.is_empty() {
            output.write_all(&bytes).await?;
        }
        banner(&mut output, controlling.load(Ordering::Relaxed)).await?;
        output.flush().await?;
        after = frame["sequence"].as_u64().context("mirror frame has no sequence")?;
        if bytes.is_empty() && frame["long_poll"] != true {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn take_control(control: &mut link::Remote, session: &str, controlling: &AtomicBool) -> Result<()> {
    let reply = control.call(&Call::new("mirror_take", json!({ "session": session, "force": true }))).await?;
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

async fn send_inputs(
    mut remote: link::Remote,
    session: String,
    controlling: Arc<AtomicBool>,
    mut incoming: mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    while let Some(mut bytes) = incoming.recv().await {
        // Coalesce a typing/paste burst into one RPC instead of one round trip per key.
        tokio::time::sleep(Duration::from_millis(2)).await;
        while bytes.len() < 32 * 1024 {
            let Ok(next) = incoming.try_recv() else {
                break;
            };
            bytes.extend(next);
        }
        if !controlling.load(Ordering::Relaxed) {
            continue;
        }
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let reply = remote.call(&Call::new("mirror_input", json!({ "session": session, "data": data }))).await?;
        if !reply.ok {
            controlling.store(false, Ordering::Relaxed);
        }
    }
    Ok(())
}

async fn forward(input: &mpsc::Sender<Vec<u8>>, controlling: &AtomicBool, bytes: &[u8]) -> Result<()> {
    if !controlling.load(Ordering::Relaxed) {
        return Ok(());
    }
    for chunk in bytes.chunks(32 * 1024) {
        input.send(chunk.to_vec()).await.context("mirror input disconnected")?;
    }
    Ok(())
}

pub async fn command(target: &str, force: bool) -> Result<()> {
    let mut input = EventStream::new();
    command_with_input(target, force, &mut input).await
}

pub async fn command_with_input(target: &str, force: bool, input: &mut EventStream) -> Result<()> {
    let (machine, name) = target
        .split_once(':')
        .filter(|(machine, name)| !machine.is_empty() && !name.is_empty())
        .context("usage: borg mirror SERVER:SESSION [--take]")?;
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
    let server = link::server(state.path(), machine)?;
    let mut control = link::Remote::open(state.path(), &server).await?;
    let catalog = checked(control.call(&Call::new("list_sessions", json!({}))).await?)?;
    let session = catalog
        .lines()
        .find_map(|line| {
            let (id, label) = line.split_once(char::is_whitespace)?;
            (id == name || label.trim() == name).then(|| id.to_owned())
        })
        .with_context(|| format!("no session named {name} on {machine}"))?;
    let take = control.call(&Call::new("mirror_take", json!({ "session": session, "force": force }))).await?;
    let controlling = Arc::new(AtomicBool::new(take.ok));
    if !controlling.load(Ordering::Relaxed) {
        if force {
            bail!("{}", take.text);
        }
        checked(control.call(&Call::new("mirror_view", json!({ "session": session }))).await?)?;
    }
    let stream = link::Remote::open(state.path(), &server).await?;
    let input_remote = link::Remote::open(state.path(), &server).await?;
    crossterm::terminal::enable_raw_mode()?;
    struct Terminal;
    impl Drop for Terminal {
        fn drop(&mut self) {
            let _ = execute!(std::io::stdout(), DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen);
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _terminal = Terminal;
    execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    let mut current_size = None;
    let mut resize = tokio::time::interval(Duration::from_millis(250));
    // Older services omit `mode`; these are the terminal modes Borg itself uses for input.
    let fallback =
        TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE | TermMode::BRACKETED_PASTE;
    let mode = Arc::new(AtomicU32::new(fallback.bits()));
    let frame_session = session.clone();
    let frame_control = controlling.clone();
    let frame_mode = mode.clone();
    let mut displaying =
        MirrorTask(tokio::spawn(async move { frames(stream, &frame_session, frame_control, frame_mode).await }));
    let (input_tx, input_rx) = mpsc::channel(64);
    let mut sending =
        MirrorTask(tokio::spawn(send_inputs(input_remote, session.clone(), controlling.clone(), input_rx)));
    let result = loop {
        tokio::select! {
            result = &mut displaying.0 => break result.context("mirror display task failed")?,
            result = &mut sending.0 => break result.context("mirror input task failed")?,
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
                    }))).await?;
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
                        } else if let Some(bytes) = encode_key(&key, remote_mode) {
                            forward(&input_tx, &controlling, &bytes).await?;
                        }
                    }
                    Event::Paste(text) => {
                        let bytes = encode_paste(&text, remote_mode);
                        forward(&input_tx, &controlling, &bytes).await?;
                    }
                    Event::Mouse(mouse) => {
                        let (_, rows) = crossterm::terminal::size()?;
                        let clicked = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
                        if clicked && mouse.row == rows.saturating_sub(1) { break Ok(()); }
                        if clicked && mouse.row == rows.saturating_sub(2) && !controlling.load(Ordering::Relaxed) {
                            take_control(&mut control, &session, &controlling).await?;
                            current_size = None;
                        } else if mouse.row < rows.saturating_sub(2)
                            && let Some(bytes) = encode_mouse(mouse, remote_mode) {
                                forward(&input_tx, &controlling, &bytes).await?;
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

    use super::{exit_requested, take_requested};

    #[test]
    fn documented_control_keys_exit_or_take_the_mirror() {
        let control = KeyModifiers::CONTROL;
        assert!(exit_requested(&KeyEvent::new(KeyCode::Char('g'), control)));
        assert!(exit_requested(&KeyEvent::new(KeyCode::Char(']'), control)));
        assert!(take_requested(&KeyEvent::new(KeyCode::Char('t'), control)));
        assert!(!exit_requested(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)));
    }
}
