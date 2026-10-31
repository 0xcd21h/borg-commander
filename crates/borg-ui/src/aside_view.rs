use borg_i18n::tr;
use std::{path::Path, time::SystemTime};

use borg_core::{Aside, Facts, Image, Nature, Peek, Peeked};
use borg_gfx::{Placement, draw_halfblocks};
use borg_vfs::Entry;
use jiff::{Timestamp, Zoned};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    format, icons,
    look::Look,
    panel_view::{ICON_GAP, style_of},
    status_view::rwx,
};

const PICTURE_Z: i32 = -1;
const TITLE_SEPARATOR: &str = " · ";
const PADDING: u16 = 1;
const LABEL_GAP: usize = 2;
const STAMP: &str = "%Y-%m-%d %H:%M:%S";
const ARROW: &str = " → ";
const OCTAL_WIDTH: usize = 4;

pub struct AsideScene<'a> {
    pub aside: Aside,
    pub focused: bool,
    pub peeked: Option<&'a Peeked>,
    pub area: Rect,
    pub icons: bool,
    pub icon_spacing: u8,
    pub pictures: bool,
    pub clock: &'a Zoned,
    /// Shown once the preview has taken a while to prepare.
    pub spinner: Option<crate::spinner::Spinner>,
}

pub fn draw_aside(scene: &AsideScene<'_>, look: &Look<'_>, buf: &mut Buffer) -> Vec<Placement> {
    let styles = look.styles();
    let shown = scene.peeked.filter(|peeked| peeked.aside == scene.aside);
    let heading = match scene.aside {
        Aside::QuickView => tr!(quick_view),
        Aside::Info => tr!(info),
        Aside::OpenFile => tr!(process_file_details),
    };
    let name = shown.map(|peeked| name(&peeked.path)).unwrap_or_default();
    let room = usize::from(scene.area.width).saturating_sub(heading.width() + TITLE_SEPARATOR.width() + LABEL_GAP * 2);
    let title = if name.is_empty() {
        format!(" {heading} ")
    } else {
        format!(" {heading}{TITLE_SEPARATOR}{} ", format::truncate_end(&name, room))
    };
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.panel)
        .border_style(if scene.focused { styles.border_active } else { styles.border })
        .title(Line::styled(title, styles.title));
    let block = match shown.and_then(|peeked| footer(&peeked.peek)) {
        Some(footer) => block.title_bottom(Line::styled(
            format!(" {}{footer} ", if scene.focused { tr!(focused_tab_back) } else { "" }),
            styles.footer,
        )),
        None => block,
    };
    let inner = block.inner(scene.area);
    block.render(scene.area, buf);
    let body = Rect { x: inner.x + PADDING, width: inner.width.saturating_sub(PADDING * 2), ..inner };
    match shown {
        Some(peeked) => content(peeked, scene, look, body, buf),
        None => {
            if let Some(spinner) = scene.spinner {
                spinner.draw_centered(tr!(activity_loading), look, styles.meta, body, buf);
            }
            Vec::new()
        }
    }
}

fn content(peeked: &Peeked, scene: &AsideScene<'_>, look: &Look<'_>, body: Rect, buf: &mut Buffer) -> Vec<Placement> {
    match &peeked.peek {
        Peek::Picture(image) => return picture(image, scene, body, buf),
        Peek::Folder { entries, .. } => lines(entries.iter().map(|entry| listed(entry, scene, look)), body, buf),
        Peek::Info(facts) => info(facts, &peeked.path, scene.clock, look, body, buf),
        peek => draw_file(peek, look, look.styles().panel, body, buf),
    }
    Vec::new()
}

/// What a file holds, for a pane that places no graphics: a picture draws in half blocks.
pub(crate) fn draw_file(peek: &Peek, look: &Look<'_>, base: Style, body: Rect, buf: &mut Buffer) {
    let styles = look.styles();
    match peek {
        Peek::Picture(image) => draw_halfblocks(&image.picture, body, buf),
        Peek::Text(pager) if pager.is_empty() => centered(tr!(empty_file), styles.meta, body, buf),
        Peek::Text(pager) => crate::document_view::draw(pager, base, body, buf),
        Peek::Binary => centered(tr!(binary_file), styles.meta, body, buf),
        Peek::Unavailable(reason) => {
            centered(&format::truncate_end(reason, usize::from(body.width)), styles.notice, body, buf);
        }
        Peek::Folder { .. } | Peek::Info(_) => {}
    }
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn footer(peek: &Peek) -> Option<String> {
    match peek {
        Peek::Folder { folders, files, bytes, .. } => Some(format!(
            "{} · {} · {}",
            format::count(*folders, format::Count::Folder),
            format::count(*files, format::Count::File),
            format::size(*bytes)
        )),
        Peek::Picture(image) => {
            let (width, height) = image.original;
            Some(format!("{width}×{height} · {} · {}", image.format.to_uppercase(), format::size(image.bytes)))
        }
        Peek::Text(pager) => {
            Some(tr!(tab_focus, pager.percent(), if pager.markdown() { tr!(s_source_render) } else { tr!(w_wrap) }))
        }
        Peek::Info(_) | Peek::Binary | Peek::Unavailable(_) => None,
    }
}

fn lines<'a>(rows: impl Iterator<Item = Line<'a>>, body: Rect, buf: &mut Buffer) {
    for (y, line) in (body.top()..body.bottom()).zip(rows) {
        line.render(Rect { y, height: 1, ..body }, buf);
    }
}

