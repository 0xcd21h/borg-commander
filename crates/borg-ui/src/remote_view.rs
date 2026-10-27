use borg_core::RemoteBrowser;
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget, Wrap},
};

use crate::{Theme, dialog_view::centered, format};

pub fn draw(browser: &RemoteBrowser, theme: &Theme, screen: Rect, buf: &mut Buffer, t: f32) -> Rect {
    let width = screen.width.saturating_sub(2).min(104);
    let height = screen.height.saturating_sub(2).clamp(5, 20).min(screen.height);
    let rect = centered(screen, width, height);
    let styles = &theme.styles;
    Clear.render(rect, buf);
    Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(remote_colony_title), styles.overlay_border).centered())
        .title_bottom(
            Line::styled(
                if browser.connecting {
                    tr!(remote_connecting_keys)
                } else if browser.deleting.is_some() {
                    tr!(remote_session_removing_keys)
                } else if browser.show_clients {
                    tr!(remote_clients_keys)
                } else if width >= 104 {
                    tr!(remote_colony_keys)
                } else {
                    tr!(remote_colony_keys_compact)
                },
                styles.overlay,
            )
            .centered(),
        )
        .render(rect, buf);
    if rect.height < 5 || rect.width < 8 {
        return rect;
    }
    let body = Rect {
        x: rect.x.saturating_add(2),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    Line::styled(format!(tr!(remote_client_role), version = env!("CARGO_PKG_VERSION")), styles.overlay)
        .render(body, buf);
    Line::from(vec![
        Span::styled(
            tr!(remote_hosts_tab),
            if browser.show_clients { styles.overlay } else { styles.overlay_selected },
        ),
        Span::raw("  "),
        Span::styled(
            tr!(remote_clients_tab),
            if browser.show_clients { styles.overlay_selected } else { styles.overlay },
        ),
    ])
    .render(Rect { y: body.y + 1, height: 1, ..body }, buf);
    let list = Rect { y: body.y.saturating_add(2), height: body.height.saturating_sub(7), ..body };
    if browser.show_clients {
        draw_clients(browser, theme, body, list, buf);
        return rect;
    }
    if browser.choices.is_empty() {
        let label = if browser.loading { tr!(remote_checking_hosts) } else { tr!(remote_no_hosts) };
        Line::styled(label, styles.overlay).render(list, buf);
    } else {
        let visible = usize::from(list.height);
        let start = browser.selected.saturating_sub(visible.saturating_sub(1));
        for (row, choice) in browser.choices.iter().skip(start).take(visible).enumerate() {
            let version = choice
                .info
                .version
                .as_ref()
                .map(|v| format!("v{v}"))
                .unwrap_or_else(|| tr!(remote_version_unknown).into());
            let label = format!(
                tr!(remote_host_session),
                machine = choice.host_name(),
                session = choice.session_name(),
                version = version
            );
            let text = format::truncate_end(&label, usize::from(list.width));
            let style = if start + row == browser.selected { styles.overlay_selected } else { styles.overlay };
            Line::styled(text, style).render(Rect { y: list.y + row as u16, height: 1, ..list }, buf);
        }
        if let Some(error) = &browser.error {
            let height = body.height.saturating_sub(1).min(3);
            Paragraph::new(format!(tr!(remote_mirror_connection_error), error = error))
                .style(styles.overlay)
                .wrap(Wrap { trim: false })
                .render(Rect { y: body.bottom().saturating_sub(height + 1), height, ..body }, buf);
            Line::styled(tr!(remote_mirror_retry), styles.overlay)
                .render(Rect { y: body.bottom().saturating_sub(1), height: 1, ..body }, buf);
        } else if let Some(choice) = browser.choices.get(browser.selected) {
            let info = &choice.info;
            let mode = match info.service {
                Some(true) => tr!(remote_service),
                Some(false) => tr!(remote_terminal),
                None => tr!(remote_unknown),
            };
            let latency = info.latency_ms.map(|ms| format!("{ms} ms")).unwrap_or_else(|| tr!(remote_unknown).into());
            let details = format!(
                tr!(remote_host_details),
                version = info.version.as_deref().unwrap_or(tr!(remote_version_unknown)),
                platform = info.platform.as_deref().unwrap_or(tr!(remote_unknown)),
                mode = mode,
                latency = latency
            );
            let y = body.bottom().saturating_sub(3);
            Line::styled(
                format!(tr!(remote_original_names), machine = choice.machine, session = choice.name),
                styles.overlay,
            )
            .render(Rect { y: y.saturating_sub(1), height: 1, ..body }, buf);
            Line::styled(details, styles.overlay).render(Rect { y, height: 1, ..body }, buf);
            Line::styled(format!(tr!(remote_relay), relay = info.relay), styles.overlay)
                .render(Rect { y: y + 1, height: 1, ..body }, buf);
        }
    }
    if browser.loading || browser.connecting || browser.deleting.is_some() {
        let glyph = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'][(t * 12.0) as usize % 10];
        let label = if browser.deleting.is_some() {
            tr!(remote_session_removing).into()
        } else {
            browser
                .choices
                .get(browser.selected)
                .filter(|_| browser.connecting)
                .map(|choice| {
                    format!(
                        tr!(remote_connecting_mirror),
                        machine = choice.host_name(),
                        session = choice.session_name()
                    )
                })
                .unwrap_or_else(|| tr!(remote_checking_hosts).into())
        };
        Line::styled(format!("{glyph} {label}"), styles.overlay)
            .render(Rect { y: body.bottom().saturating_sub(1), height: 1, ..body }, buf);
    }
    rect
}

