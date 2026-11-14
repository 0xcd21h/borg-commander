use std::{
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    time::Duration,
};

use anyhow::Result;
use borg_core::Neighborhood;

#[cfg(any(target_os = "macos", test))]
mod darwin;
#[cfg(any(target_os = "linux", test))]
mod linux;
mod resolv;

const RESOLV_CONF: &str = "/etc/resolv.conf";
const DISCARD_PORT: u16 = 9;
const SWEEP_TIME: Duration = Duration::from_secs(1);

pub(super) fn read() -> Neighborhood {
    collect().unwrap_or_else(|error| Neighborhood { error: Some(format!("{error:#}")), ..Neighborhood::default() })
}

#[cfg(target_os = "macos")]
fn collect() -> Result<Neighborhood> {
    let run = |program: &str, arguments: &[&str]| -> Result<String> {
        let output = std::process::Command::new(program).args(arguments).output()?;
        if !output.status.success() {
            anyhow::bail!(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let mut neighbors = darwin::arp(&run(darwin::ARP, &darwin::ARP_ARGUMENTS)?);
    neighbors.extend(darwin::ndp(&run(darwin::NDP, &darwin::NDP_ARGUMENTS)?));
    let gateways = darwin::gateways(&run(darwin::NETSTAT, &darwin::NETSTAT_ARGUMENTS)?);
    let dns = resolv::nameservers(&std::fs::read_to_string(RESOLV_CONF).unwrap_or_default());
    Ok(Neighborhood { neighbors, gateways, dns, error: None })
}

#[cfg(target_os = "linux")]
fn collect() -> Result<Neighborhood> {
    use std::fs::read_to_string;
    let names = linux::interface_names()?;
    let neighbors = match linux::dump(&names) {
        Ok(neighbors) => neighbors,
        Err(_) => linux::proc_arp(&read_to_string(linux::PROC_ARP)?),
    };
    let mut gateways = linux::routes_v4(&read_to_string(linux::PROC_ROUTE)?);
    gateways.extend(linux::routes_v6(&read_to_string(linux::PROC_ROUTE_V6).unwrap_or_default()));
    let mut dns = resolv::nameservers(&read_to_string(RESOLV_CONF).unwrap_or_default());
    if resolv::only_stub(&dns) {
        dns = resolv::nameservers(&read_to_string(resolv::RESOLVED_CONF).unwrap_or_default());
    }
    Ok(Neighborhood { neighbors, gateways, dns, error: None })
}

pub(super) fn sweep(addresses: &[Ipv4Addr]) -> Result<usize> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    let pause = SWEEP_TIME / u32::try_from(addresses.len().max(1)).unwrap_or(u32::MAX);
    let mut sent = 0;
    let mut failure = None;
    for address in addresses {
        match socket.send_to(&[], SocketAddrV4::new(*address, DISCARD_PORT)) {
            Ok(_) => sent += 1,
            Err(error) => failure = Some(error),
        }
        std::thread::sleep(pause);
    }
    match failure {
        Some(error) if sent == 0 => Err(error.into()),
        _ => Ok(sent),
    }
}
