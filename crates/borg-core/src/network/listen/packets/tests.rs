use super::super::dns::{self, build};
use super::*;

const PRINTER_MAC: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x44];
const ROUTER_MAC: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x01];
const OWN_MAC: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x20];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];
const MDNS_MAC: [u8; 6] = [0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb];
const SSDP_MAC: [u8; 6] = [0x01, 0x00, 0x5e, 0x7f, 0xff, 0xfa];
const ALL_NODES_MAC: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x01];
const LLDP_MAC: [u8; 6] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x0e];
const ROUTER: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const OWN: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 20);
const LAPTOP: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 52);
const PRINTER: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 44);
const SPEAKER: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 61);
const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const SSDP_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const LLMNR_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 252);
const RESPONSE: u16 = 0x8400;
const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28;
const TYPE_SRV: u16 = 33;

fn frame(destination: [u8; 6], source: [u8; 6], kind: u16, body: &[u8]) -> Vec<u8> {
    let mut frame = [destination, source].concat();
    frame.extend(kind.to_be_bytes());
    frame.extend(body);
    frame
}

fn udp(ports: (u16, u16), payload: &[u8]) -> Vec<u8> {
    let mut datagram = Vec::new();
    datagram.extend(ports.0.to_be_bytes());
    datagram.extend(ports.1.to_be_bytes());
    datagram.extend(u16::try_from(8 + payload.len()).unwrap().to_be_bytes());
    datagram.extend([0, 0]);
    datagram.extend(payload);
    datagram
}

fn ipv4(source: Ipv4Addr, destination: Ipv4Addr, datagram: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x45, 0];
    packet.extend(u16::try_from(20 + datagram.len()).unwrap().to_be_bytes());
    packet.extend([0, 0, 0, 0, 255, 17, 0, 0]);
    packet.extend(source.octets());
    packet.extend(destination.octets());
    packet.extend(datagram);
    packet
}

fn ipv6(source: Ipv6Addr, destination: Ipv6Addr, next: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x60, 0, 0, 0];
    packet.extend(u16::try_from(payload.len()).unwrap().to_be_bytes());
    packet.extend([next, 255]);
    packet.extend(source.octets());
    packet.extend(destination.octets());
    packet.extend(payload);
    packet
}

/// A UDP datagram over IPv4 in an Ethernet frame from `PRINTER_MAC` to `destination`.
fn over_udp(destination: [u8; 6], ends: (Ipv4Addr, Ipv4Addr), ports: (u16, u16), payload: &[u8]) -> Vec<u8> {
    frame(destination, PRINTER_MAC, 0x0800, &ipv4(ends.0, ends.1, &udp(ports, payload)))
}

/// What a frame announces, after checking that every cut of it reads without trouble.
fn heard(frame: &[u8]) -> Vec<Heard> {
    for cut in 0..frame.len() {
        hear(LINK_ETHERNET, &frame[..cut]);
    }
    hear(LINK_ETHERNET, frame)
}

fn hosts(heard: &[Heard]) -> Vec<Announcement> {
    heard
        .iter()
        .filter_map(|heard| match heard {
            Heard::Host(announcement) => Some(announcement.clone()),
            Heard::Switch(_) => None,
        })
        .collect()
}

fn host(protocol: ListenProtocol, address: impl Into<IpAddr>) -> Announcement {
    Announcement::of(protocol, address.into())
}

fn named(protocol: ListenProtocol, address: impl Into<IpAddr>, name: &str) -> Announcement {
    Announcement { name: Some(name.to_owned()), ..host(protocol, address) }
}

#[test]
fn gratuitous_arp_gives_the_sender() {
    let mut arp = vec![0, 1, 8, 0, 6, 4, 0, 1];
    arp.extend(PRINTER_MAC);
    arp.extend(PRINTER.octets());
    arp.extend([0; 6]);
    arp.extend(PRINTER.octets());
    let found = heard(&frame(BROADCAST_MAC, PRINTER_MAC, 0x0806, &arp));
    assert_eq!(
        found,
        vec![Heard::Host(Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Arp, PRINTER) })]
    );
}

#[test]
fn an_arp_probe_from_no_address_gives_nothing() {
    let mut arp = vec![0, 1, 8, 0, 6, 4, 0, 1];
    arp.extend(PRINTER_MAC);
    arp.extend([0; 4]);
    arp.extend([0; 6]);
    arp.extend(PRINTER.octets());
    assert!(heard(&frame(BROADCAST_MAC, PRINTER_MAC, 0x0806, &arp)).is_empty());
}

