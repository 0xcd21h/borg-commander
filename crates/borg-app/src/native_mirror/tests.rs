use super::*;
use crate::agent::{Call, Reply};
use base64::{Engine, engine::general_purpose::STANDARD};
use borg_term::{Emulator, GridSize};
use crossterm::event::Event;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Position};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
};
use serde_json::json;

fn frame(screen: &Screen, after: u64, cols: u16, rows: u16) -> serde_json::Value {
    let reply = screen.frame(after, cols, rows, "paired").unwrap();
    assert!(reply.ok);
    serde_json::from_str(&reply.text).unwrap()
}

fn bytes(value: &serde_json::Value) -> Vec<u8> {
    STANDARD.decode(value["data"].as_str().unwrap()).unwrap()
}

#[test]
fn copies_reach_only_the_controlling_viewer_once_and_never_a_new_connection() {
    let mut screen = Screen::default();
    screen.publish(&Buffer::empty(Rect::new(0, 0, 10, 2)), None);
    assert!(!screen.copy("secret"), "no viewer controls the session");
    screen.controller = Some("paired".into());
    let after = frame(&screen, 0, 10, 2)["sequence"].as_u64().unwrap();
    assert!(screen.copy("notes.txt"));
    let next = frame(&screen, after, 10, 2);
    assert_eq!(next["clipboard"], STANDARD.encode("notes.txt"));
    assert!(frame(&screen, next["sequence"].as_u64().unwrap(), 10, 2).get("clipboard").is_none());
    assert!(frame(&screen, 0, 10, 2).get("clipboard").is_none(), "a new connection gets no old copy");
    let watcher = serde_json::from_str::<serde_json::Value>(&screen.frame(after, 10, 2, "watcher").unwrap().text);
    assert!(watcher.unwrap().get("clipboard").is_none(), "a view-only mirror gets no copy");
}

#[test]
fn screen_snapshots_deltas_cursor_and_size_changes_reproduce_the_host() {
    let mut screen = Screen::default();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 10));
    buffer.set_string(2, 2, "host screen", Style::default().fg(Color::Cyan).bg(Color::Blue));
    screen.publish(&buffer, Some(Position::new(3, 4)));
    let first = frame(&screen, 0, 30, 10);
    let mut viewer = Emulator::new(GridSize { cols: 30, rows: 10 }, 10);
    viewer.feed(&bytes(&first));
    assert!(viewer.screen().live_text().iter().any(|line| line.contains("host screen")));
    assert_eq!(viewer.screen().cursor(), (3, 4));
    let after = first["sequence"].as_u64().unwrap();
    buffer.set_string(2, 2, "updated    ", Style::default());
    screen.publish(&buffer, None);
    let next = frame(&screen, after, 30, 10);
    assert_eq!(next["reset"], false);
    viewer.feed(&bytes(&next));
    assert!(viewer.screen().live_text().iter().any(|line| line.contains("updated")));
    assert!(!viewer.screen().cursor_visible());
    let after = next["sequence"].as_u64().unwrap();
    let smaller = Buffer::empty(Rect::new(0, 0, 20, 8));
    screen.publish(&smaller, None);
    let resized = frame(&screen, after, 30, 10);
    assert_eq!(resized["reset"], true);
    viewer.feed(&bytes(&resized));
    assert!(!viewer.screen().live_text().iter().any(|line| line.contains("updated")));
}

#[test]
fn repeated_updates_send_the_latest_cells_without_resetting_the_whole_screen() {
    let mut screen = Screen::default();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 195, 62));
    buffer.set_style(buffer.area, Style::default().fg(Color::Cyan).bg(Color::Blue));
    screen.publish(&buffer, None);
    let first = frame(&screen, 0, 195, 62);
    let after = first["sequence"].as_u64().unwrap();
    let mut viewer = Emulator::new(GridSize { cols: 195, rows: 62 }, 10);
    viewer.feed(&bytes(&first));
    for index in 0..24 {
        let text = format!("frame {index:02} {}", char::from(b'a' + index).to_string().repeat(70));
        buffer.set_string(2, 2, text, Style::default().fg(Color::White).bg(Color::Blue));
        screen.publish(&buffer, None);
    }
    let latest = frame(&screen, after, 195, 62);
    let encoded = bytes(&latest);
    assert_eq!(latest["reset"], false);
    assert!(encoded.len() < 512, "a changed strip must not resend the full screen: {} bytes", encoded.len());
    viewer.feed(&encoded);
    let mut expected = Emulator::new(GridSize { cols: 195, rows: 62 }, 10);
    expected.feed(&bytes(&frame(&screen, 0, 195, 62)));
    assert_eq!(viewer.export_ansi(62), expected.export_ansi(62));
}

