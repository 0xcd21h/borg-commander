//! The kernel's own report of every process that starts, over netlink's proc connector. The
//! kernel sends a datagram for each fork, exec and exit, which needs root or `CAP_NET_ADMIN`.
//! Reading the bytes is a pure function, so its tests run on any system.

use std::io;

/// What the kernel says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcEvent {
    /// A new process, not another thread of one already running.
    Forked {
        parent: u32,
        child: u32,
    },
    /// A process replaced its program with another.
    Executed {
        pid: u32,
    },
    Exited {
        pid: u32,
    },
}

/// Bytes of the netlink message header: length, type, flags, sequence and port.
const MESSAGE_HEADER: usize = 16;
/// Where the message's type sits in its header.
const MESSAGE_TYPE: usize = 4;
/// A netlink message carrying a payload rather than an error or an end marker.
const MESSAGE_DONE: u16 = 3;
/// Messages follow one another on this boundary.
const MESSAGE_ALIGN: usize = 4;
/// Bytes of the connector header: which connector, its sequence, and the payload's length.
const CONNECTOR_HEADER: usize = 20;
/// The process connector, and the one value it uses.
const PROC_CONNECTOR: u32 = 1;
/// Bytes of the event header before the event's own fields: what happened, which processor,
/// and when.
const EVENT_HEADER: usize = 16;
const EVENT_FORK: u32 = 1;
const EVENT_EXEC: u32 = 2;
const EVENT_EXIT: u32 = 0x8000_0000;
/// The message that asks the connector to start sending events.
#[cfg_attr(all(not(target_os = "linux"), not(test)), allow(dead_code))]
const LISTEN: u32 = 1;
/// A fork's fields: the parent's thread and process, then the child's.
const FORK_PARENT_PROCESS: usize = 4;
const FORK_CHILD_THREAD: usize = 8;
const FORK_CHILD_PROCESS: usize = 12;
/// An exec's or exit's fields: the thread, then the process.
const EVENT_PROCESS: usize = 4;
/// How much of one datagram Borg reads.
pub const DATAGRAM: usize = 8 * 1024;

fn number(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + size_of::<u32>())?;
    Some(u32::from_ne_bytes(slice.try_into().ok()?))
}

fn short(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + size_of::<u16>())?;
    Some(u16::from_ne_bytes(slice.try_into().ok()?))
}

/// Every process event in one datagram, leaving out the messages that are something else. A
/// datagram cut short ends the reading rather than guessing at the rest.
pub fn parse_events(bytes: &[u8]) -> Vec<ProcEvent> {
    let mut events = Vec::new();
    let mut at = 0;
    while let Some(length) = number(bytes, at).map(|length| length as usize) {
        if length < MESSAGE_HEADER || at.saturating_add(length) > bytes.len() {
            break;
        }
        if let Some(event) = parse_message(&bytes[at..at + length]) {
            events.push(event);
        }
        at += length.next_multiple_of(MESSAGE_ALIGN);
    }
    events
}

fn parse_message(message: &[u8]) -> Option<ProcEvent> {
    if short(message, MESSAGE_TYPE)? != MESSAGE_DONE {
        return None;
    }
    let connector = message.get(MESSAGE_HEADER..)?;
    if number(connector, 0)? != PROC_CONNECTOR || number(connector, size_of::<u32>())? != PROC_CONNECTOR {
        return None;
    }
    let event = connector.get(CONNECTOR_HEADER..)?;
    let fields = event.get(EVENT_HEADER..)?;
    match number(event, 0)? {
        EVENT_FORK => {
            let child = number(fields, FORK_CHILD_PROCESS)?;
            let thread = number(fields, FORK_CHILD_THREAD)?;
            let parent = number(fields, FORK_PARENT_PROCESS)?;
            (thread == child).then_some(ProcEvent::Forked { parent, child })
        }
        EVENT_EXEC => Some(ProcEvent::Executed { pid: number(fields, EVENT_PROCESS)? }),
        EVENT_EXIT => Some(ProcEvent::Exited { pid: number(fields, EVENT_PROCESS)? }),
        _ => None,
    }
}