fn printer_response() -> Vec<u8> {
    let instance = "Office Printer._ipp._tcp.local";
    let answers = [
        build::record("_ipp._tcp.local", TYPE_PTR, &build::name(instance)),
        build::record(instance, TYPE_SRV, &build::service("office-printer.local")),
        build::record(instance, TYPE_TXT, &build::text(&["txtvers=1", "ty=Example LaserJet 400", "note=Hall"])),
    ];
    let additional = [
        build::record("office-printer.local", TYPE_A, &PRINTER.octets()),
        build::record("office-printer.local", TYPE_AAAA, &"fe80::44".parse::<Ipv6Addr>().unwrap().octets()),
    ];
    build::message(RESPONSE, None, [&answers, &[], &additional])
}

#[test]
fn an_mdns_response_names_the_host_with_its_services_and_model() {
    let message = printer_response();
    let found = hosts(&heard(&over_udp(MDNS_MAC, (PRINTER, MDNS_GROUP), (5353, 5353), &message)));
    let printer = |address: IpAddr, mac: Option<Mac>| Announcement {
        mac,
        services: vec!["ipp".to_owned()],
        model: Some("Example LaserJet 400".to_owned()),
        ..named(ListenProtocol::Mdns, address, "office-printer")
    };
    assert_eq!(
        found,
        vec![printer(PRINTER.into(), Some(Mac(PRINTER_MAC))), printer("fe80::44".parse().unwrap(), None)]
    );
}

#[test]
fn a_frame_sent_to_one_host_pairs_no_hardware_address() {
    let found = hosts(&heard(&over_udp(OWN_MAC, (PRINTER, OWN), (5353, 5353), &printer_response())));
    assert!(found.iter().all(|announcement| announcement.mac.is_none()));
}

#[test]
fn an_mdns_probe_names_the_address_it_claims() {
    let authority = [build::record("laptop.local", TYPE_A, &LAPTOP.octets())];
    let message = build::message(0, Some("laptop.local"), [&[], &authority, &[]]);
    let found = hosts(&heard(&over_udp(MDNS_MAC, (LAPTOP, MDNS_GROUP), (5353, 5353), &message)));
    assert_eq!(
        found,
        vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..named(ListenProtocol::Mdns, LAPTOP, "laptop") }]
    );
}

#[test]
fn the_answers_an_mdns_query_already_knows_name_nobody() {
    let known = [build::record("_airplay._tcp.local", TYPE_PTR, &build::name("Living Room._airplay._tcp.local"))];
    let message = build::message(0, Some("_airplay._tcp.local"), [&known, &[], &[]]);
    let found = hosts(&heard(&over_udp(MDNS_MAC, (LAPTOP, MDNS_GROUP), (5353, 5353), &message)));
    assert_eq!(found, vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Mdns, LAPTOP) }]);
}

#[test]
fn a_reverse_pointer_names_the_address_it_spells() {
    let answers = [build::record("52.2.0.192.in-addr.arpa", TYPE_PTR, &build::name("laptop.local"))];
    let message = build::message(RESPONSE, None, [&answers, &[], &[]]);
    let found = hosts(&heard(&over_udp(OWN_MAC, (LAPTOP, OWN), (5353, 5353), &message)));
    assert_eq!(found, vec![named(ListenProtocol::Mdns, LAPTOP, "laptop")]);
    let nibbles = "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.e.f.ip6.arpa";
    assert_eq!(reversed(nibbles), Some("fe80::1".parse().unwrap()));
    assert_eq!(reversed("1.2.3.in-addr.arpa"), None);
    assert_eq!(reversed("+.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.e.f.ip6.arpa"), None);
}

#[test]
fn services_a_proxy_answers_for_go_to_the_host_they_point_at() {
    let instance = "Studio Mac._smb._tcp.local";
    let answers = [
        build::record("_smb._tcp.local", TYPE_PTR, &build::name(instance)),
        build::record(instance, TYPE_SRV, &build::service("studio-mac.local")),
        build::record("studio-mac.local", TYPE_A, &LAPTOP.octets()),
    ];
    let message = build::message(RESPONSE, None, [&answers, &[], &[]]);
    let found = hosts(&heard(&over_udp(MDNS_MAC, (ROUTER, MDNS_GROUP), (5353, 5353), &message)));
    let studio = Announcement { services: vec!["smb".to_owned()], ..named(ListenProtocol::Mdns, LAPTOP, "studio-mac") };
    let proxy = Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Mdns, ROUTER) };
    assert_eq!(found, vec![studio, proxy]);
}

