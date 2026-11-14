use std::net::{IpAddr, Ipv4Addr};

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) const RESOLVED_CONF: &str = "/run/systemd/resolve/resolv.conf";
const NAMESERVER: &str = "nameserver";
const COMMENTS: [char; 2] = ['#', ';'];
const SCOPE: char = '%';
const RESOLVED_STUB: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 53);

pub(super) fn nameservers(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let line = line.split(COMMENTS).next()?;
            let mut words = line.split_whitespace();
            (words.next()? == NAMESERVER).then_some(())?;
            let address = words.next()?;
            address.split(SCOPE).next()?.parse().ok()
        })
        .collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn only_stub(servers: &[IpAddr]) -> bool {
    !servers.is_empty() && servers.iter().all(|server| *server == IpAddr::V4(RESOLVED_STUB))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nameserver_lines_give_the_servers_in_order_without_scopes_or_comments() {
        let text = "# made by hand\nsearch example.lan\nnameserver 192.0.2.1\nnameserver 2001:db8::53 # second\n\
                    ; nameserver 198.51.100.9\nnameserver fe80::1%en0\noptions edns0\nnameserver not-an-address\n";
        let expected: Vec<IpAddr> =
            ["192.0.2.1", "2001:db8::53", "fe80::1"].iter().map(|address| address.parse().unwrap()).collect();
        assert_eq!(nameservers(text), expected);
        assert_eq!(nameservers(""), Vec::<IpAddr>::new());
    }

    #[test]
    fn the_resolved_stub_alone_points_at_the_real_servers() {
        assert!(only_stub(&nameservers("nameserver 127.0.0.53\noptions edns0 trust-ad\n")));
        assert!(!only_stub(&nameservers("nameserver 127.0.0.53\nnameserver 192.0.2.1\n")));
        assert!(!only_stub(&[]));
    }
}
