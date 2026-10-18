use std::f32::consts::TAU;

use ratatui::style::{Color, Style};

use crate::{Palette, Rgb, Theme, fx::Gradient, theme::Styles};

const BREATH_PERIOD: f32 = 3.2;
const BREATH_DEPTH: f32 = 0.35;
const STEADY: f32 = 1.0;

#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub theme: &'a Theme,
    pub palette: &'a Palette,
    pub t: f32,
}

impl<'a> Look<'a> {
    pub fn styles(&self) -> &'a Styles<Style> {
        &self.theme.styles
    }

    pub fn fg(&self, style: Style) -> Rgb {
        self.palette.foreground_of(style.fg.unwrap_or(Color::Reset))
    }

    pub fn bg(&self, style: Style) -> Rgb {
        self.palette.background_of(style.bg.unwrap_or(Color::Reset))
    }

    pub fn background(&self) -> Rgb {
        self.bg(self.styles().panel)
    }

    pub fn gradient(&self, [from, to]: [Color; 2]) -> Gradient {
        Gradient { from: self.palette.foreground_of(from), to: self.palette.foreground_of(to) }
    }

    pub fn accent(&self) -> Gradient {
        self.gradient(self.theme.effects.accent)
    }

    pub fn breath(&self) -> f32 {
        if !self.theme.effects.breathe {
            return STEADY;
        }
        STEADY - BREATH_DEPTH * (0.5 - 0.5 * (self.t * TAU / BREATH_PERIOD).cos())
    }
}
