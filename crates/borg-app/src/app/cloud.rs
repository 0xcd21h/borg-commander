//! The cloud link: pairing this Borg, the relay it goes through, the computers paired with it,
//! and the agent's approve-all.

use super::App;
use borg_i18n::tr;

impl App {
    /// Makes a pairing link for the Connect dialog, or says why there is none.
    pub(super) fn start_pairing(&mut self) {
        let pairing = if self.cloud_suppressed {
            Err("The background service or viewer owns no pairing connection; use borg service pair.".into())
        } else if !self.state.settings.preferences.agent_cloud {
            Err(tr!(agent_cloud_off).to_owned())
        } else if self.link.relay().is_none() {
            Err(tr!(agent_cloud_no_relay).to_owned())
        } else {
            self.link.pair().map_err(|error| format!("{error:#}"))
        };
        self.state.agent.pairing = Some(pairing);
        self.link.sync(self.state.settings.preferences.agent_cloud && !self.cloud_suppressed);
    }

    /// Saves the relay typed into the Connect dialog, empty for none, in its full form, and
    /// asks it for a pairing link.
    pub(super) fn set_relay(&mut self, text: &str) {
        let relay = borg_link::Relay::parse(text);
        if relay.is_none() && !text.is_empty() {
            self.state.agent.connect.relay = self.link.relay().map(ToString::to_string).unwrap_or_default();
            self.state.agent.pairing = Some(Err(tr!(agent_cloud_bad_relay).to_owned()));
            return;
        }
        let saved = relay.as_ref().map(ToString::to_string);
        self.state.agent.connect.relay = saved.clone().unwrap_or_default();
        if let Some(borg_core::Overlay::Connect(connect)) = self.state.overlay.as_mut() {
            connect.relay = borg_core::LineInput::new(self.state.agent.connect.relay.clone());
        }
        self.config.relay = saved;
        self.save_config();
        self.link.set_relay(relay);
        self.start_pairing();
    }

    pub(super) fn forget_computers(&mut self) {
        if let Err(error) = self.link.forget() {
            self.state.notice = Some(format!("{error:#}"));
        }
    }

    /// Stops approving the agent's requests by themselves, here and in what the core remembers.
    pub(super) fn approval_mode_off(&mut self) {
        self.approve_all.clear();
        self.state.agent.auto_approve.clear();
        self.state.notice = Some("Approve all is off for this session.".into());
        self.agent.note("Approve all turned off locally");
    }
}
