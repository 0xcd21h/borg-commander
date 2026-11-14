use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::PortScanRecord;
use crate::Mac;

pub const PORT_SCANS_FORMAT: u8 = 1;
const MAX_SCANS: usize = 256;
/// The longest name a host can have.
pub const MAX_TARGET: usize = 253;
const UNKNOWN_FORMAT: &str = "unsupported port scans format";
const TOO_MANY_SCANS: &str = "too many remembered port scans";
const BAD_SCAN: &str = "a remembered port scan does not add up";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPortScans {
    pub version: u8,
    pub scans: Vec<PortScanRecord>,
}

/// The last port scan of each host, newest first. A private address behind another gateway is
/// another host, so a scan is known by its address and the gateway it was found behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortScanMemory {
    scans: Vec<PortScanRecord>,
}

impl PortScanMemory {
    pub fn note(&mut self, record: PortScanRecord) {
        self.scans.retain(|scan| (scan.address, scan.gateway) != (record.address, record.gateway));
        self.scans.insert(0, record);
        self.scans.truncate(MAX_SCANS);
    }

    pub fn find(&self, address: IpAddr, gateway: Option<Mac>) -> Option<&PortScanRecord> {
        self.scans.iter().find(|scan| scan.address == address && scan.gateway == gateway)
    }

    /// The newest scan of a target as typed, by its name or its address.
    pub fn last(&self, target: &str) -> Option<&PortScanRecord> {
        let target = target.trim();
        self.scans.iter().find(|scan| scan.target.eq_ignore_ascii_case(target) || scan.address.to_string() == target)
    }

    pub fn saved(&self) -> SavedPortScans {
        SavedPortScans { version: PORT_SCANS_FORMAT, scans: self.scans.clone() }
    }

    pub fn restore(saved: SavedPortScans) -> Result<Self, &'static str> {
        if saved.version != PORT_SCANS_FORMAT {
            return Err(UNKNOWN_FORMAT);
        }
        if saved.scans.len() > MAX_SCANS {
            return Err(TOO_MANY_SCANS);
        }
        if !saved.scans.iter().all(PortScanRecord::adds_up) {
            return Err(BAD_SCAN);
        }
        Ok(Self { scans: saved.scans })
    }
}
