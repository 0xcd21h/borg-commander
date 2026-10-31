use super::*;
use anyhow::Result;
use borg_link::hex;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn paired_client_status_counts_connections_per_identity_and_cleans_up_on_drop() {
    let computer = Computer { name: "laptop".into(), key: hex(&[7; 32]) };
    let shared = Arc::new(Mutex::new(Shared {
        computers: vec![computer.clone(), Computer { name: "offline".into(), key: hex(&[8; 32]) }],
        pairing: None,
        connections: Default::default(),
    }));
    let clients = Arc::new(AtomicUsize::new(0));
    let (messages, _) = tokio::sync::mpsc::unbounded_channel();
    let first = ClientConnection::new(shared.clone(), clients.clone(), messages.clone(), computer.clone());
    let second = ClientConnection::new(shared.clone(), clients.clone(), messages, computer);
    let status = client_status(&shared.lock().unwrap());
    assert_eq!(status.len(), 2);
    assert_eq!(status[0].connections, Some(2));
    assert_eq!(status[1].connections, Some(0));
    assert_ne!(status[0].fingerprint, status[1].fingerprint);
    drop(first);
    assert_eq!(client_status(&shared.lock().unwrap())[0].connections, Some(1));
    drop(second);
    assert_eq!(client_status(&shared.lock().unwrap())[0].connections, Some(0));
    assert_eq!(clients.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn paired_clients_remain_visible_when_the_status_owner_is_unavailable() -> Result<()> {
    let state = tempfile::tempdir()?;
    save(&state.path().join(LINK_DIR).join(COMPUTERS_FILE), &[Computer { name: "laptop".into(), key: hex(&[7; 32]) }])?;
    let clients = paired_clients(state.path()).await;
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].name, "laptop");
    assert_eq!(clients[0].connections, None);
    Ok(())
}

#[test]
fn one_borg_on_a_machine_has_borg_cloud_and_others_say_which() {
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(state.path().join(LINK_DIR)).unwrap();
    let first = claim(state.path()).unwrap();
    let error = claim(state.path()).unwrap_err().to_string();
    assert!(error.contains(&format!("PID {}", std::process::id())), "{error}");
    drop(first);
    // A process another test forks holds a copy of the file until it runs its program.
    let deadline = Instant::now() + Duration::from_secs(5);
    while claim(state.path()).is_err() {
        assert!(Instant::now() < deadline, "the claim ends with the borg that held it");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn secrets_compare_whole_and_names_stay_short_and_printable() {
    assert!(same(b"abcd", b"abcd") && !same(b"abcd", b"abce") && !same(b"abc", b"abcd"));
    assert_eq!(clean_name("  dev\u{7}box  "), "devbox");
    assert_eq!(clean_name(""), "computer");
    assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_LIMIT);
}
