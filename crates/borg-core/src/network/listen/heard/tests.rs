use super::*;
use crate::network::{InterfaceKind, NeighborState};

const OWN_MAC: &str = "02:00:00:00:00:10";
const PRINTER_MAC: &str = "02:00:00:00:00:31";
const LAPTOP_MAC: &str = "02:00:00:00:00:44";
const AT: u64 = 1_800_000_000;

fn mac(text: &str) -> Mac {
    Mac::parse(text).unwrap()
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn interface(name: &str, addresses: &[(&str, u8)], mac: Option<&str>) -> Interface {
    Interface {
        name: name.into(),
        kind: InterfaceKind::Ethernet,
        up: true,
        addresses: addresses.iter().map(|(address, prefix)| (ip(address), *prefix)).collect(),
        mac: mac.map(str::to_owned),
        mtu: None,
        received: 0,
        sent: 0,
        packets_received: 0,
        packets_sent: 0,
        errors: 0,
    }
}

/// This machine: 192.0.2.10/24 and 2001:db8:1::10/64 on en0, 198.51.100.5/24 on en1.
fn interfaces() -> Vec<Interface> {
    vec![
        interface("en0", &[("192.0.2.10", 24), ("2001:db8:1::10", 64), ("fe80::10", 64)], Some(OWN_MAC)),
        interface("en1", &[("198.51.100.5", 24)], None),
    ]
}

fn said(protocol: ListenProtocol, address: Option<&str>, name: Option<&str>) -> Announcement {
    Announcement { address: address.map(ip), name: name.map(str::to_owned), ..Announcement::new(protocol) }
}

fn hosts(hearing: &Hearing) -> Vec<String> {
    hearing.hosts.iter().map(|host| host.address.to_string()).collect()
}

#[test]
fn only_other_hosts_on_the_interface_s_networks_are_taken() {
    let mut hearing = Hearing::default();
    let heard = [
        "192.0.2.31",
        "192.0.2.10",
        "192.0.2.0",
        "192.0.2.255",
        "198.51.100.7",
        "203.0.113.9",
        "169.254.7.7",
        "224.0.0.251",
        "fe80::31",
        "2001:db8:1::31",
        "2001:db8:2::31",
        "::",
    ]
    .into_iter()
    .map(|address| Heard::Host(said(ListenProtocol::Arp, Some(address), None)))
    .collect();
    let taken = hearing.take("en0", heard, &interfaces(), AT);
    let expected = ["169.254.7.7", "192.0.2.31", "2001:db8:1::31", "fe80::31"];
    assert_eq!(taken.iter().map(ToString::to_string).collect::<Vec<_>>(), expected);
    assert_eq!(hosts(&hearing).len(), 4, "this machine, the edges, other networks and multicast are left out");
}

#[test]
fn frames_from_this_machine_s_own_hardware_are_left_out() {
    let mut hearing = Hearing::default();
    let own = Announcement { mac: Some(mac(OWN_MAC)), ..said(ListenProtocol::Mdns, Some("192.0.2.99"), Some("me")) };
    assert!(hearing.take("en0", vec![Heard::Host(own)], &interfaces(), AT).is_empty());
    assert!(hearing.hosts.is_empty());
}

#[test]
fn the_most_trusted_protocol_names_a_host_and_every_protocol_is_kept() {
    let mut hearing = Hearing::default();
    let heard = vec![
        Heard::Host(said(ListenProtocol::Netbios, Some("192.0.2.31"), Some("PRINTER"))),
        Heard::Host(said(ListenProtocol::Mdns, Some("192.0.2.31"), Some("printer.local"))),
        Heard::Host(said(ListenProtocol::Llmnr, Some("192.0.2.31"), Some("printer"))),
    ];
    hearing.take("en0", heard, &interfaces(), AT);
    let host = hearing.host(ip("192.0.2.31")).unwrap();
    assert_eq!(host.announced.name().map(|name| name.text.as_str()), Some("printer.local"));
    assert_eq!(host.announced.protocols, [ListenProtocol::Mdns, ListenProtocol::Netbios, ListenProtocol::Llmnr]);
    assert_eq!(host.announced.names.len(), 3, "one name for each protocol");
}

#[test]
fn a_model_from_a_less_trusted_protocol_leaves_a_better_one() {
    let mut announced = Announced::default();
    let ssdp = Announcement { model: Some("Linux UPnP/1.0".into()), ..Announcement::new(ListenProtocol::Ssdp) };
    let mdns = Announcement { model: Some("LaserBox 3000".into()), ..Announcement::new(ListenProtocol::Mdns) };
    assert!(announced.absorb(&ssdp));
    assert!(announced.absorb(&mdns));
    assert!(!announced.absorb(&ssdp), "SSDP's model is trusted less and tells nothing new");
    assert_eq!(announced.model.as_ref().map(|model| model.text.as_str()), Some("LaserBox 3000"));
}

#[test]
fn device_types_and_services_stop_at_the_limit() {
    let mut announced = Announced::default();
    let kinds: Vec<String> = (0..MOST_KINDS + 4).map(|index| format!("Kind{index}")).collect();
    let many = Announcement { kinds: kinds.clone(), services: kinds, ..Announcement::new(ListenProtocol::Ssdp) };
    announced.absorb(&many);
    assert_eq!((announced.kinds.len(), announced.services.len()), (MOST_KINDS, MOST_KINDS));
    assert!(announced.valid());
}

#[test]
fn what_a_dhcp_client_says_before_it_has_an_address_goes_to_the_host_with_its_mac() {
    let mut hearing = Hearing::default();
    let request = Announcement {
        mac: Some(mac(LAPTOP_MAC)),
        name: Some("laptop".into()),
        client: Some("MSFT 5.0".into()),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    let taken = hearing.take("en0", vec![Heard::Host(request)], &interfaces(), AT);
    assert!(taken.is_empty(), "no host has that MAC yet");
    let arp = Announcement { mac: Some(mac(LAPTOP_MAC)), ..said(ListenProtocol::Arp, Some("192.0.2.44"), None) };
    hearing.take("en0", vec![Heard::Host(arp)], &interfaces(), AT + 1);
    let host = hearing.host(ip("192.0.2.44")).unwrap();
    assert_eq!(host.announced.name().map(|name| name.text.as_str()), Some("laptop"));
    assert_eq!(host.announced.client.as_deref(), Some("MSFT 5.0"));

    let renew = Announcement {
        mac: Some(mac(LAPTOP_MAC)),
        name: Some("laptop-2".into()),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    let taken = hearing.take("en0", vec![Heard::Host(renew)], &interfaces(), AT + 2);
    assert_eq!(taken, [ip("192.0.2.44")], "a host heard with that MAC learns it at once");
    assert_eq!(hearing.host(ip("192.0.2.44")).unwrap().last, AT + 2);
}

#[test]
fn a_mac_heard_alone_names_the_neighbor_the_tables_list_with_it() {
    let mut hearing = Hearing::default();
    let request = Announcement {
        mac: Some(mac(PRINTER_MAC)),
        name: Some("printer".into()),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    hearing.take("en0", vec![Heard::Host(request)], &interfaces(), AT);
    let listed = |interface: &str| Neighbor {
        address: ip("192.0.2.31"),
        mac: Some(mac(PRINTER_MAC)),
        interface: interface.into(),
        state: Some(NeighborState::Reachable),
        router: false,
    };
    assert!(!hearing.place(&[listed("en1")]), "the same MAC on another link is another host");
    assert!(hearing.place(&[listed("en0")]));
    assert!(!hearing.place(&[listed("en0")]), "placing again tells nothing new");
    assert_eq!(hosts(&hearing), ["192.0.2.31"]);
    assert_eq!(hearing.host(ip("192.0.2.31")).unwrap().mac, Some(mac(PRINTER_MAC)));
}

#[test]
fn group_and_unset_macs_are_never_a_host_s() {
    let mut hearing = Hearing::default();
    let group =
        Announcement { mac: Some(mac("01:00:5e:00:00:fb")), ..said(ListenProtocol::Mdns, Some("192.0.2.31"), None) };
    let unset = Announcement { mac: Some(mac("00:00:00:00:00:00")), ..Announcement::new(ListenProtocol::Dhcp) };
    hearing.take("en0", vec![Heard::Host(group), Heard::Host(unset)], &interfaces(), AT);
    assert_eq!(hearing.host(ip("192.0.2.31")).unwrap().mac, None);
    assert!(hearing.unplaced.is_empty());
}

#[test]
fn the_switch_port_comes_from_lldp_before_cdp_and_the_newest_frame_wins() {
    let mut hearing = Hearing::default();
    let port = |protocol, name: &str, port: &str| {
        Heard::Switch(SwitchPort {
            protocol,
            name: Some(name.into()),
            port: Some(port.into()),
            model: None,
            address: None,
        })
    };
    hearing.take("en0", vec![port(ListenProtocol::Cdp, "core-sw", "Gi0/7")], &interfaces(), AT);
    assert_eq!(hearing.switch("en0").unwrap().port.port.as_deref(), Some("Gi0/7"));
    hearing.take("en0", vec![port(ListenProtocol::Lldp, "core-sw", "ge-0/0/7")], &interfaces(), AT + 1);
    hearing.take("en0", vec![port(ListenProtocol::Lldp, "core-sw", "ge-0/0/8")], &interfaces(), AT + 2);
    let switch = hearing.switch("en0").unwrap();
    assert_eq!((switch.port.port.as_deref(), switch.at), (Some("ge-0/0/8"), AT + 2));
    assert_eq!(hearing.switches.len(), 2);
    assert!(hearing.switch("en1").is_none());
}

#[test]
fn a_session_keeps_a_bounded_number_of_hosts() {
    let mut hearing = Hearing::default();
    let wide = [interface("en0", &[("10.0.0.10", 16)], None)];
    let heard: Vec<Heard> = (0..MOST_HOSTS + 10)
        .map(|index| {
            let address = Ipv4Addr::new(10, 0, (index / 250) as u8 + 1, (index % 250) as u8 + 1);
            Heard::Host(Announcement { address: Some(address.into()), ..Announcement::new(ListenProtocol::Arp) })
        })
        .collect();
    let taken = hearing.take("en0", heard, &wide, AT);
    assert_eq!((taken.len(), hearing.hosts.len()), (MOST_HOSTS, MOST_HOSTS));
}

#[test]
fn forgetting_drops_a_heard_host() {
    let mut hearing = Hearing::default();
    hearing.take("en0", vec![Heard::Host(said(ListenProtocol::Arp, Some("192.0.2.31"), None))], &interfaces(), AT);
    assert!(hearing.forget(ip("192.0.2.31")));
    assert!(!hearing.forget(ip("192.0.2.31")));
    assert!(hearing.hosts.is_empty());
}

#[test]
fn a_remembered_announcement_that_breaks_the_rules_is_invalid() {
    let named = |protocol, text: &str| Said { protocol, text: text.into() };
    let good = Announced {
        names: vec![named(ListenProtocol::Mdns, "printer.local")],
        protocols: vec![ListenProtocol::Mdns],
        ..Announced::default()
    };
    assert!(good.valid());
    let twice =
        Announced { names: vec![named(ListenProtocol::Mdns, "a"), named(ListenProtocol::Mdns, "b")], ..good.clone() };
    let long = Announced { names: vec![named(ListenProtocol::Mdns, &"x".repeat(LONGEST_TEXT + 1))], ..good.clone() };
    let empty = Announced { client: Some(String::new()), ..good.clone() };
    let kinds = Announced { kinds: vec!["Kind".into(); MOST_KINDS + 1], ..good };
    for bad in [twice, long, empty, kinds] {
        assert!(!bad.valid(), "{bad:?}");
    }
}
