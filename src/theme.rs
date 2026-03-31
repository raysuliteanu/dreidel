// SPDX-License-Identifier: GPL-3.0-only

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
            dim: Rgb(220, 225, 240),
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
            dim: Rgb(30, 30, 30),
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
