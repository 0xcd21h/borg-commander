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
const QUICK_VIEW_TITLE: &str = tr!(quick_view);
const INFO_TITLE: &str = tr!(info);
const TITLE_SEPARATOR: &str = " · ";
const LOADING: &str = "…";
const BINARY: &str = tr!(binary_file);
const EMPTY: &str = tr!(empty_file);
const PADDING: u16 = 1;
const LABEL_GAP: usize = 2;
const STAMP: &str = "%Y-%m-%d %H:%M:%S";
const ARROW: &str = " → ";
const FOLDER: &str = tr!(folder);
const FILE: &str = tr!(file);
const LINK: &str = tr!(link);
const SPECIAL: &str = tr!(special);
const BYTES: &str = tr!(bytes);
const OCTAL_WIDTH: usize = 4;

const NAME_LABEL: &str = tr!(name);
const KIND_LABEL: &str = tr!(kind);
const SIZE_LABEL: &str = tr!(size);
const CONTENTS_LABEL: &str = tr!(contents);
const MODE_LABEL: &str = tr!(mode);
const OWNER_LABEL: &str = tr!(owner);
const MODIFIED_LABEL: &str = tr!(modified);
const ACCESSED_LABEL: &str = tr!(accessed);
const CREATED_LABEL: &str = tr!(created);
const INODE_LABEL: &str = tr!(inode);
const LINKS_LABEL: &str = tr!(links);
const DISK_LABEL: &str = tr!(free_space);

pub struct AsideScene<'a> {
    pub aside: Aside,
    pub focused: bool,
    pub peeked: Option<&'a Peeked>,
    pub area: Rect,
    pub icons: bool,
    pub icon_spacing: u8,
    pub pictures: bool,
    pub clock: &'a Zoned,
}

pub fn draw_aside(scene: &AsideScene<'_>, look: &Look<'_>, buf: &mut Buffer) -> Vec<Placement> {
    let styles = look.styles();
    let shown = scene.peeked.filter(|peeked| peeked.aside == scene.aside);
    let heading = match scene.aside {
        Aside::QuickView => QUICK_VIEW_TITLE,
        Aside::Info => INFO_TITLE,
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
            centered(LOADING, styles.meta, body, buf);
            Vec::new()
        }
    }
}

fn content(peeked: &Peeked, scene: &AsideScene<'_>, look: &Look<'_>, body: Rect, buf: &mut Buffer) -> Vec<Placement> {
    let styles = look.styles();
    match &peeked.peek {
        Peek::Picture(image) => return picture(image, scene, body, buf),
        Peek::Text(pager) if pager.is_empty() => centered(EMPTY, styles.meta, body, buf),
        Peek::Text(pager) => crate::document_view::draw(pager, styles.panel, body, buf),
        Peek::Folder { entries, .. } => lines(entries.iter().map(|entry| listed(entry, scene, look)), body, buf),
        Peek::Info(facts) => info(facts, &peeked.path, scene.clock, look, body, buf),
        Peek::Binary => centered(BINARY, styles.meta, body, buf),
        Peek::Unavailable(reason) => {
            centered(&format::truncate_end(reason, usize::from(body.width)), styles.notice, body, buf);
        }
    }
    Vec::new()
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
        Peek::Text(pager) => Some(format!(
            tr!(tab_focus),
            pager.percent(),
            if pager.markdown() { tr!(s_source_render) } else { tr!(w_wrap) }
        )),
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
        Nature::Folder => FOLDER.to_owned(),
        Nature::File => FILE.to_owned(),
        Nature::Link(target) => format!("{LINK}{ARROW}{}", target.display()),
        Nature::Special => SPECIAL.to_owned(),
    }
}

fn facts(facts: &Facts, path: &Path, clock: &Zoned) -> Vec<(&'static str, String)> {
    let exact = format!("{} {BYTES}", facts.size);
    let human = format::size(facts.size);
    let size = if human == facts.size.to_string() { exact } else { format!("{human} ({exact})") };
    let rows = [
        Some((NAME_LABEL, name(path))),
        Some((KIND_LABEL, nature(&facts.nature))),
        facts
            .items
            .map(|items| (CONTENTS_LABEL, format::count(items, format::Count::Item)))
            .or(Some((SIZE_LABEL, size))),
        Some((MODE_LABEL, format!("{} {:0OCTAL_WIDTH$o}", rwx(facts.permissions), facts.permissions))),
        Some((OWNER_LABEL, format!("{}:{}", facts.owner, facts.group))),
        stamp(facts.modified, clock).map(|time| (MODIFIED_LABEL, time)),
        stamp(facts.accessed, clock).map(|time| (ACCESSED_LABEL, time)),
        stamp(facts.created, clock).map(|time| (CREATED_LABEL, time)),
        Some((INODE_LABEL, facts.inode.to_string())),
        Some((LINKS_LABEL, facts.links.to_string())),
        facts
            .disk
            .map(|(free, total)| (DISK_LABEL, format!(tr!(space_of_total), format::size(free), format::size(total)))),
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
                Span::styled(format!("{label:<label_width$}"), styles.meta),
                Span::styled(format::truncate_end(&value, value_width), styles.panel.add_modifier(Modifier::BOLD)),
            ])
        }),
        body,
        buf,
    );
}
