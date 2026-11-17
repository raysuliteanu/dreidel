// SPDX-License-Identifier: GPL-3.0-only

//! Color theme definitions.
//!
//! [`Theme`] selects auto/light/dark; [`ColorPalette`] provides named semantic
//! color roles (`fg`, `bg`, `border`, `accent`, `warn`, `critical`, `dim`,
//! `highlight`) used by all components.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct ColorPalette {
    // bg is part of the palette API even if not all renderers use it yet
    #[allow(dead_code)]
    pub bg: ratatui::style::Color,
    pub fg: ratatui::style::Color,
    pub border: ratatui::style::Color,
    pub accent: ratatui::style::Color,
    pub warn: ratatui::style::Color,
    pub critical: ratatui::style::Color,
    pub dim: ratatui::style::Color,
    pub highlight: ratatui::style::Color,
}

impl ColorPalette {
    pub fn dark() -> Self {
        use ratatui::style::Color::*;
        Self {
            bg: Rgb(26, 27, 38),
            fg: Rgb(192, 202, 245),
            border: Rgb(65, 72, 104),
            accent: Rgb(122, 162, 247),
            warn: Rgb(224, 175, 104),
            critical: Rgb(247, 118, 142),
            dim: Rgb(86, 95, 137),
            highlight: Rgb(187, 154, 247),
        }
    }

    pub fn light() -> Self {
        use ratatui::style::Color::*;
        Self {
            bg: White,
            fg: Rgb(20, 20, 20),
            border: Rgb(180, 180, 180),
            accent: Rgb(0, 100, 200),
            warn: Rgb(180, 100, 0),
            critical: Rgb(180, 0, 0),
            dim: Rgb(120, 120, 120),
            highlight: Rgb(80, 0, 180),
        }
    }
}

impl Theme {
    pub fn palette(self) -> ColorPalette {
        match self {
            Theme::Dark | Theme::Auto => ColorPalette::dark(),
            Theme::Light => ColorPalette::light(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn luminance(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else {
            panic!("expected Rgb color, got {c:?}");
        };
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    #[test]
    fn dark_dim_is_darker_than_fg() {
        let p = ColorPalette::dark();
        assert!(luminance(p.dim) < luminance(p.fg));
        assert!(luminance(p.dim) > luminance(p.bg));
    }

    #[test]
    fn light_dim_is_lighter_than_fg() {
        let p = ColorPalette::light();
        assert!(luminance(p.dim) > luminance(p.fg));
        assert!(luminance(p.dim) < luminance(Color::Rgb(255, 255, 255)));
    }
}