#[test]
fn slow_viewers_skip_stale_animation_and_small_viewports_do_not_scroll() {
    let mut screen = Screen::default();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 50));
    screen.publish(&buffer, None);
    let after = screen.sequence;
    for index in 0..40 {
        for y in 0..50 {
            buffer.set_string(0, y, format!("frame {index:02} {}", "x".repeat(90)), Style::default());
        }
        screen.publish(&buffer, None);
    }
    let latest = frame(&screen, after, 12, 6);
    assert_eq!(latest["reset"], false);
    let mut viewer = Emulator::new(GridSize { cols: 12, rows: 8 }, 10);
    viewer.feed(&bytes(&latest));
    assert!(viewer.screen().live_text()[0].contains("frame 39"));
    assert!(viewer.screen().live_text()[7].is_empty());
}

#[tokio::test]
async fn waiting_frames_wake_on_output_without_waiting_for_the_poll_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().to_string_lossy().into_owned();
    let host = Host::start(directory.path(), &session).unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 195, 62));
    host.publish(&buffer, None);
    let after = host.shared.screen.lock().unwrap().sequence;
    let mut call =
        Call::new("mirror_frame", json!({"session":session,"after":after,"wait_ms":1000,"cols":195,"rows":62}));
    call.origin = Some("paired".into());
    let waiting = host.shared.call(call);
    tokio::pin!(waiting);
    assert!(futures::poll!(&mut waiting).is_pending());
    buffer.set_string(1, 1, "immediate output", Style::default());
    host.publish(&buffer, None);
    let reply = match futures::poll!(&mut waiting) {
        std::task::Poll::Ready(reply) => reply,
        std::task::Poll::Pending => panic!("published output must wake a waiting frame immediately"),
    };
    assert!(reply.ok, "{}", reply.text);
    let value: serde_json::Value = serde_json::from_str(&reply.text).unwrap();
    assert!(value["sequence"].as_u64().unwrap() > after);
    assert!(!bytes(&value).is_empty());
}

#[test]
fn accumulated_wide_character_updates_preserve_the_same_screen_as_a_full_frame() {
    let mut screen = Screen::default();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 12));
    buffer.set_style(buffer.area, Style::default().fg(Color::White).bg(Color::Blue));
    screen.publish(&buffer, None);
    let first = frame(&screen, 0, 80, 12);
    let mut viewer = Emulator::new(GridSize { cols: 80, rows: 12 }, 10);
    viewer.feed(&bytes(&first));
    let mut after = first["sequence"].as_u64().unwrap();
    for texts in [["a界b", "abc", "a🙂b"], ["a界b", "a  b", "xy"], ["🙂ab", "a界", "abc界"]] {
        for text in texts {
            buffer.set_string(2, 2, " ".repeat(20), Style::default().bg(Color::Blue));
            buffer.set_string(2, 2, text, Style::default().fg(Color::Yellow).bg(Color::Blue));
            screen.publish(&buffer, None);
        }
        let update = frame(&screen, after, 80, 12);
        assert_eq!(update["reset"], false);
        viewer.feed(&bytes(&update));
        let mut expected = Emulator::new(GridSize { cols: 80, rows: 12 }, 10);
        expected.feed(&bytes(&frame(&screen, 0, 80, 12)));
        assert_eq!(viewer.export_ansi(12), expected.export_ansi(12));
        after = update["sequence"].as_u64().unwrap();
    }
}

