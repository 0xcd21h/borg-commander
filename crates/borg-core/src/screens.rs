//! What Borg's full-width screens share: the activity monitor, the container list, the network
//! screen, the startup items and the alerts. One of them shows at a time.

use crate::AppState;

/// Rows PgUp and PgDn move a list.
pub(crate) const PAGE: usize = 10;
/// Characters a screen's filter holds at most.
pub(crate) const FILTER_LIMIT: usize = 128;
/// Rows a list pane's frame, summary, filter, column titles and footer take.
pub(crate) const CHROME_ROWS: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Activity,
    Docker,
    Network,
    Startup,
    Alerts,
}

const SCREENS: [Screen; 5] = [Screen::Activity, Screen::Docker, Screen::Network, Screen::Startup, Screen::Alerts];

fn close(state: &mut AppState, screen: Screen) {
    match screen {
        Screen::Activity => state.activity.side = None,
        Screen::Docker => crate::docker::close(state),
        Screen::Network => crate::network::close(state),
        Screen::Startup => crate::startup_items::close(state),
        Screen::Alerts => crate::watch::close(state),
    }
}

/// Closes every screen, as the panels' own views need.
pub(crate) fn close_all(state: &mut AppState) {
    SCREENS.into_iter().for_each(|screen| close(state, screen));
}

/// Closes every screen but the one about to open.
pub(crate) fn close_others(state: &mut AppState, opening: Screen) {
    SCREENS.into_iter().filter(|screen| *screen != opening).for_each(|screen| close(state, screen));
}
