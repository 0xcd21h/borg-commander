//! What kind of link each interface is: the hardware ports macOS names, the device Linux
//! describes in `/sys/class/net`, and the interface's name for the rest.

use borg_core::InterfaceKind;
use std::collections::HashMap;

#[cfg(target_os = "macos")]
const NETWORKSETUP: &str = "/usr/sbin/networksetup";
#[cfg(target_os = "macos")]
const HARDWARE_PORTS: &str = "-listallhardwareports";
#[cfg(target_os = "linux")]
const SYS_NET: &str = "/sys/class/net";

#[cfg(any(target_os = "macos", test))]
const PORT_FIELD: &str = "Hardware Port:";
#[cfg(any(target_os = "macos", test))]
const DEVICE_FIELD: &str = "Device:";
/// Words in a hardware port's name that say what it is; any other port is wired.
#[cfg(any(target_os = "macos", test))]
const PORT_WORDS: [(&str, InterfaceKind); 3] =
    [("Wi-Fi", InterfaceKind::WiFi), ("AirPort", InterfaceKind::WiFi), ("Bridge", InterfaceKind::Bridge)];

/// The kinds the system names, read once: on macOS from its hardware ports.
pub(super) fn hardware() -> HashMap<String, InterfaceKind> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new(NETWORKSETUP)
            .arg(HARDWARE_PORTS)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| hardware_ports(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    HashMap::new()
}

/// The kind of the interface `name`, from what the system said or, failing that, its name.
pub(super) fn kind(name: &str, hardware: &HashMap<String, InterfaceKind>) -> InterfaceKind {
    if let Some(kind) = hardware.get(name) {
        return *kind;
    }
    #[cfg(target_os = "linux")]
    if let Some(kind) = described(name) {
        return kind;
    }
    InterfaceKind::from_name(name)
}

/// The kind the files Linux keeps for the device say.
#[cfg(target_os = "linux")]
fn described(name: &str) -> Option<InterfaceKind> {
    const MARKERS: [(&str, InterfaceKind); 4] = [
        ("wireless", InterfaceKind::WiFi),
        ("phy80211", InterfaceKind::WiFi),
        ("bridge", InterfaceKind::Bridge),
        ("tun_flags", InterfaceKind::Tunnel),
    ];
    const DEVICE: &str = "device";
    let dir = std::path::Path::new(SYS_NET).join(name);
    MARKERS.iter().find(|(marker, _)| dir.join(marker).exists()).map(|(_, kind)| *kind).or_else(|| {
        (InterfaceKind::from_name(name) == InterfaceKind::Other && dir.join(DEVICE).exists())
            .then_some(InterfaceKind::Ethernet)
    })
}

/// Each device `networksetup -listallhardwareports` lists, by the kind its port's name says.
#[cfg(any(target_os = "macos", test))]
pub(super) fn hardware_ports(output: &str) -> HashMap<String, InterfaceKind> {
    let mut kinds = HashMap::new();
    let mut port = None;
    for line in output.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix(PORT_FIELD) {
            port = Some(name.trim());
        } else if let (Some(device), Some(name)) = (line.strip_prefix(DEVICE_FIELD), port.take()) {
            let kind = PORT_WORDS
                .iter()
                .find(|(word, _)| name.contains(word))
                .map_or(InterfaceKind::Ethernet, |(_, kind)| *kind);
            kinds.insert(device.trim().to_owned(), kind);
        }
    }
    kinds
}
