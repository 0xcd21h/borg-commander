use super::{kinds, netstat, proc_net};
use borg_core::{InterfaceKind, Socket, TcpState, Transport};
use std::{collections::HashMap, net::SocketAddr};

const NETSTAT: &str = "\
Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)          rxbytes      txbytes  rhiwat  shiwat          process:pid    state  options           gencnt    flags   flags1 usecnt rtncnt fltrs
tcp4       0      0  192.0.2.10.50123       198.51.100.7.443       ESTABLISHED        10518         2309  131072  131600   Web Helper:8239   00102 00000008 0000000001bd4b13 20000080 04000900      3      0 000004
tcp46      0      0  *.8080                 *.*                    LISTEN                 0            0  131072  131072  devserver:4100  00100 00000006 0000000001bd4aee 00000080 01000800      2      0 000000
tcp6       0      0  ::1.3010               ::1.57325              ESTABLISHED        11400         8538  405530  146808 Code Helper (Plu:90552  00102 00000004 0000000001bd4aeb 00000080 01000800      2      0 000000
tcp6       0      0  fe80::1%lo0.631        *.*                    LISTEN                 0            0  131072  131072  cupsd:301  00100 00000006 0000000001bd4aea 00000080 01000800      2      0 000000
udp4       0      0  192.0.2.10.62556       203.0.113.9.443                           51684         8790 1048576   29040   Web Helper:8239   00102 00000000 0000000001bd4b0d 20000000 04200900      2      0 000002
udp4       0      0  *.5353                 *.*                                       45217         8804       0    9216    mDNSResponder:44716  ffffa130 00020000 0000000001bc7854 20100009 04280908      1      0 000002
kctl       0      0      1     12 com.apple.netsrc

Active LOCAL (UNIX) domain sockets
";

fn address(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

#[test]
fn netstat_lists_every_socket_with_its_process_and_counts() {
    let (sockets, names) = netstat::parse(NETSTAT);
    assert_eq!(sockets.len(), 6);
    assert_eq!(
        sockets[0],
        Socket {
            transport: Transport::Tcp,
            local: address("192.0.2.10:50123"),
            remote: Some(address("198.51.100.7:443")),
            state: Some(TcpState::Established),
            pid: Some(8239),
            received: Some(10518),
            sent: Some(2309),
        }
    );
    assert_eq!(names[&8239], "Web Helper");
    assert_eq!(names[&90552], "Code Helper (Plu");
}

#[test]
fn netstat_reads_any_address_by_the_socket_family() {
    let (sockets, _) = netstat::parse(NETSTAT);
    let listener = &sockets[1];
    assert_eq!(listener.local, address("[::]:8080"));
    assert_eq!(listener.remote, None);
    assert!(listener.listens());
    assert_eq!(sockets[5].local, address("0.0.0.0:5353"));
    assert!(sockets[5].listens());
}

#[test]
fn netstat_drops_the_scope_of_a_link_local_address() {
    let (sockets, _) = netstat::parse(NETSTAT);
    assert_eq!(sockets[3].local, address("[fe80::1]:631"));
    assert_eq!(sockets[2].remote, Some(address("[::1]:57325")));
}

#[test]
fn netstat_gives_udp_no_state_and_keeps_its_peer() {
    let (sockets, _) = netstat::parse(NETSTAT);
    let udp = &sockets[4];
    assert_eq!(udp.state, None);
    assert_eq!(udp.remote, Some(address("203.0.113.9:443")));
    assert!(udp.connects());
}

const PROC_TCP: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41001 1 0000000000000000 100 0 0 10 0
   1: 0A0200C0:C350 076433C6:01BB 01 00000000:00000000 02:000A7D8F 00000000  1000        0 41002 2 0000000000000000 20 4 30 10 -1
";

const PROC_TCP6: &str = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 41003 1 0000000000000000 100 0 0 10 0
   1: B80D0120000000000000000001000000:01BB B80D0120000000000000000009000000:D431 06 00000000:00000000 03:00000F9A 00000000     0        0 0 3 0000000000000000
";

const PROC_UDP: &str = "\
   sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops
  0: 00000000:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000   100        0 41004 2 0000000000000000 0
";

#[test]
fn proc_net_reads_addresses_in_the_kernel_byte_order() {
    let tcp = proc_net::parse_table(PROC_TCP, Transport::Tcp);
    assert_eq!(tcp.len(), 2);
    assert_eq!(tcp[0].local, address("127.0.0.1:8080"));
    assert_eq!(tcp[0].remote, None);
    assert_eq!(tcp[0].state, Some(TcpState::Listen));
    assert_eq!(tcp[1].local, address("192.0.2.10:50000"));
    assert_eq!(tcp[1].remote, Some(address("198.51.100.7:443")));
    assert_eq!(tcp[1].state, Some(TcpState::Established));
    assert_eq!(tcp[1].inode, 41002);
}

#[test]
fn proc_net_reads_ipv6_word_by_word() {
    let tcp6 = proc_net::parse_table(PROC_TCP6, Transport::Tcp);
    assert_eq!(tcp6[0].local, address("[::]:22"));
    assert_eq!(tcp6[1].local, address("[2001:db8::1]:443"));
    assert_eq!(tcp6[1].remote, Some(address("[2001:db8::9]:54321")));
    assert_eq!(tcp6[1].state, Some(TcpState::TimeWait));
}

#[test]
fn proc_net_gives_udp_no_state() {
    let udp = proc_net::parse_table(PROC_UDP, Transport::Udp);
    assert_eq!(udp[0].local, address("0.0.0.0:5353"));
    assert_eq!(udp[0].state, None);
}

const SS: &str = "\
ESTAB 0      0      192.0.2.10:50000 198.51.100.7:443 uid:1000 ino:41002 sk:1 cgroup:/user.slice <->
\t cubic wscale:7,7 rto:204 rtt:3.2/1.1 mss:1448 bytes_sent:2600 bytes_acked:2500 bytes_received:9000 segs_out:20 segs_in:22
TIME-WAIT 0      0      192.0.2.10:49999 198.51.100.7:443 timer:(timewait,50sec,0) ino:0 sk:2
";

#[test]
fn ss_counts_bytes_by_inode() {
    let counts = proc_net::parse_counts(SS);
    assert_eq!(counts[&41002], (9000, 2500));
    assert!(!counts.contains_key(&0));
}

#[test]
fn proc_net_joins_processes_and_counts_by_inode() {
    let listed = proc_net::parse_table(PROC_TCP, Transport::Tcp);
    let owners = HashMap::from([(41001, 300), (41002, 301)]);
    let counts = proc_net::parse_counts(SS);
    let sockets = proc_net::assemble(listed, &owners, Some(&counts));
    assert_eq!(sockets[0].pid, Some(300));
    assert_eq!((sockets[0].received, sockets[0].sent), (Some(0), Some(0)));
    assert_eq!((sockets[1].received, sockets[1].sent), (Some(9000), Some(2500)));
    let udp = proc_net::assemble(proc_net::parse_table(PROC_UDP, Transport::Udp), &owners, Some(&counts));
    assert_eq!((udp[0].pid, udp[0].received), (None, None));
}

#[test]
fn proc_net_leaves_bytes_unknown_without_ss() {
    let sockets = proc_net::assemble(proc_net::parse_table(PROC_TCP, Transport::Tcp), &HashMap::new(), None);
    assert_eq!(sockets[1].received, None);
}

#[test]
fn an_open_file_names_its_socket() {
    assert_eq!(proc_net::socket_inode("socket:[41002]"), Some(41002));
    assert_eq!(proc_net::socket_inode("/dev/null"), None);
    assert_eq!(proc_net::socket_inode("pipe:[41002]"), None);
}

#[test]
fn hardware_ports_name_wifi_bridges_and_wires() {
    let output = "\
Hardware Port: Ethernet Adapter (en4)
Device: en4
Ethernet Address: 02:00:00:00:00:04

Hardware Port: Wi-Fi
Device: en0
Ethernet Address: 02:00:00:00:00:00

Hardware Port: Thunderbolt Bridge
Device: bridge0
Ethernet Address: N/A

VLAN Configurations
===================
";
    let kinds = kinds::hardware_ports(output);
    assert_eq!(kinds["en4"], InterfaceKind::Ethernet);
    assert_eq!(kinds["en0"], InterfaceKind::WiFi);
    assert_eq!(kinds["bridge0"], InterfaceKind::Bridge);
    assert_eq!(kinds.len(), 3);
    assert_eq!(kinds::kind("en0", &kinds), InterfaceKind::WiFi);
    assert_eq!(kinds::kind("utun3", &kinds), InterfaceKind::Tunnel);
}