/// The message that asks the process connector for its events.
#[cfg_attr(all(not(target_os = "linux"), not(test)), allow(dead_code))]
pub fn listen_message() -> Vec<u8> {
    let payload = size_of::<u32>();
    let length = (MESSAGE_HEADER + CONNECTOR_HEADER + payload) as u32;
    let mut message = Vec::with_capacity(length as usize);
    message.extend_from_slice(&length.to_ne_bytes());
    message.extend_from_slice(&MESSAGE_DONE.to_ne_bytes());
    message.extend_from_slice(&0_u16.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    message.extend_from_slice(&std::process::id().to_ne_bytes());
    message.extend_from_slice(&PROC_CONNECTOR.to_ne_bytes());
    message.extend_from_slice(&PROC_CONNECTOR.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    message.extend_from_slice(&(payload as u16).to_ne_bytes());
    message.extend_from_slice(&0_u16.to_ne_bytes());
    message.extend_from_slice(&LISTEN.to_ne_bytes());
    message
}

/// A socket the kernel sends every process event to. It needs root or `CAP_NET_ADMIN`, and
/// says so when it is refused.
#[cfg(target_os = "linux")]
pub fn subscribe() -> io::Result<std::os::fd::OwnedFd> {
    use rustix::net::{
        AddressFamily, SendFlags, SocketFlags, SocketType, bind,
        netlink::{self, SocketAddrNetlink},
        send, socket_with,
    };
    let socket =
        socket_with(AddressFamily::NETLINK, SocketType::DGRAM, SocketFlags::CLOEXEC, Some(netlink::CONNECTOR))?;
    bind(&socket, &SocketAddrNetlink::new(0, PROC_CONNECTOR))?;
    send(&socket, &listen_message(), SendFlags::empty())?;
    Ok(socket)
}

#[cfg(not(target_os = "linux"))]
pub fn subscribe() -> io::Result<std::os::fd::OwnedFd> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "process events from the kernel are a Linux feature"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One netlink message holding one process event, as the kernel writes it.
    fn message(what: u32, fields: &[u32]) -> Vec<u8> {
        let payload = EVENT_HEADER + std::mem::size_of_val(fields);
        let length = (MESSAGE_HEADER + CONNECTOR_HEADER + payload) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&length.to_ne_bytes());
        bytes.extend_from_slice(&MESSAGE_DONE.to_ne_bytes());
        bytes.extend_from_slice(&0_u16.to_ne_bytes());
        bytes.extend_from_slice(&7_u32.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(&PROC_CONNECTOR.to_ne_bytes());
        bytes.extend_from_slice(&PROC_CONNECTOR.to_ne_bytes());
        bytes.extend_from_slice(&1_u32.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(&(payload as u16).to_ne_bytes());
        bytes.extend_from_slice(&0_u16.to_ne_bytes());
        bytes.extend_from_slice(&what.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(&0_u64.to_ne_bytes());
        for field in fields {
            bytes.extend_from_slice(&field.to_ne_bytes());
        }
        while bytes.len() % MESSAGE_ALIGN != 0 {
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn forks_execs_and_exits_read_as_the_processes_they_are_about() {
        let fork = message(EVENT_FORK, &[1000, 1000, 2000, 2000]);
        assert_eq!(parse_events(&fork), [ProcEvent::Forked { parent: 1000, child: 2000 }]);
        let exec = message(EVENT_EXEC, &[2000, 2000]);
        assert_eq!(parse_events(&exec), [ProcEvent::Executed { pid: 2000 }]);
        let exit = message(EVENT_EXIT, &[2000, 2000, 0, 9, 1000, 1000]);
        assert_eq!(parse_events(&exit), [ProcEvent::Exited { pid: 2000 }]);
    }

    #[test]
    fn a_new_thread_of_a_running_process_is_not_a_new_process() {
        let thread = message(EVENT_FORK, &[1000, 1000, 2001, 2000]);
        assert_eq!(parse_events(&thread), Vec::new());
    }

    #[test]
    fn several_events_in_one_datagram_all_read() {
        let mut datagram = message(EVENT_FORK, &[1, 1, 42, 42]);
        datagram.extend(message(EVENT_EXEC, &[42, 42]));
        datagram.extend(message(EVENT_EXIT, &[42, 42, 0, 0, 1, 1]));
        assert_eq!(
            parse_events(&datagram),
            [
                ProcEvent::Forked { parent: 1, child: 42 },
                ProcEvent::Executed { pid: 42 },
                ProcEvent::Exited { pid: 42 }
            ]
        );
    }

    #[test]
    fn messages_that_are_not_process_events_are_left_out() {
        let mut other = message(EVENT_FORK, &[1, 1, 42, 42]);
        other[MESSAGE_TYPE] = 2;
        assert_eq!(parse_events(&other), Vec::new(), "an error message carries no event");
        let mut elsewhere = message(EVENT_EXEC, &[42, 42]);
        elsewhere[MESSAGE_HEADER] = 9;
        assert_eq!(parse_events(&elsewhere), Vec::new(), "another connector's message is not ours");
        let unknown = message(0x40, &[42, 42]);
        assert_eq!(parse_events(&unknown), Vec::new(), "an event Borg does not read says nothing");
    }

    #[test]
    fn a_datagram_cut_anywhere_reads_what_it_holds_without_panicking() {
        let mut datagram = message(EVENT_FORK, &[1, 1, 42, 42]);
        datagram.extend(message(EVENT_EXEC, &[42, 42]));
        for end in 0..datagram.len() {
            let events = parse_events(&datagram[..end]);
            assert!(events.len() <= 2, "{end}: {events:?}");
        }
        let claims_more = {
            let mut bytes = message(EVENT_EXEC, &[42, 42]);
            let longer = (bytes.len() * 4) as u32;
            bytes[..size_of::<u32>()].copy_from_slice(&longer.to_ne_bytes());
            bytes
        };
        assert_eq!(parse_events(&claims_more), Vec::new());
        let nonsense = [0_u8; 7];
        assert_eq!(parse_events(&nonsense), Vec::new());
    }

    #[test]
    fn the_listen_message_says_its_own_length_and_asks_the_process_connector() {
        let message = listen_message();
        assert_eq!(number(&message, 0), Some(message.len() as u32));
        assert_eq!(short(&message, MESSAGE_TYPE), Some(MESSAGE_DONE));
        assert_eq!(number(&message, MESSAGE_HEADER), Some(PROC_CONNECTOR));
        assert_eq!(number(&message, MESSAGE_HEADER + CONNECTOR_HEADER), Some(LISTEN));
    }
}
