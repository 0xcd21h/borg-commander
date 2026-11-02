use super::*;
use borg_core::{Exposure, PortRow, Rate, Transport};
use ratatui::layout::Rect;

fn fields(page: NetworkPage, width: u16) -> Vec<Field> {
    columns(page, width).iter().map(|column| column.field).collect()
}

#[test]
fn narrow_panes_leave_out_who_can_reach_a_port() {
    assert!(fields(NetworkPage::Ports, REACH_FROM).contains(&Field::Reach));
    assert!(!fields(NetworkPage::Ports, REACH_FROM - 1).contains(&Field::Reach));
}

#[test]
fn wide_panes_show_a_connections_protocol_and_state() {
    assert_eq!(
        fields(NetworkPage::Connections, WIDE_FROM),
        [Field::Process, Field::Remote, Field::Protocol, Field::State, Field::Down, Field::Up]
    );
    assert_eq!(
        fields(NetworkPage::Connections, WIDE_FROM - 1),
        [Field::Process, Field::Remote, Field::Down, Field::Up]
    );
}

#[test]
fn the_open_column_takes_what_the_others_leave() {
    let area = Rect::new(4, 0, 70, 1);
    let columns = columns(NetworkPage::Ports, area.width);
    let cells = cells(&columns, area);
    assert_eq!(cells.first().map(|cell| cell.x), Some(area.x));
    assert_eq!(cells.last().map(|cell| cell.right()), Some(area.right()));
    assert!(cells.windows(2).all(|pair| pair[0].right() + 1 == pair[1].x));
}

#[test]
fn a_port_on_several_addresses_names_the_first_and_counts_the_rest() {
    let row = |addresses: &[&str], exposure| PortRow {
        transport: Transport::Tcp,
        port: 8080,
        pid: None,
        addresses: addresses.iter().map(|address| address.parse().unwrap()).collect(),
        exposure,
        connections: 0,
        rate: Rate::default(),
    };
    assert_eq!(list::reach(&row(&["192.0.2.10", "198.51.100.3"], Exposure::Addresses)), "192.0.2.10 +1");
    assert_eq!(list::reach(&row(&["192.0.2.10"], Exposure::Addresses)), "192.0.2.10");
    assert_eq!(list::reach(&row(&["127.0.0.1", "::1"], Exposure::ThisMachine)), tr!(network_this_machine));
}

#[test]
fn uncounted_bytes_show_as_unknown() {
    assert_eq!(rate(None), UNKNOWN);
    assert_eq!(rate(Some(0)), format::size(0));
}

#[test]
fn an_interface_shows_its_ipv4_address_first() {
    let addresses = [("2001:db8::20".parse().unwrap(), 64), ("192.0.2.20".parse().unwrap(), 24)];
    assert_eq!(first_address(&addresses), Some("192.0.2.20".parse().unwrap()));
    assert_eq!(first_address(&addresses[..1]), Some("2001:db8::20".parse().unwrap()));
}
