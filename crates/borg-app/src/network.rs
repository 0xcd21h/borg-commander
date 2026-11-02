//! Reads the network screen's samples: each socket with the process that holds it, every
//! interface with its counters, and the names of the hosts connections go to.

use anyhow::{Result, anyhow};
use borg_core::{Interface, InterfaceKind, Msg, NetworkSample, Socket};
use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use sysinfo::{InterfaceOperationalState, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

mod kinds;
#[cfg(any(target_os = "macos", test))]
mod netstat;
#[cfg(any(target_os = "linux", test))]
mod proc_net;

/// How long a host's name may take before the screen keeps showing its address.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(2);

/// Two samples a second apart and the seconds between them, for an agent to read rates as
/// the screen shows them.
pub(crate) fn measure() -> Result<(NetworkSample, NetworkSample, f64)> {
    const SETTLE: Duration = Duration::from_secs(1);
    let mut sampler = Sampler::default();
    let before = sampler.sample()?;
    let started = Instant::now();
    std::thread::sleep(SETTLE);
    let after = sampler.sample()?;
    Ok((before, after, started.elapsed().as_secs_f64()))
}

pub struct Network {
    messages: UnboundedSender<Msg>,
    sampler: Arc<Mutex<Sampler>>,
    task: Option<JoinHandle<()>>,
}

impl Network {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, sampler: Arc::default(), task: None }
    }

    pub fn request(&mut self, id: u64) {
        self.cancel();
        let (sampler, messages) = (self.sampler.clone(), self.messages.clone());
        self.task = Some(tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                sampler.lock().map_err(|error| anyhow!(error.to_string())).and_then(|mut sampler| sampler.sample())
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
            .map_err(|error| tr!(network_cannot_read, error = format!("{error:#}")));
            let _ = messages.send(Msg::Network { id, result });
        }));
    }

    /// Looks up the names of `addresses` all at once; each one that takes too long stays
    /// without a name.
    pub fn resolve(&self, addresses: Vec<IpAddr>) {
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let lookups = addresses.into_iter().map(|address| async move {
                let lookup = tokio::task::spawn_blocking(move || host_name(address));
                let name = tokio::time::timeout(RESOLVE_TIMEOUT, lookup).await.ok().and_then(Result::ok).flatten();
                (address, name)
            });
            let _ = messages.send(Msg::HostNames(futures::future::join_all(lookups).await));
        });
    }

    /// Stops the running sample once the screen no longer waits for it.
    pub fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The name the system's resolver has for `address`, when it has one.
fn host_name(address: IpAddr) -> Option<String> {
    dns_lookup::lookup_addr(&address).ok().filter(|name| name.parse::<IpAddr>().is_err())
}

#[derive(Default)]
struct Sampler {
    system: System,
    networks: Option<Networks>,
    /// The kinds the system names for its interfaces, read once.
    hardware: Option<HashMap<String, InterfaceKind>>,
}

impl Sampler {
    fn sample(&mut self) -> Result<NetworkSample> {
        let (sockets, reported) = sockets()?;
        let names = self.names(&sockets, reported);
        Ok(NetworkSample { sockets, interfaces: self.interfaces(), names })
    }

    /// The name of each process that holds a socket, as the activity monitor shows it, or as
    /// the system's socket list gave it for a process that has gone.
    fn names(&mut self, sockets: &[Socket], mut reported: HashMap<u32, String>) -> HashMap<u32, String> {
        let pids: Vec<Pid> = sockets
            .iter()
            .filter_map(|socket| socket.pid)
            .collect::<HashSet<_>>()
            .into_iter()
            .map(Pid::from_u32)
            .collect();
        self.system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing());
        pids.iter()
            .filter_map(|pid| {
                let name = match self.system.process(*pid) {
                    Some(process) => process.name().to_string_lossy().into_owned(),
                    None => reported.remove(&pid.as_u32())?,
                };
                Some((pid.as_u32(), name))
            })
            .collect()
    }

    fn interfaces(&mut self) -> Vec<Interface> {
        let networks = self.networks.get_or_insert_with(Networks::new_with_refreshed_list);
        networks.refresh(true);
        let hardware = self.hardware.get_or_insert_with(kinds::hardware);
        let mut interfaces: Vec<Interface> = networks
            .list()
            .iter()
            .map(|(name, data)| {
                let addresses: Vec<(IpAddr, u8)> =
                    data.ip_networks().iter().map(|network| (network.addr, network.prefix)).collect();
                let up = match data.operational_state() {
                    InterfaceOperationalState::Up => true,
                    InterfaceOperationalState::Unknown => !addresses.is_empty(),
                    _ => false,
                };
                Interface {
                    name: name.clone(),
                    kind: kinds::kind(name, hardware),
                    up,
                    addresses,
                    mac: Some(data.mac_address()).filter(|mac| !mac.is_unspecified()).map(|mac| mac.to_string()),
                    mtu: Some(data.mtu()).filter(|mtu| *mtu > 0),
                    received: data.total_received(),
                    sent: data.total_transmitted(),
                    packets_received: data.total_packets_received(),
                    packets_sent: data.total_packets_transmitted(),
                    errors: data.total_errors_on_received() + data.total_errors_on_transmitted(),
                }
            })
            .collect();
        interfaces.sort_by(|a, b| a.name.cmp(&b.name));
        interfaces
    }
}

/// Every TCP and UDP socket on the machine, and the names the list gave their processes.
#[cfg(target_os = "macos")]
fn sockets() -> Result<(Vec<Socket>, HashMap<u32, String>)> {
    let output = std::process::Command::new(netstat::PROGRAM).args(netstat::ARGUMENTS).output()?;
    if !output.status.success() {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(netstat::parse(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(target_os = "linux")]
fn sockets() -> Result<(Vec<Socket>, HashMap<u32, String>)> {
    Ok((proc_net::read()?, HashMap::new()))
}

#[cfg(test)]
mod tests;