#[test]
fn the_service_catalog_and_device_info_are_no_services() {
    assert_eq!(service_type("_services._dns-sd._udp.local"), None);
    assert_eq!(service_type("Laptop._device-info._tcp.local"), None);
    assert_eq!(service_type("_printer._sub._http._tcp.local"), Some("http".to_owned()));
    assert_eq!(service_type("laptop.local"), None);
}

#[test]
fn an_ssdp_notice_gives_the_device_type_and_server() {
    let notice = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
                  NTS: ssdp:alive\r\nSERVER: ExampleOS/1.0 UPnP/1.0 Speaker/2.1\r\nUSN: uuid:0000::upnp:rootdevice\r\n\r\n";
    let found = hosts(&heard(&over_udp(SSDP_MAC, (SPEAKER, SSDP_GROUP), (1900, 1900), notice.as_bytes())));
    let speaker = Announcement {
        mac: Some(Mac(PRINTER_MAC)),
        model: Some("ExampleOS/1.0 UPnP/1.0 Speaker/2.1".to_owned()),
        kinds: vec!["MediaRenderer".to_owned()],
        ..host(ListenProtocol::Ssdp, SPEAKER)
    };
    assert_eq!(found, vec![speaker]);
    let response = "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:service:AVTransport:1\r\n\r\n";
    let found = hosts(&heard(&over_udp(OWN_MAC, (SPEAKER, OWN), (1900, 50_000), response.as_bytes())));
    assert_eq!(found[0].services, vec!["AVTransport".to_owned()]);
}

#[test]
fn an_ssdp_search_only_says_its_sender_is_there() {
    let search = "M-SEARCH * HTTP/1.1\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nMX: 2\r\n\r\n";
    let found = hosts(&heard(&over_udp(SSDP_MAC, (LAPTOP, SSDP_GROUP), (50_000, 1900), search.as_bytes())));
    assert_eq!(found, vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Ssdp, LAPTOP) }]);
}

fn netbios_encoded(name: &str, suffix: u8) -> String {
    let mut padded = format!("{name:<15}").into_bytes();
    padded.push(suffix);
    padded.iter().flat_map(|byte| [char::from(b'A' + (byte >> 4)), char::from(b'A' + (byte & 0x0f))]).collect()
}

fn netbios_entry(flags: u16, address: Ipv4Addr) -> Vec<u8> {
    [flags.to_be_bytes().to_vec(), address.octets().to_vec()].concat()
}

#[test]
fn a_netbios_registration_names_the_machine() {
    let encoded = netbios_encoded("DESKTOP-7K2Q", 0x00);
    let additional = [build::record(&encoded, 0x20, &netbios_entry(0, LAPTOP))];
    let message = build::message(0x2910, Some(&encoded), [&[], &[], &additional]);
    let found = hosts(&heard(&over_udp(BROADCAST_MAC, (LAPTOP, Ipv4Addr::BROADCAST), (137, 137), &message)));
    let desktop =
        Announcement { mac: Some(Mac(PRINTER_MAC)), ..named(ListenProtocol::Netbios, LAPTOP, "DESKTOP-7K2Q") };
    assert_eq!(found, vec![desktop]);
}

#[test]
fn netbios_group_names_and_other_suffixes_name_nobody() {
    let workgroup = netbios_encoded("WORKGROUP", 0x00);
    let additional = [build::record(&workgroup, 0x20, &netbios_entry(0x8000, LAPTOP))];
    let message = build::message(0x2910, Some(&workgroup), [&[], &[], &additional]);
    let found = hosts(&heard(&over_udp(BROADCAST_MAC, (LAPTOP, Ipv4Addr::BROADCAST), (137, 137), &message)));
    assert_eq!(found, vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Netbios, LAPTOP) }]);
    assert_eq!(netbios_name(&netbios_encoded("DESKTOP-7K2Q", 0x1d)), None);
    assert_eq!(netbios_name("not-encoded"), None);
}

#[test]
fn a_positive_netbios_answer_names_each_address() {
    let encoded = netbios_encoded("FILESERVER", 0x20);
    let entries = [netbios_entry(0, PRINTER), netbios_entry(0, LAPTOP)].concat();
    let answers = [build::record(&encoded, 0x20, &entries)];
    let message = build::message(0x8500, None, [&answers, &[], &[]]);
    let found = hosts(&heard(&over_udp(OWN_MAC, (PRINTER, OWN), (137, 137), &message)));
    let named_server = |address| named(ListenProtocol::Netbios, address, "FILESERVER");
    assert_eq!(found, vec![named_server(PRINTER), named_server(LAPTOP)]);
}