#[tokio::test]
async fn mirror_control_requires_identity_and_the_selected_session_and_local_input_reclaims_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::start(directory.path(), "session").unwrap();
    host.publish(&Buffer::empty(Rect::new(0, 0, 30, 10)), None);
    let call = |tool: &str, origin: Option<&str>, args: serde_json::Value| {
        let mut call = Call::new(tool, args);
        call.origin = origin.map(str::to_owned);
        call
    };
    assert!(!host.shared.call(call("mirror_take", None, json!({"session":"session"}))).await.ok);
    assert!(!host.shared.call(call("mirror_take", Some("paired"), json!({"session":"other"}))).await.ok);
    assert!(host.shared.call(call("mirror_view", Some("paired"), json!({"session":"session"}))).await.ok);
    assert!(!host.controls("paired"));
    assert!(host.shared.call(call("mirror_take", Some("paired"), json!({"session":"session"}))).await.ok);
    assert!(!host.shared.call(call("mirror_take", Some("other"), json!({"session":"session"}))).await.ok);
    host.incoming.recv().await.unwrap();
    let shared = host.shared.clone();
    let event = Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    let expected = event.clone();
    let sending = tokio::spawn(async move {
        shared.call(call("mirror_input", Some("paired"), json!({"session":"session","events":[event]}))).await
    });
    let Some(Input::Events { events, reply, .. }) = host.incoming.recv().await else { panic!("the keys arrive") };
    assert_eq!(events, vec![expected]);
    reply.send(Reply::ok("sent")).unwrap();
    assert!(sending.await.unwrap().ok);
    assert!(host.local_input());
    assert!(!host.controls("paired"));
    assert!(!host.shared.call(call("mirror_input", Some("paired"), json!({"session":"session","events":[]}))).await.ok);
    assert!(host.shared.call(call("mirror_take", Some("other"), json!({"session":"session","force":true}))).await.ok);
    let mut peer = Peer::at(&socket(directory.path(), "session").unwrap()).await.unwrap();
    assert!(peer.call(&call("mirror_view", Some("paired"), json!({"session":"session"}))).await.unwrap().ok);
    let saved = host.socket.clone();
    drop(host);
    assert!(!saved.exists());
}

#[test]
fn borg_draws_at_the_controlling_viewers_size_only_where_that_is_smaller() {
    let terminal = Rect::new(0, 0, 130, 34);
    assert_eq!(fit(terminal, None), terminal);
    assert_eq!(fit(terminal, Some(Size { width: 140, height: 32 })), Rect::new(0, 0, 130, 32));
    assert_eq!(fit(terminal, Some(Size { width: 80, height: 50 })), Rect::new(0, 0, 80, 34));
    assert_eq!(viewer_size(&json!({"cols":80,"rows":24})), Some(Size { width: 80, height: 24 }));
    for args in
        [json!({"cols":80}), json!({"cols":19,"rows":24}), json!({"cols":80,"rows":201}), json!({"cols":80,"rows":-1})]
    {
        assert_eq!(viewer_size(&args), None, "{args}");
    }
}

#[tokio::test]
async fn the_controlling_viewers_window_lasts_while_it_controls() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::start(directory.path(), "fitted").unwrap();
    let terminal = Rect::new(0, 0, 130, 34);
    let call = |tool: &str, origin: &str, args: serde_json::Value| {
        let mut call = Call::new(tool, json!({"session":"fitted"}));
        call.origin = Some(origin.to_owned());
        if let (Some(call), Some(args)) = (call.args.as_object_mut(), args.as_object()) {
            call.extend(args.clone());
        }
        call
    };
    let window = json!({"cols":140,"rows":32});
    assert!(!host.shared.call(call("mirror_resize", "paired", window.clone())).await.ok, "only a controller resizes");
    assert!(host.shared.call(call("mirror_take", "paired", json!({}))).await.ok);
    assert!(matches!(host.incoming.recv().await, Some(Input::Control)));
    assert!(!host.shared.call(call("mirror_resize", "paired", json!({"cols":140,"rows":2}))).await.ok);
    assert_eq!(host.fit(terminal), terminal);
    assert!(host.shared.call(call("mirror_resize", "paired", window.clone())).await.ok);
    assert!(matches!(host.incoming.recv().await, Some(Input::Resized)));
    assert_eq!(host.fit(terminal), Rect::new(0, 0, 130, 32));
    assert!(host.shared.call(call("mirror_resize", "paired", window.clone())).await.ok);
    assert!(host.incoming.try_recv().is_err(), "the same size wakes nothing");

    assert!(host.local_input());
    assert_eq!(host.fit(terminal), terminal, "local input takes the whole terminal back");
    assert!(host.shared.call(call("mirror_take", "paired", json!({}))).await.ok);
    assert!(host.shared.call(call("mirror_resize", "paired", window.clone())).await.ok);
    assert!(host.shared.call(call("mirror_take", "other", json!({"force":true}))).await.ok);
    assert_eq!(host.fit(terminal), terminal, "another viewer brings its own window");
    assert!(host.shared.call(call("mirror_resize", "other", window)).await.ok);
    assert!(host.shared.call(call("mirror_release", "other", json!({}))).await.ok);
    assert_eq!(host.fit(terminal), terminal, "letting go gives the terminal back");
}
