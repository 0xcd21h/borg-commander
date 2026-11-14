use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use borg_core::{Gateway, Mac, Neighbor, NeighborState};

#[cfg(target_os = "macos")]
pub(super) const ARP: &str = "/usr/sbin/arp";
#[cfg(target_os = "macos")]
pub(super) const ARP_ARGUMENTS: [&str; 1] = ["-anl"];
#[cfg(target_os = "macos")]
pub(super) const NDP: &str = "/usr/sbin/ndp";
#[cfg(target_os = "macos")]
pub(super) const NDP_ARGUMENTS: [&str; 1] = ["-an"];
#[cfg(target_os = "macos")]
pub(super) const NETSTAT: &str = "/usr/sbin/netstat";
#[cfg(target_os = "macos")]
pub(super) const NETSTAT_ARGUMENTS: [&str; 1] = ["-rn"];

const INCOMPLETE: &str = "(incomplete)";
const EXPIRED: &str = "expired";
const PERMANENT: &str = "permanent";
const SCOPE: char = '%';
const ROUTER_FLAG: char = 'R';
const LINK: usize = 1;
const ARP_TIMERS: usize = 2;
const ARP_INTERFACE: usize = 4;
const NDP_INTERFACE: usize = 2;
const NDP_EXPIRE: usize = 3;
const NDP_STATE: usize = 4;
const NDP_STATES: [(&str, NeighborState); 5] = [
    ("R", NeighborState::Reachable),
    ("S", NeighborState::Stale),
    ("D", NeighborState::Delay),
    ("P", NeighborState::Probe),
    ("I", NeighborState::Incomplete),
];
const DEFAULT_ROUTE: &str = "default";
const HEADER: &str = "Destination";
const FLAGS_COLUMN: &str = "Flags";
const NETIF_COLUMN: &str = "Netif";
const GATEWAY_COLUMN: usize = 1;
const GATEWAY_FLAG: char = 'G';
const SCOPED_FLAG: char = 'I';

fn unscoped(address: &str) -> &str {
    address.split(SCOPE).next().unwrap_or(address)
}

fn running(timer: &str) -> bool {
    timer.starts_with(|c: char| c.is_ascii_digit())
}

pub(super) fn arp(output: &str) -> Vec<Neighbor> {
    output
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let address: Ipv4Addr = words.first()?.parse().ok()?;
            let link = *words.get(LINK)?;
            let timers = words.get(ARP_TIMERS..ARP_INTERFACE)?;
            let state = if link == INCOMPLETE {
                Some(NeighborState::Incomplete)
            } else if timers.iter().any(|timer| running(timer)) {
                Some(NeighborState::Reachable)
            } else if timers.contains(&EXPIRED) {
                Some(NeighborState::Stale)
            } else {
                None
            };
            Some(Neighbor {
                address: IpAddr::V4(address),
                mac: Mac::parse(link),
                interface: (*words.get(ARP_INTERFACE)?).to_owned(),
                state,
                router: false,
            })
        })
        .collect()
}

pub(super) fn ndp(output: &str) -> Vec<Neighbor> {
    output
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let address: Ipv6Addr = unscoped(words.first()?).parse().ok()?;
            let letter = *words.get(NDP_STATE)?;
            let state = if words[NDP_EXPIRE] == PERMANENT {
                Some(NeighborState::Permanent)
            } else {
                NDP_STATES.iter().find(|(known, _)| *known == letter).map(|(_, state)| *state)
            };
            let flags = &words[NDP_STATE + 1..];
            let router =
                flags.iter().any(|flag| !flag.chars().all(|c| c.is_ascii_digit()) && flag.contains(ROUTER_FLAG));
            Some(Neighbor {
                address: IpAddr::V6(address),
                mac: Mac::parse(words[LINK]),
                interface: words[NDP_INTERFACE].to_owned(),
                state,
                router,
            })
        })
        .collect()
}

pub(super) fn gateways(output: &str) -> Vec<Gateway> {
    let mut columns: Option<(usize, usize)> = None;
    let mut found: Vec<(bool, Gateway)> = Vec::new();
    for line in output.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first() {
            Some(&HEADER) => {
                let at = |name: &str| words.iter().position(|word| *word == name);
                columns = at(FLAGS_COLUMN).zip(at(NETIF_COLUMN));
            }
            Some(&DEFAULT_ROUTE) => {
                let Some((flags, netif)) = columns else { continue };
                let (Some(gateway), Some(flags), Some(interface)) =
                    (words.get(GATEWAY_COLUMN), words.get(flags), words.get(netif))
                else {
                    continue;
                };
                let Ok(address) = unscoped(gateway).parse::<IpAddr>() else { continue };
                if flags.contains(GATEWAY_FLAG) {
                    let gateway = Gateway { address, interface: (*interface).to_owned() };
                    found.push((flags.contains(SCOPED_FLAG), gateway));
                }
            }
            _ => {}
        }
    }
    found.sort_by_key(|(scoped, _)| *scoped);
    found.into_iter().map(|(_, gateway)| gateway).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARP_OUTPUT: &str = "\
Neighbor                Linklayer Address Expire(O) Expire(I)          Netif Refs Prbs
192.0.2.1               3c:22:fb:1:2:3    19m42s    19m42s               en0    1
192.0.2.20              a4:83:e7:aa:bb:cc expired   expired              en0    0
192.0.2.44              da:a1:19:12:34:56 (none)    (none)               en0    0
192.0.2.77              (incomplete)      expired   expired              en0    0    3
192.0.2.255             ff:ff:ff:ff:ff:ff (none)    (none)               en0    0
";

    const NDP_OUTPUT: &str = "\
Neighbor                             Linklayer Address  Netif Expire    St Flgs Prbs
2001:db8::20                         a4:83:e7:aa:bb:cc    en0 23h59m58s S
fe80::1%en0                          3c:22:fb:1:2:3       en0 8s        R  R
fe80::a6:83ff:feaa:bbcc%en0          (incomplete)         en0 expired   I       2
fe80::99%en0                         0:5:1a:0:0:9         en0 permanent R
";

    const NETSTAT_OUTPUT: &str = "\
Routing tables

