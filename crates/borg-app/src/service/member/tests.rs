use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{UnixListener, unix::OwnedReadHalf},
};

use super::*;

const WAIT: Duration = Duration::from_secs(5);

struct Borg {
    membership: Membership,
    calls: UnboundedReceiver<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedReceiver<Msg>,
    clients: Arc<AtomicUsize>,
}

fn borg(socket: PathBuf) -> Borg {
    let (calls, calls_rx) = unbounded_channel();
    let (messages, messages_rx) = unbounded_channel();
    let clients = Arc::new(AtomicUsize::new(1));
    let registration =
        Registration { session: "work".into(), pid: 7, mirror: PathBuf::from("/run/work"), place: Place::Terminal };
    let inbox = Inbox { calls, messages, clients: clients.clone() };
    Borg {
        membership: Membership::start_at(socket, registration, inbox),
        calls: calls_rx,
        messages: messages_rx,
        clients,
    }
}

async fn next<T: serde::de::DeserializeOwned>(lines: &mut Lines<BufReader<OwnedReadHalf>>) -> T {
    let line = tokio::time::timeout(WAIT, lines.next_line()).await.expect("in time").unwrap().expect("a line");
    serde_json::from_str(&line).unwrap()
}

async fn send(write: &mut tokio::net::unix::OwnedWriteHalf, message: ToMember) {
    write.write_all(wire::line(&message).unwrap().as_bytes()).await.unwrap();
}

async fn until(what: &str, done: impl Fn() -> bool) {
    tokio::time::timeout(WAIT, async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what}"));
}

#[tokio::test]
async fn a_borg_registers_answers_the_services_calls_and_asks_for_pairing_through_it() {
    let folder = tempfile::tempdir().unwrap();
    let socket = folder.path().join("service.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut borg = borg(socket);
    let (stream, _) = listener.accept().await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let request: serde_json::Value = next(&mut lines).await;
    assert_eq!(request["mode"], REGISTER);
    assert_eq!(serde_json::from_value::<Registration>(request).unwrap().place, Place::Terminal);
    borg.membership.pair();
    assert!(!borg.membership.registered(), "registered once the service answers");
    tokio::time::sleep(Duration::from_millis(50)).await;

    send(&mut write, ToMember::Computers { names: vec!["laptop".into()] }).await;
    until("registered", || borg.membership.registered()).await;
    let Some(Msg::Agent(AgentEvent::Paired { computers })) = borg.messages.recv().await else { panic!("paired") };
    assert_eq!(computers, ["laptop"]);
    assert!(matches!(next(&mut lines).await, FromMember::Pair), "a request waits for the service's answer");

    let mut call = Call::new("screen", json!({}));
    call.origin = Some("laptop".into());
    send(&mut write, ToMember::Call { id: 3, call }).await;
    let (call, reply) = borg.calls.recv().await.unwrap();
    assert_eq!(call.origin.as_deref(), Some("laptop"), "the call keeps who asked");
    reply.send(Reply::ok("panels")).unwrap();
    let FromMember::Reply { id, reply } = next(&mut lines).await else { panic!("a reply") };
    assert_eq!((id, reply.text.as_str()), (3, "panels"));

    send(&mut write, ToMember::Link { link: Ok("borg://relay/link".into()) }).await;
    let Some(Msg::Agent(AgentEvent::Pairing(link))) = borg.messages.recv().await else { panic!("a link") };
    assert_eq!(link.as_deref(), Ok("borg://relay/link"));

    send(&mut write, ToMember::Agents { count: 2 }).await;
    until("the paired connections count", || borg.clients.load(Ordering::SeqCst) == 3).await;
    drop(write);
    drop(lines);
    until("unregistered once the service goes", || !borg.membership.registered()).await;
    assert_eq!(borg.clients.load(Ordering::SeqCst), 1, "only the Borg's own agents remain");
}
