// SPDX-License-Identifier: BSD-3-Clause
//! Display-agnostic color system.
//!
//! Apps use semantic [`Color`] values that get resolved by the active theme at
//! render time. This lets the same widget tree render correctly on both 1-bit
//! e-paper (BinaryColor) and full-color LCD (Rgb565) displays.

/// Semantic color that gets resolved by the theme at render time.
/// Apps use these — never raw pixel values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    // -- Theme colors (resolved at render time) --
    /// Primary brand / UI chrome color.
    Primary,
    /// Page / screen background.
    Background,
    /// Card / surface background (slightly different from page bg).
    Surface,
    /// Primary text color.
    Text,
    /// Secondary / muted text color.
    TextSecondary,
    /// Accent / highlight color.
    Accent,
    /// Error / destructive action color.
    Error,

    // -- Explicit colors --
    /// Explicit RGB value. On e-paper the renderer thresholds to B/W.
    Rgb(u8, u8, u8),
    /// Explicit black — works on every display type.
    Black,
    /// Explicit white — works on every display type.
    White,
}

impl Color {
    /// Convenience: create an [`Color::Rgb`] from a 24-bit hex value.
    ///
    /// ```
    /// # use thistle_tk::color::Color;
    /// let teal = Color::from_hex(0x00BCD4);
    /// assert_eq!(teal, Color::Rgb(0x00, 0xBC, 0xD4));
    /// ```
    pub const fn from_hex(hex: u32) -> Self {
        let r = ((hex >> 16) & 0xFF) as u8;
        let g = ((hex >> 8) & 0xFF) as u8;
        let b = (hex & 0xFF) as u8;
        Self::Rgb(r, g, b)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::Text
    }
}
