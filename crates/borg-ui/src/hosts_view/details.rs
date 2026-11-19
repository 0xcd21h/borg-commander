//! The selected host's facts beside the cards: who it is, how Borg reaches it, and each figure
//! in words.

use std::time::{Duration, Instant, SystemTime};

use borg_core::{HostCard, HostFigures, Hosts, Reach};
use borg_i18n::tr;
use jiff::Zoned;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthStr;

use super::{SEPARATOR, concern_style, containers_text, percent, silent, watch_text};
use crate::{
    activity_view::{dim, row, text, wrap},
    format,
    look::Look,
};

/// Cells between a fact's label and its value.
const LABEL_GAP: u16 = 2;

pub(super) fn draw(hosts: &Hosts, look: &Look<'_>, rect: Rect, clock: &Zoned, now: Instant, buf: &mut Buffer) {
    let title = hosts.selected().map_or(tr!(hosts_title), HostCard::name);
    let area = crate::results_view::pane_frame(hosts.expanded, look, rect, title, false, buf);
    let Some(card) = hosts.selected() else { return };
    if area.is_empty() {
        return;
    }
    let mut facts = Facts::new(area, look);
    let theme = look.theme;
    match &card.reach {
        Reach::Silent { since } => facts.line(buf, &silent(*since, now), theme.styles.notice),
        Reach::Failed(error) => facts.line(buf, error, theme.styles.notice),
        Reach::Offline => facts.line(buf, tr!(hosts_offline_now), dim(theme)),
        Reach::Waiting => facts.line(buf, tr!(hosts_asking), dim(theme)),
        Reach::Answering => {}
    }
    if card.label.is_some() {
        facts.add(buf, tr!(hosts_machine_label), &card.machine, None);
    }
    facts.add(buf, tr!(hosts_system_label), card.info.platform.as_deref().unwrap_or_default(), None);
    let version = match card.info.version.as_deref() {
        Some(version) if card.older() => tr!(hosts_older, version = version),
        Some(version) => version.to_owned(),
        None => String::new(),
    };
    facts.add(buf, tr!(hosts_version_column), &version, None);
    facts.add(buf, tr!(hosts_relay_label), &card.info.relay, None);
    if let Some(latency) = card.info.latency_ms {
        facts.add(buf, tr!(hosts_round_trip_label), &tr!(hosts_latency, ms = latency), None);
    }
    let sessions: Vec<&str> = card.sessions.iter().map(|session| session.name.as_str()).collect();
    facts.add(buf, tr!(hosts_sessions_label), &sessions.join(", "), None);
    if let Some(seen) = card.last_seen {
        facts.add(buf, tr!(hosts_answered_label), &format::exact_stamp(Some(seen), clock), None);
    }
    if let Some(figures) = &card.figures {
        facts.gap();
        figure_facts(&mut facts, figures, buf);
    }
}

fn figure_facts(facts: &mut Facts<'_, '_>, figures: &HostFigures, buf: &mut Buffer) {
    let theme = facts.look.theme;
    if let Some(uptime) = figures.uptime {
        facts.add(buf, tr!(summary_uptime), &borg_core::uptime_text(Duration::from_secs(uptime)), None);
    }
    if let Some(cpu) = figures.cpu {
        facts.add(buf, tr!(summary_cpu), &percent(cpu), None);
    }
    if let Some(reading) = figures.load.and_then(|load| borg_core::load_reading(load, figures.cores)) {
        let mut load = borg_core::load_text(reading);
        if let Some(waiting) = reading.waiting {
            load = format!("{load}{SEPARATOR}{}", borg_core::waiting_text(waiting));
        }
        facts.add(buf, tr!(summary_load), &load, Some(concern_style(figures.load_concern(), theme)));
    }
    if let Some(memory) = figures.memory.filter(|memory| memory.total > 0) {
        let used = tr!(hosts_of, used = format::size(memory.used), total = format::size(memory.total));
        facts.add(buf, tr!(summary_ram), &used, Some(concern_style(figures.memory_concern(), theme)));
        if memory.swap_total > 0 {
            let swap = tr!(hosts_of, used = format::size(memory.swap_used), total = format::size(memory.swap_total));
            facts.add(buf, tr!(summary_swap), &swap, None);
        }
    }
    if let Some(disk) = &figures.disk {
        let value = tr!(
            hosts_disk_value,
            mount = disk.mount,
            free = format::size(disk.available),
            total = format::size(disk.total)
        );
        facts.add(buf, tr!(summary_disk), &value, Some(concern_style(figures.disk_concern(), theme)));
    }
    if let Some(containers) = figures.containers.filter(|containers| containers.total > 0) {
        let style = concern_style(figures.containers_concern(), theme);
        facts.add(buf, tr!(hosts_containers_column), &containers_text(containers), Some(style));
    }
    if let Some(watch) = figures.watch {
        facts.gap();
        facts.line(buf, &watch_text(&watch, SystemTime::now()), concern_style(figures.watch_concern(), theme));
    }
}

/// Labelled values one under another, long ones wrapped under their label.
struct Facts<'a, 'b> {
    area: Rect,
    y: u16,
    label: u16,
    look: &'b Look<'a>,
}

impl<'a, 'b> Facts<'a, 'b> {
    fn new(area: Rect, look: &'b Look<'a>) -> Self {
        let labels = [
            tr!(hosts_machine_label),
            tr!(hosts_system_label),
            tr!(hosts_version_column),
            tr!(hosts_relay_label),
            tr!(hosts_round_trip_label),
            tr!(hosts_sessions_label),
            tr!(hosts_answered_label),
            tr!(hosts_containers_column),
        ];
        let label = labels.iter().chain(borg_core::bar_labels().iter()).map(|label| label.width()).max().unwrap_or(0);
        let label = (u16::try_from(label).unwrap_or(u16::MAX) + LABEL_GAP).min(area.width / 2);
        Self { area, y: 0, label, look }
    }

    /// A line of its own across the pane, wrapped.
    fn line(&mut self, buf: &mut Buffer, value: &str, style: Style) {
        for line in wrap(value, usize::from(self.area.width)) {
            if self.y >= self.area.height {
                return;
            }
            text(buf, row(self.area, self.y), line, style);
            self.y += 1;
        }
    }

    fn gap(&mut self) {
        self.y = self.y.saturating_add(1);
    }

    fn add(&mut self, buf: &mut Buffer, label: &str, value: &str, style: Option<Style>) {
        if value.is_empty() {
            return;
        }
        let theme = self.look.theme;
        let width = self.area.width.saturating_sub(self.label);
        for (index, line) in wrap(value, usize::from(width)).into_iter().enumerate() {
            if self.y >= self.area.height {
                return;
            }
            let at = row(self.area, self.y);
            if index == 0 {
                text(buf, Rect { width: self.label.min(at.width), ..at }, label, dim(theme));
            }
            let rest = Rect { x: at.x + self.label.min(at.width), width, ..at };
            let style = style.unwrap_or(theme.styles.overlay);
            text(buf, rest, format::truncate_end(&line, usize::from(rest.width)), style);
            self.y += 1;
        }
    }
}