#[test]
fn an_llmnr_answer_names_the_host_and_a_query_only_shows_the_asker() {
    let answers = [build::record("laptop", TYPE_A, &LAPTOP.octets())];
    let answer = build::message(0x8000, Some("laptop"), [&answers, &[], &[]]);
    let found = hosts(&heard(&over_udp(OWN_MAC, (LAPTOP, OWN), (5355, 50_000), &answer)));
    assert_eq!(found, vec![named(ListenProtocol::Llmnr, LAPTOP, "laptop")]);
    let query = build::message(0, Some("printer"), [&[], &[], &[]]);
    let found = hosts(&heard(&over_udp(MDNS_MAC, (LAPTOP, LLMNR_GROUP), (50_000, 5355), &query)));
    assert_eq!(found, vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Llmnr, LAPTOP) }]);
}

fn bootp(op: u8, your: Ipv4Addr, options: &[(u8, &[u8])]) -> Vec<u8> {
    let mut message = vec![op, 1, 6, 0, 0x12, 0x34, 0x56, 0x78, 0, 0, 0x80, 0];
    message.extend([0; 4]);
    message.extend(your.octets());
    message.extend([0; 8]);
    message.extend(PRINTER_MAC);
    message.resize(236, 0);
    message.extend([0x63, 0x82, 0x53, 0x63]);
    for (code, data) in options {
        message.push(*code);
        message.push(u8::try_from(data.len()).unwrap());
        message.extend(*data);
    }
    message.push(255);
    message
}

#[test]
fn a_dhcp_request_names_the_client_with_its_address_and_vendor_class() {
    let options: [(u8, &[u8]); 4] = [(53, &[3]), (50, &LAPTOP.octets()), (12, b"laptop"), (60, b"MSFT 5.0")];
    let request = bootp(1, Ipv4Addr::UNSPECIFIED, &options);
    let found =
        hosts(&heard(&over_udp(BROADCAST_MAC, (Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST), (68, 67), &request)));
    let laptop = Announcement {
        mac: Some(Mac(PRINTER_MAC)),
        client: Some("MSFT 5.0".to_owned()),
        ..named(ListenProtocol::Dhcp, LAPTOP, "laptop")
    };
    assert_eq!(found, vec![laptop]);
}

#[test]
fn a_dhcp_discover_gives_the_name_from_the_fqdn_and_no_address() {
    let fqdn = [&[0x04, 0, 0][..], b"\x06laptop\x07example\x00"].concat();
    let options: [(u8, &[u8]); 2] = [(53, &[1]), (81, &fqdn)];
    let discover = bootp(1, Ipv4Addr::UNSPECIFIED, &options);
    let found =
        hosts(&heard(&over_udp(BROADCAST_MAC, (Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST), (68, 67), &discover)));
    let laptop = Announcement {
        mac: Some(Mac(PRINTER_MAC)),
        name: Some("laptop".to_owned()),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    assert_eq!(found, vec![laptop]);
    assert_eq!(fqdn_name(&[&[0, 0, 0][..], b"laptop.example"].concat()), Some("laptop".to_owned()));
}

#[test]
fn a_dhcp_acknowledgment_gives_the_address_it_hands_out() {
    let ack = bootp(2, LAPTOP, &[(53, &[5])]);
    let frame = frame(BROADCAST_MAC, ROUTER_MAC, 0x0800, &ipv4(ROUTER, Ipv4Addr::BROADCAST, &udp((67, 68), &ack)));
    let found = hosts(&heard(&frame));
    let laptop = Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Dhcp, LAPTOP) };
    let server = Announcement { mac: Some(Mac(ROUTER_MAC)), ..host(ListenProtocol::Dhcp, ROUTER) };
    assert_eq!(found, vec![laptop, server]);
    assert!(
        hosts(&heard(&over_udp(
            BROADCAST_MAC,
            (ROUTER, Ipv4Addr::BROADCAST),
            (67, 68),
            &bootp(2, LAPTOP, &[(53, &[2])])
        )))
        .iter()
        .all(|found| found.address == Some(ROUTER.into()))
    );
}