Internet:
Destination        Gateway            Flags               Netif Expire
default            198.51.100.1       UGScIg                en7
default            192.0.2.1          UGScg                 en0
127                127.0.0.1          UCS                   lo0
192.0.2            link#14            UCS                   en0      !

Internet6:
Destination                             Gateway                                 Flags               Netif Expire
default                                 fe80::1%en0                             UGcg                  en0
default                                 fe80::%utun0                            UGcIg               utun0
::1                                     ::1                                     UHL                   lo0
";

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn arp_rows_read_as_neighbors_with_the_state_their_timers_tell() {
        let neighbors = arp(ARP_OUTPUT);
        let found: Vec<(IpAddr, Option<String>, Option<NeighborState>)> = neighbors
            .iter()
            .map(|neighbor| (neighbor.address, neighbor.mac.map(|mac| mac.to_string()), neighbor.state))
            .collect();
        assert_eq!(
            found,
            [
                (address("192.0.2.1"), Some("3c:22:fb:01:02:03".into()), Some(NeighborState::Reachable)),
                (address("192.0.2.20"), Some("a4:83:e7:aa:bb:cc".into()), Some(NeighborState::Stale)),
                (address("192.0.2.44"), Some("da:a1:19:12:34:56".into()), None),
                (address("192.0.2.77"), None, Some(NeighborState::Incomplete)),
                (address("192.0.2.255"), Some("ff:ff:ff:ff:ff:ff".into()), None),
            ]
        );
        assert!(neighbors.iter().all(|neighbor| neighbor.interface == "en0" && !neighbor.router));
    }

    #[test]
    fn ndp_rows_keep_the_state_letter_and_the_router_flag_without_the_scope() {
        let neighbors = ndp(NDP_OUTPUT);
        let found: Vec<(IpAddr, Option<NeighborState>, bool, bool)> = neighbors
            .iter()
            .map(|neighbor| (neighbor.address, neighbor.state, neighbor.router, neighbor.mac.is_some()))
            .collect();
        assert_eq!(
            found,
            [
                (address("2001:db8::20"), Some(NeighborState::Stale), false, true),
                (address("fe80::1"), Some(NeighborState::Reachable), true, true),
                (address("fe80::a6:83ff:feaa:bbcc"), Some(NeighborState::Incomplete), false, false),
                (address("fe80::99"), Some(NeighborState::Permanent), false, true),
            ]
        );
    }

    #[test]
    fn default_routes_give_the_gateways_with_the_unscoped_ones_first() {
        let found: Vec<(IpAddr, String)> =
            gateways(NETSTAT_OUTPUT).into_iter().map(|gateway| (gateway.address, gateway.interface)).collect();
        assert_eq!(
            found,
            [
                (address("192.0.2.1"), "en0".to_owned()),
                (address("fe80::1"), "en0".to_owned()),
                (address("198.51.100.1"), "en7".to_owned()),
                (address("fe80::"), "utun0".to_owned()),
            ]
        );
    }

    #[test]
    fn older_route_tables_with_more_columns_still_find_the_interface() {
        let older = "Destination        Gateway            Flags        Refs      Use   Netif Expire\n\
                     default            192.0.2.1          UGSc           12        0     en0\n";
        let found: Vec<String> = gateways(older).into_iter().map(|gateway| gateway.interface).collect();
        assert_eq!(found, ["en0"]);
    }

    #[test]
    fn lines_that_are_not_rows_are_left_out() {
        for nonsense in
            ["", "\n\n", "Neighbor Linklayer", "192.0.2.1", "fe80::1%en0 3c:22:fb:1:2:3", "default 192.0.2.1"]
        {
            assert!(arp(nonsense).is_empty(), "{nonsense:?}");
            assert!(ndp(nonsense).is_empty(), "{nonsense:?}");
            assert!(gateways(nonsense).is_empty(), "{nonsense:?}");
        }
    }
}
