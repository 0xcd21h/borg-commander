use std::path::PathBuf;

use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;

const WAIT: Duration = Duration::from_millis(200);

fn registration(session: &str, place: Place) -> Registration {
    Registration { session: session.into(), pid: 1, mirror: PathBuf::from(format!("/run/{session}")), place }
}

fn member(members: &Members, session: &str, place: Place) -> (u64, UnboundedReceiver<ToMember>) {
    let (sender, receiver) = unbounded_channel();
    (members.join(registration(session, place), sender), receiver)
}

/// The calls a member got, answered with their tool's name, and the counts of agents it heard.
fn answer_all(members: &Members, received: &mut UnboundedReceiver<ToMember>) -> Vec<usize> {
    let mut agents = Vec::new();
    while let Ok(message) = received.try_recv() {
        match message {
            ToMember::Call { id, call } => members.replied(id, Reply::ok(call.tool)),
            ToMember::Agents { count } => agents.push(count),
            _ => {}
        }
    }
    agents
}

#[tokio::test]
async fn a_call_reaches_the_borg_of_its_session_with_its_origin_and_comes_back() {
    let members = Members::default();
    let (_, mut work) = member(&members, "work", Place::Terminal);
    let mut call = Call::new("screen", json!({}));
    call.origin = Some("laptop".into());
    let asking = tokio::spawn({
        let members = members.clone();
        async move { members.ask("work", call).await }
    });
    let (id, call) = loop {
        if let ToMember::Call { id, call } = work.recv().await.expect("a call") {
            break (id, call);
        }
    };
    assert_eq!(call.origin.as_deref(), Some("laptop"), "the service keeps who asked");
    members.replied(id, Reply::ok("seen"));
    assert_eq!(asking.await.unwrap().text, "seen");
    assert!(!members.ask("elsewhere", Call::new("screen", json!({}))).await.ok);
}

#[tokio::test]
async fn calls_a_borg_left_unanswered_fail_when_it_goes() {
    let members = Members::default();
    let (token, _work) = member(&members, "work", Place::Terminal);
    let asking = tokio::spawn({
        let members = members.clone();
        async move { members.ask("work", Call::new("screen", json!({}))).await }
    });
    tokio::task::yield_now().await;
    members.leave("work", token);
    assert!(!asking.await.unwrap().ok);
    assert_eq!(members.place("work"), None);
}

#[tokio::test]
async fn calls_left_with_a_replaced_borg_fail_when_it_goes() {
    let members = Members::default();
    let (old, _old) = member(&members, "work", Place::Terminal);
    let asking = tokio::spawn({
        let members = members.clone();
        async move { members.ask("work", Call::new("screen", json!({}))).await }
    });
    tokio::task::yield_now().await;
    let _new = member(&members, "work", Place::Terminal);
    members.leave("work", old);
    let reply = tokio::time::timeout(WAIT, asking).await.expect("the call ends").unwrap();
    assert!(!reply.ok);
    assert!(members.place("work").is_some(), "the new registration stays");
}

#[test]
fn a_new_registration_replaces_the_old_one_which_cannot_end_it() {
    let members = Members::default();
    let (old, _) = member(&members, "work", Place::Background);
    let (_, _new) = member(&members, "work", Place::Terminal);
    members.leave("work", old);
    assert_eq!(members.place("work"), Some((Place::Terminal, PathBuf::from("/run/work"))));
}

#[test]
fn the_session_used_last_is_the_one_the_user_worked_in_last() {
    let members = Members::default();
    let _work = member(&members, "work", Place::Terminal);
    let _logs = member(&members, "logs", Place::Background);
    assert_eq!(members.last_used().as_deref(), Some("logs"), "a Borg that registers counts as used");
    members.used("work");
    assert_eq!(members.last_used().as_deref(), Some("work"));
    let mut places = members.sessions();
    places.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(places, [("logs".into(), Place::Background), ("work".into(), Place::Terminal)]);
}

#[test]
fn a_borg_hears_how_many_paired_connections_reach_it_even_after_it_registers_again() {
    let members = Members::default();
    let (_, mut first) = member(&members, "work", Place::Terminal);
    let lease = members.lease("work");
    let other = members.lease("work");
    assert_eq!(answer_all(&members, &mut first), [0, 1, 2]);
    drop(other);
    let (_, mut second) = member(&members, "work", Place::Terminal);
    assert_eq!(answer_all(&members, &mut second), [1]);
    drop(lease);
    assert_eq!(answer_all(&members, &mut second), [0]);
}

#[tokio::test]
async fn waiting_for_a_session_ends_when_its_borg_registers() {
    let members = Members::default();
    assert!(!members.joined("work", WAIT).await);
    let waiting = tokio::spawn({
        let members = members.clone();
        async move { members.joined("work", WAIT * 10).await }
    });
    tokio::task::yield_now().await;
    let _work = member(&members, "work", Place::Background);
    assert!(waiting.await.unwrap());
}
