use std::collections::HashMap;

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
};

use crate::{Capabilities, CellSize, Fit, GfxError, Picture, Protocol, encode, fit};

const IMAGE_BUDGET: usize = 192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    Fit,
    Stretch,
    Exact { offset: (u16, u16) },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub picture: Picture,
    pub area: Rect,
    pub z: i32,
    pub anchor: Anchor,
}

impl Placement {
    pub fn fitted(picture: Picture, area: Rect, z: i32) -> Self {
        Self { picture, area, z, anchor: Anchor::Fit }
    }

    pub fn stretched(picture: Picture, area: Rect, z: i32) -> Self {
        Self { picture, area, z, anchor: Anchor::Stretch }
    }

    pub fn exact(picture: Picture, origin: (u16, u16), offset: (u16, u16), z: i32) -> Self {
        let area = Rect { x: origin.0, y: origin.1, width: 0, height: 0 };
        Self { picture, area, z, anchor: Anchor::Exact { offset } }
    }

    pub fn fit(&self, cell: CellSize) -> Fit {
        let (width, height) = (self.picture.width(), self.picture.height());
        match self.anchor {
            Anchor::Fit => fit(width, height, self.area, cell),
            Anchor::Stretch => Fit {
                area: self.area,
                offset: (0, 0),
                size: (
                    u32::from(self.area.width) * u32::from(cell.width),
                    u32::from(self.area.height) * u32::from(cell.height),
                ),
            },
            Anchor::Exact { offset } => {
                let span = |offset: u16, pixels: u32, cell: u16| {
                    u16::try_from((u32::from(offset) + pixels).div_ceil(u32::from(cell.max(1)))).unwrap_or(u16::MAX)
                };
                let area = Rect {
                    width: span(offset.0, width, cell.width),
                    height: span(offset.1, height, cell.height),
                    ..self.area
                };
                Fit { area, offset, size: (width, height) }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub repaint: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    picture: u64,
    fit: Fit,
    z: i32,
    anchor: Anchor,
}

#[derive(Debug)]
struct Shown {
    key: Key,
    placement: u32,
    cells: Vec<Cell>,
}

#[derive(Debug)]
pub struct Graphics {
    capabilities: Capabilities,
    shown: Vec<Shown>,
    images: HashMap<u64, u32>,
    next_image: u32,
    next_placement: u32,
    errors: Vec<GfxError>,
}

fn region(buffer: &Buffer, area: Rect) -> Vec<Cell> {
    let area = area.intersection(buffer.area);
    (area.top()..area.bottom())
        .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
        .map(|(x, y)| buffer[(x, y)].clone())
        .collect()
}

impl Graphics {
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            shown: Vec::new(),
            images: HashMap::new(),
            next_image: 1,
            next_placement: 1,
            errors: Vec::new(),
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn take_errors(&mut self) -> Vec<GfxError> {
        std::mem::take(&mut self.errors)
    }

    fn key(&self, placement: &Placement) -> Key {
        let fit = placement.fit(self.capabilities.cell);
        Key { picture: placement.picture.id(), fit, z: placement.z, anchor: placement.anchor }
    }

    pub fn frame(&mut self, placements: &[Placement], buffer: &Buffer) -> Output {
        match self.capabilities.protocol {
            Protocol::Cells => Output::default(),
            Protocol::Kitty => self.kitty(placements),
            Protocol::Sixel | Protocol::Iterm => self.inline(placements, buffer),
        }
    }

    pub fn reset(&mut self) -> Vec<u8> {
        let removals: String = match self.capabilities.protocol {
            Protocol::Kitty => self.images.values().map(|image| encode::kitty_forget(*image)).collect(),
            Protocol::Sixel | Protocol::Iterm | Protocol::Cells => String::new(),
        };
        self.shown.clear();
        self.images.clear();
        removals.into_bytes()
    }

    fn kitty(&mut self, placements: &[Placement]) -> Output {
        let wanted: Vec<(Key, &Placement)> = placements.iter().map(|p| (self.key(p), p)).collect();
        let mut out = String::new();
        if self.images.len() > IMAGE_BUDGET {
            out.push_str(encode::KITTY_FORGET_ALL);
            self.images.clear();
            self.shown.clear();
        }
        let (kept, gone): (Vec<Shown>, Vec<Shown>) = std::mem::take(&mut self.shown)
            .into_iter()
            .partition(|shown| wanted.iter().any(|(key, _)| *key == shown.key));
        for shown in &gone {
            if let Some(image) = self.images.get(&shown.key.picture) {
                out.push_str(&encode::kitty_remove(*image, shown.placement));
            }
        }
        self.shown = kept;

        for (key, placement) in wanted {
            if self.shown.iter().any(|shown| shown.key == key) {
                continue;
            }
            let image = match self.images.get(&key.picture) {
                Some(image) => *image,
                None => {
                    let image = self.next_image;
                    match encode::kitty_transmit(&placement.picture, image) {
                        Ok(data) => out.push_str(&data),
                        Err(error) => {
                            self.errors.push(error);
                            continue;
                        }
                    }
                    self.next_image += 1;
                    self.images.insert(key.picture, image);
                    image
                }
            };
            let id = self.next_placement;
            self.next_placement += 1;
            let scaled = !matches!(key.anchor, Anchor::Exact { .. });
            out.push_str(&encode::kitty_place(&key.fit, image, id, key.z, scaled));
            self.shown.push(Shown { key, placement: id, cells: Vec::new() });
        }
        Output { bytes: out.into_bytes(), repaint: false }
    }

    fn inline(&mut self, placements: &[Placement], buffer: &Buffer) -> Output {
        let wanted: Vec<(Key, &Placement)> = placements.iter().map(|p| (self.key(p), p)).collect();
        let removed = self.shown.iter().any(|shown| !wanted.iter().any(|(key, _)| *key == shown.key));
        if removed {
            self.shown.clear();
            return Output { bytes: Vec::new(), repaint: true };
        }

        let mut out = String::new();
        let mut shown = Vec::with_capacity(wanted.len());
        for (key, placement) in wanted {
            let cells = region(buffer, key.fit.area);
            let unchanged = self.shown.iter().any(|old| old.key == key && old.cells == cells);
            if !unchanged {
                let encoded = match self.capabilities.protocol {
                    Protocol::Iterm => encode::iterm(&placement.picture, &key.fit),
                    Protocol::Sixel | Protocol::Kitty | Protocol::Cells => encode::sixel(&placement.picture, &key.fit),
                };
                match encoded {
                    Ok(data) => out.push_str(&data),
                    Err(error) => self.errors.push(error),
                }
            }
            shown.push(Shown { key, placement: 0, cells });
        }
        self.shown = shown;
        Output { bytes: out.into_bytes(), repaint: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellSize, Protocol};

    fn picture(shade: u8) -> Picture {
        Picture::from_rgba(2, 2, vec![shade; 16]).unwrap_or_else(|e| panic!("{e}"))
    }

    fn graphics(protocol: Protocol) -> Graphics {
        Graphics::new(Capabilities { protocol, cell: CellSize { width: 10, height: 20 } })
    }

    fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.bytes).into_owned()
    }

    #[test]
    fn kitty_transmits_once_and_only_diffs_placements() {
        let mut gfx = graphics(Protocol::Kitty);
        let buffer = Buffer::empty(Rect::new(0, 0, 20, 5));
        let one = Placement::fitted(picture(9), Rect::new(0, 0, 10, 4), 0);

        let first = text(&gfx.frame(std::slice::from_ref(&one), &buffer));
        assert!(first.contains("a=t,") && first.contains("a=p,"));
        assert!(gfx.frame(std::slice::from_ref(&one), &buffer).bytes.is_empty());

        let moved = Placement { area: Rect::new(5, 0, 10, 4), ..one.clone() };
        let second = text(&gfx.frame(&[moved], &buffer));
        assert!(second.contains("a=d,d=i") && second.contains("a=p,") && !second.contains("a=t,"));

        let gone = text(&gfx.frame(&[], &buffer));
        assert!(gone.contains("a=d,d=i") && !gone.contains("a=p,"));

        let exact = Placement::exact(picture(9), (3, 1), (4, 7), -2);
        let native = text(&gfx.frame(&[exact], &buffer));
        assert!(native.contains("\x1b[2;4H") && native.contains("X=4,Y=7,C=1,z=-2") && !native.contains(",c="));
    }

    #[test]
    fn inline_protocols_repaint_when_pictures_leave_and_redraw_when_covered() {
        let mut gfx = graphics(Protocol::Sixel);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 5));
        let one = Placement::fitted(picture(9), Rect::new(0, 0, 10, 4), 0);

        assert!(text(&gfx.frame(std::slice::from_ref(&one), &buffer)).contains("\x1bP"));
        assert!(gfx.frame(std::slice::from_ref(&one), &buffer).bytes.is_empty());

        buffer[(4, 1)].set_symbol("x");
        assert!(!gfx.frame(std::slice::from_ref(&one), &buffer).bytes.is_empty());

        let leaving = gfx.frame(&[], &buffer);
        assert!(leaving.repaint && leaving.bytes.is_empty());
    }

    #[test]
    fn cells_mode_emits_nothing() {
        let mut gfx = graphics(Protocol::Cells);
        let one = Placement::fitted(picture(1), Rect::new(0, 0, 4, 4), 0);
        let output = gfx.frame(&[one], &Buffer::empty(Rect::new(0, 0, 4, 4)));
        assert!(output.bytes.is_empty() && !output.repaint);
    }
}