fn centered(text: &str, style: Style, body: Rect, buf: &mut Buffer) {
    let y = body.y + body.height / 2;
    if body.height > 0 {
        Line::styled(text.to_owned(), style).centered().render(Rect { y, height: 1, ..body }, buf);
    }
}

fn picture(image: &Image, scene: &AsideScene<'_>, body: Rect, buf: &mut Buffer) -> Vec<Placement> {
    if scene.pictures {
        return vec![Placement::fitted(image.picture.clone(), body, PICTURE_Z)];
    }
    draw_halfblocks(&image.picture, body, buf);
    Vec::new()
}

fn listed<'a>(entry: &Entry, scene: &AsideScene<'_>, look: &Look<'_>) -> Line<'a> {
    let style = style_of(entry, look.styles());
    let icon = Some(if scene.icons { icons::icon(entry) } else { icons::portable(entry) });
    let gap = ICON_GAP.to_string().repeat(usize::from(scene.icon_spacing));
    let lead = icon
        .map(|icon| Span::styled(format!("{}{gap}", icon.glyph), icon.color.map_or(style, |color| style.fg(color))));
    Line::from(lead.into_iter().chain(std::iter::once(Span::styled(entry.name.clone(), style))).collect::<Vec<_>>())
}

fn stamp(time: Option<SystemTime>, clock: &Zoned) -> Option<String> {
    let stamp = Timestamp::try_from(time?).ok()?;
    Some(stamp.to_zoned(clock.time_zone().clone()).strftime(STAMP).to_string())
}

fn nature(nature: &Nature) -> String {
    match nature {
        Nature::Folder => tr!(folder).to_owned(),
        Nature::File => tr!(file).to_owned(),
        Nature::Link(target) => format!("{}{ARROW}{}", tr!(link), target.display()),
        Nature::Special => tr!(special).to_owned(),
    }
}

fn facts(facts: &Facts, path: &Path, clock: &Zoned) -> Vec<(&'static str, String)> {
    let exact = format!("{} {}", facts.size, tr!(bytes));
    let human = format::size(facts.size);
    let size = if human == facts.size.to_string() { exact } else { format!("{human} ({exact})") };
    let rows = [
        Some((tr!(name), name(path))),
        Some((tr!(kind), nature(&facts.nature))),
        facts.items.map(|items| (tr!(contents), format::count(items, format::Count::Item))).or(Some((tr!(size), size))),
        Some((tr!(mode), format!("{} {:0OCTAL_WIDTH$o}", rwx(facts.permissions), facts.permissions))),
        Some((tr!(owner), format!("{}:{}", facts.owner, facts.group))),
        stamp(facts.modified, clock).map(|time| (tr!(modified), time)),
        stamp(facts.accessed, clock).map(|time| (tr!(accessed), time)),
        stamp(facts.created, clock).map(|time| (tr!(created), time)),
        Some((tr!(inode), facts.inode.to_string())),
        Some((tr!(links), facts.links.to_string())),
        facts.disk.map(|(free, total)| (tr!(free_space), tr!(space_of_total, format::size(free), format::size(total)))),
        facts.backup.as_ref().map(|backup| (tr!(backup_info_label), crate::backup_view::status_label(backup))),
    ];
    rows.into_iter().flatten().collect()
}

fn info(facts_of: &Facts, path: &Path, clock: &Zoned, look: &Look<'_>, body: Rect, buf: &mut Buffer) {
    let styles = look.styles();
    let rows = facts(facts_of, path, clock);
    let label_width = rows.iter().map(|(label, _)| label.width()).max().unwrap_or_default() + LABEL_GAP;
    let value_width = usize::from(body.width).saturating_sub(label_width);
    lines(
        rows.into_iter().map(|(label, value)| {
            Line::from(vec![
                Span::styled(format::pad_end(label, label_width), styles.meta),
                Span::styled(format::truncate_end(&value, value_width), styles.panel.add_modifier(Modifier::BOLD)),
            ])
        }),
        body,
        buf,
    );
}
