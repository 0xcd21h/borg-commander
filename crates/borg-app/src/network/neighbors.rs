use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use borg_core::Neighborhood;

#[cfg(any(target_os = "macos", test))]
mod darwin;
#[cfg(any(target_os = "linux", test))]
mod linux;
mod resolv;

const RESOLV_CONF: &str = "/etc/resolv.conf";

pub(super) fn read() -> Neighborhood {
    let read_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let neighborhood =
        collect().unwrap_or_else(|error| Neighborhood { error: Some(format!("{error:#}")), ..Neighborhood::default() });
    Neighborhood { read_at, ..neighborhood }
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
    Ok(Neighborhood { neighbors, gateways, dns, ..Neighborhood::default() })
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
    Ok(Neighborhood { neighbors, gateways, dns, ..Neighborhood::default() })
}