fn icmpv6(kind: u8, body: &[u8]) -> Vec<u8> {
    [&[kind, 0, 0, 0][..], body].concat()
}

fn link_option(kind: u8, mac: [u8; 6]) -> Vec<u8> {
    [&[kind, 1][..], &mac].concat()
}

#[test]
fn a_neighbor_advertisement_gives_the_target_and_router_flag() {
    let router: Ipv6Addr = "fe80::1".parse().unwrap();
    let body = [&[0xa0, 0, 0, 0][..], &router.octets(), &link_option(2, ROUTER_MAC)].concat();
    let packet = ipv6(router, "ff02::1".parse().unwrap(), 58, &icmpv6(136, &body));
    let found = hosts(&heard(&frame(ALL_NODES_MAC, ROUTER_MAC, 0x86dd, &packet)));
    let advertised = Announcement { mac: Some(Mac(ROUTER_MAC)), router: true, ..host(ListenProtocol::Ndp, router) };
    assert_eq!(found, vec![advertised]);
}

#[test]
fn duplicate_address_detection_gives_the_address_being_claimed() {
    let claimed: Ipv6Addr = "2001:db8::44".parse().unwrap();
    let body = [&[0, 0, 0, 0][..], &claimed.octets()].concat();
    let packet = ipv6(Ipv6Addr::UNSPECIFIED, "ff02::1:ff00:44".parse().unwrap(), 58, &icmpv6(135, &body));
    let found = hosts(&heard(&frame([0x33, 0x33, 0xff, 0, 0, 0x44], PRINTER_MAC, 0x86dd, &packet)));
    assert_eq!(found, vec![Announcement { mac: Some(Mac(PRINTER_MAC)), ..host(ListenProtocol::Ndp, claimed) }]);
}

#[test]
fn a_router_advertisement_marks_its_sender_a_router() {
    let router: Ipv6Addr = "fe80::1".parse().unwrap();
    let body = [&[64, 0, 0x07, 0x08, 0, 0, 0, 0, 0, 0, 0, 0][..], &link_option(1, ROUTER_MAC)].concat();
    let packet = ipv6(router, "ff02::1".parse().unwrap(), 58, &icmpv6(134, &body));
    let found = hosts(&heard(&frame(ALL_NODES_MAC, OWN_MAC, 0x86dd, &packet)));
    assert_eq!(
        found,
        vec![Announcement { mac: Some(Mac(ROUTER_MAC)), router: true, ..host(ListenProtocol::Ndp, router) }]
    );
}

#[test]
fn other_icmpv6_messages_give_nothing() {
    let packet = ipv6("fe80::44".parse().unwrap(), "ff02::16".parse().unwrap(), 58, &icmpv6(143, &[0; 4]));
    assert!(heard(&frame([0x33, 0x33, 0, 0, 0, 0x16], PRINTER_MAC, 0x86dd, &packet)).is_empty());
}

fn lldp_unit(kind: u16, value: &[u8]) -> Vec<u8> {
    let head = (kind << 9) | u16::try_from(value.len()).unwrap();
    [&head.to_be_bytes()[..], value].concat()
}

fn lldp_frame(port: &[u8]) -> Vec<u8> {
    let management = [5, 1, 192, 0, 2, 2, 2, 0, 0, 0, 7, 0];
    let units = [
        lldp_unit(1, &[&[4][..], &ROUTER_MAC].concat()),
        lldp_unit(2, port),
        lldp_unit(3, &[0, 120]),
        lldp_unit(4, b"GigabitEthernet1/0/12"),
        lldp_unit(5, b"core-switch"),
        lldp_unit(6, b"Example Switch OS 15.2\nTechnical Support: example.test"),
        lldp_unit(8, &management),
        lldp_unit(0, &[]),
    ]
    .concat();
    frame(LLDP_MAC, ROUTER_MAC, 0x88cc, &units)
}

#[test]
fn lldp_names_the_switch_and_port() {
    let found = heard(&lldp_frame(&[&[5][..], b"Gi1/0/12"].concat()));
    let switch = SwitchPort {
        protocol: ListenProtocol::Lldp,
        name: Some("core-switch".to_owned()),
        port: Some("Gi1/0/12".to_owned()),
        model: Some("Example Switch OS 15.2".to_owned()),
        address: Some(Ipv4Addr::new(192, 0, 2, 2).into()),
    };
    let host = Announcement {
        model: Some("Example Switch OS 15.2".to_owned()),
        ..named(ListenProtocol::Lldp, Ipv4Addr::new(192, 0, 2, 2), "core-switch")
    };
    assert_eq!(found, vec![Heard::Switch(switch), Heard::Host(host)]);
}

