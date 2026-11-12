//! Whether alerts leave the machine, and what Borg says when they do not: the log on this
//! machine is all there is then, and once freezing starts Borg acts with nobody told.

use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

use super::{AlertLog, WatchSource};

/// How long before freezing starts Borg says that it is about to.
pub const FREEZING_SOON: Duration = Duration::from_secs(24 * 60 * 60);

/// Where alerts go besides the log on this machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertDelivery {
    /// Nowhere.
    #[default]
    Local,
    /// A heartbeat says the watcher runs, and carries no alerts.
    HeartbeatOnly,
    /// At least one place takes alerts.
    Sent,
}

/// How close Borg is to acting on what it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freezing {
    /// Borg only writes alerts down, for now or for good.
    NotYet,
    /// Freezing starts within [`FREEZING_SOON`], or waits only for the review of the learning
    /// period.
    Soon,
    Started,
}

/// Alerts that reach nobody: what the Alerts page and `borg service watch` warn about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsent {
    /// A heartbeat is set up, which the user may take for delivery.
    pub heartbeat: bool,
    pub freezing: Freezing,
}

impl Unsent {
    /// The warning for a watcher that is turned on with these places to send to, before it
    /// has said how it stands.
    pub fn configured(delivery: AlertDelivery) -> Option<Self> {
        Self::of(delivery, Freezing::NotYet)
    }

    fn of(delivery: AlertDelivery, freezing: Freezing) -> Option<Self> {
        match delivery {
            AlertDelivery::Local => Some(Self { heartbeat: false, freezing }),
            AlertDelivery::HeartbeatOnly => Some(Self { heartbeat: true, freezing }),
            AlertDelivery::Sent => None,
        }
    }

    /// The warning in a line: how things stand, then what to do about it.
    pub fn line(self) -> String {
        let state = match self.freezing {
            Freezing::NotYet => tr!(watch_unsent_local),
            Freezing::Soon => tr!(watch_unsent_soon),
            Freezing::Started => tr!(watch_unsent_freezing),
        };
        let fix = if self.heartbeat { tr!(watch_unsent_fix_heartbeat) } else { tr!(watch_unsent_fix) };
        tr!(watch_unsent, state = state, fix = fix)
    }
}

impl AlertLog {
    /// How close the running watcher is to freezing what it finds.
    fn freezing(&self, now: SystemTime) -> Freezing {
        let watch = &self.watch;
        if !watch.freeze {
            Freezing::NotYet
        } else if watch.learning(now) {
            let soon = watch.learned_at.duration_since(now).is_ok_and(|left| left <= FREEZING_SOON);
            if soon { Freezing::Soon } else { Freezing::NotYet }
        } else if self.freezing_waits(now).is_some() {
            Freezing::Soon
        } else {
            Freezing::Started
        }
    }

    /// The warning for alerts that reach nobody. A running watcher has said where its alerts
    /// go. For one that does not run, `configured` speaks: where the configuration sends
    /// alerts, when it turns the watcher on.
    pub fn unsent(&self, configured: Option<AlertDelivery>, now: SystemTime) -> Option<Unsent> {
        if self.watch.source == WatchSource::Off {
            return configured.and_then(Unsent::configured);
        }
        Unsent::of(self.watch.delivery, self.freezing(now))
    }
}