fn draw_clients(browser: &RemoteBrowser, theme: &Theme, body: Rect, list: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    let Some(clients) = &browser.clients else {
        Line::styled(tr!(remote_clients_checking), styles.overlay).render(list, buf);
        return;
    };
    if clients.is_empty() {
        Line::styled(tr!(remote_clients_empty), styles.overlay).render(list, buf);
        return;
    }
    let visible = usize::from(list.height);
    let start = browser.client_selected.saturating_sub(visible.saturating_sub(1));
    for (row, client) in clients.iter().skip(start).take(visible).enumerate() {
        let status = match client.connections {
            Some(0) => tr!(remote_client_offline),
            Some(_) => tr!(remote_client_online),
            None => tr!(remote_client_status_unknown),
        };
        let label = format!(tr!(remote_client_row), name = client.name, status = status);
        let style = if start + row == browser.client_selected { styles.overlay_selected } else { styles.overlay };
        Line::styled(format::truncate_end(&label, usize::from(list.width)), style)
            .render(Rect { y: list.y + row as u16, height: 1, ..list }, buf);
    }
    if let Some(client) = clients.get(browser.client_selected) {
        let count = client.connections.map(|count| count.to_string()).unwrap_or_else(|| tr!(remote_unknown).into());
        for (row, label) in [
            tr!(remote_client_direction).to_owned(),
            format!(tr!(remote_client_fingerprint), fingerprint = client.fingerprint),
            format!(tr!(remote_client_connections), count = count),
            tr!(remote_clients_tab_hint).to_owned(),
        ]
        .into_iter()
        .enumerate()
        {
            if body.height >= 4 {
                Line::styled(label, styles.overlay)
                    .render(Rect { y: body.bottom() - 4 + row as u16, height: 1, ..body }, buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use borg_core::{RemoteBrowser, RemoteChoice};
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::{ThemeSet, remote_view::draw};

    #[test]
    fn paired_machines_and_sessions_are_visible_in_the_picker() {
        let themes = ThemeSet::builtin().unwrap();
        let screen = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(screen);
        let browser = RemoteBrowser {
            side: borg_core::Side::Left,
            choices: vec![RemoteChoice {
                machine: "server".into(),
                session: Some("id".into()),
                name: "MirrorTest".into(),
                info: borg_core::RemoteHostInfo {
                    version: Some("0.8.0".into()),
                    platform: Some("linux / x86_64".into()),
                    service: Some(true),
                    latency_ms: Some(42),
                    relay: "wss://relay.test".into(),
                },
                ..Default::default()
            }],
            selected: 0,
            loading: false,
            connecting: false,
            error: None,
            clients: None,
            show_clients: false,
            client_selected: 0,
            deleting: None,
        };
        draw(&browser, themes.find(None), screen, &mut buf, 0.0);
        let rendered: String = (0..screen.height)
            .map(|y| (0..screen.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Remote colony"));
        assert!(rendered.contains("This Borg: v"));
        assert!(rendered.contains("Host server · MirrorTest · v0.8.0"));
        assert!(rendered.contains("linux / x86_64"));
        assert!(rendered.contains("42 ms"));
        assert!(rendered.contains("M mirror"));
    }
}