#[test]
fn an_lldp_port_given_as_hardware_address_shows_its_description() {
    let found = heard(&lldp_frame(&[&[3][..], &ROUTER_MAC].concat()));
    let Some(Heard::Switch(switch)) = found.first() else { panic!("no switch in {found:?}") };
    assert_eq!(switch.port.as_deref(), Some("GigabitEthernet1/0/12"));
}

fn cdp_unit(kind: u16, value: &[u8]) -> Vec<u8> {
    [&kind.to_be_bytes()[..], &u16::try_from(value.len() + 4).unwrap().to_be_bytes(), value].concat()
}

#[test]
fn cdp_names_the_switch_and_port() {
    let ipv6_entry =
        [&[2, 8][..], &[0xaa, 0xaa, 0x03, 0, 0, 0, 0x86, 0xdd], &[0, 16], &[0xfe, 0x80], &[0; 14]].concat();
    let ipv4_entry = [&[1, 1, 0xcc][..], &[0, 4], &[192, 0, 2, 3]].concat();
    let addresses = [&2u32.to_be_bytes()[..], &ipv6_entry, &ipv4_entry].concat();
    let units = [
        cdp_unit(1, b"edge-switch"),
        cdp_unit(2, &addresses),
        cdp_unit(3, b"FastEthernet0/7"),
        cdp_unit(6, b"Example 2960"),
    ]
    .concat();
    let body = [&CDP_SNAP[..], &[2, 180, 0, 0], &units].concat();
    let found = heard(&frame(CDP_ADDRESS.0, ROUTER_MAC, u16::try_from(body.len()).unwrap(), &body));
    let switch = SwitchPort {
        protocol: ListenProtocol::Cdp,
        name: Some("edge-switch".to_owned()),
        port: Some("FastEthernet0/7".to_owned()),
        model: Some("Example 2960".to_owned()),
        address: Some(Ipv4Addr::new(192, 0, 2, 3).into()),
    };
    assert_eq!(found.first(), Some(&Heard::Switch(switch)));
    assert_eq!(found.len(), 2);
}

#[test]
fn other_ethernet_frames_give_nothing() {
    let body = [&CDP_SNAP[..], &[2, 180, 0, 0]].concat();
    assert!(heard(&frame(BROADCAST_MAC, ROUTER_MAC, u16::try_from(body.len()).unwrap(), &body)).is_empty());
    assert!(heard(&frame(BROADCAST_MAC, ROUTER_MAC, 0x88b5, &[0; 46])).is_empty());
    let tcp = [&[0, 22, 0xc3, 0x50][..], &[0; 16]].concat();
    let mut packet = ipv4(PRINTER, OWN, &tcp);
    packet[9] = 6;
    assert!(heard(&frame(OWN_MAC, PRINTER_MAC, 0x0800, &packet)).is_empty());
}

#[test]
fn announced_text_loses_control_and_invisible_characters_and_stays_short() {
    assert_eq!(clean("\u{202e}evil\u{7} name\nsecond line".as_bytes()), Some("evil name".to_owned()));
    assert_eq!(clean(b"\n\n  printer \0\0"), Some("printer".to_owned()));
    assert_eq!(clean(&[b'a'; 200]).map(|text| text.chars().count()), Some(LONGEST_TEXT));
    assert_eq!(clean(b" \x1b "), None);
}

#[test]
fn a_name_that_points_at_itself_ends_the_message() {
    let mut message = build::message(RESPONSE, None, [&[], &[], &[]]);
    message[7] = 1;
    message.extend([0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 120, 0, 4, 192, 0, 2, 9]);
    let read = dns::read(&message).unwrap();
    assert!(read.answers.is_empty());
}

#[test]
fn a_vlan_tag_is_skipped() {
    let mut arp = vec![0, 1, 8, 0, 6, 4, 0, 2];
    arp.extend(PRINTER_MAC);
    arp.extend(PRINTER.octets());
    arp.extend(OWN_MAC);
    arp.extend(OWN.octets());
    let mut tagged = frame(BROADCAST_MAC, PRINTER_MAC, 0x8100, &[0x00, 0x0a]);
    tagged.extend(0x0806_u16.to_be_bytes());
    tagged.extend(arp);
    assert_eq!(hosts(&heard(&tagged)).first().and_then(|found| found.address), Some(PRINTER.into()));
}
