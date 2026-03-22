// SPDX-License-Identifier: BSD-3-Clause
//! Theme definition and color resolution.
//!
//! A [`Theme`] maps semantic [`Color`] variants to concrete RGB tuples and also
//! carries font-size hints so widgets can pick appropriate sizes without
//! hard-coding pixel values.

use crate::color::Color;
use embedded_graphics::pixelcolor::BinaryColor;

/// A complete UI theme.
///
/// All color fields are stored as `(R, G, B)` tuples.  The [`resolve`] method
/// converts a semantic [`Color`] into the concrete RGB triple for this theme.
///
/// [`resolve`]: Theme::resolve
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub primary: (u8, u8, u8),
    pub background: (u8, u8, u8),
    pub surface: (u8, u8, u8),
    pub text: (u8, u8, u8),
    pub text_secondary: (u8, u8, u8),
    pub accent: (u8, u8, u8),
    pub error: (u8, u8, u8),

    pub font_size_small: u32,
    pub font_size_normal: u32,
    pub font_size_large: u32,
}

impl Theme {
    /// Black-on-white theme suitable for e-paper / monochrome displays.
    pub const fn monochrome() -> Self {
        Self {
            primary: (0, 0, 0),
            background: (255, 255, 255),
            surface: (255, 255, 255),
            text: (0, 0, 0),
            text_secondary: (96, 96, 96),
            accent: (0, 0, 0),
            error: (0, 0, 0),
            font_size_small: 10,
            font_size_normal: 14,
            font_size_large: 20,
        }
    }

    /// Light-on-dark theme for OLED / LCD displays.
    pub const fn dark() -> Self {
        Self {
            primary: (187, 134, 252),   // soft purple
            background: (18, 18, 18),
            surface: (30, 30, 30),
            text: (230, 230, 230),
            text_secondary: (160, 160, 160),
            accent: (3, 218, 198),      // teal
            error: (207, 102, 121),     // muted red
            font_size_small: 10,
            font_size_normal: 14,
            font_size_large: 20,
        }
    }

    /// Resolve a semantic [`Color`] to a concrete `(R, G, B)` tuple using this
    /// theme's palette.
    pub const fn resolve(&self, color: Color) -> (u8, u8, u8) {
        match color {
            Color::Primary => self.primary,
            Color::Background => self.background,
            Color::Surface => self.surface,
            Color::Text => self.text,
            Color::TextSecondary => self.text_secondary,
            Color::Accent => self.accent,
            Color::Error => self.error,
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Black => (0, 0, 0),
            Color::White => (255, 255, 255),
        }
    }

    /// Convert an RGB triple to [`BinaryColor`] for e-paper rendering.
    ///
    /// Uses perceived-luminance weighting (ITU-R BT.601) with a 50 % threshold.
    pub const fn to_binary(r: u8, g: u8, b: u8) -> BinaryColor {
        // Luminance ≈ 0.299R + 0.587G + 0.114B  (scaled to avoid floats)
        let lum = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        if lum > 127 {
            BinaryColor::Off // white pixel (e-paper convention: Off = white)
        } else {
            BinaryColor::On // black pixel
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::monochrome()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_explicit_rgb() {
        let theme = Theme::monochrome();
        assert_eq!(theme.resolve(Color::Rgb(10, 20, 30)), (10, 20, 30));
    }

    #[test]
    fn to_binary_black_white() {
        assert_eq!(Theme::to_binary(0, 0, 0), BinaryColor::On);
        assert_eq!(Theme::to_binary(255, 255, 255), BinaryColor::Off);
    }

    #[test]
    fn dark_theme_text_is_light() {
        let dark = Theme::dark();
        let (r, g, b) = dark.text;
        // text on dark theme should be bright
        assert!(r > 200 && g > 200 && b > 200);
    }
}
