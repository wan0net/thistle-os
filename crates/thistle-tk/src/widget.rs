// SPDX-License-Identifier: BSD-3-Clause
//! Widget types for the thistle-tk toolkit.
//!
//! Every visible element in the UI is a [`Widget`]. Widgets are value types
//! stored in a flat arena ([`UiTree`](crate::tree::UiTree)). The tree owns the
//! widgets; apps manipulate them via [`WidgetId`] handles.

use crate::color::Color;
use crate::layout::{Align, Direction};
use heapless::String as HString;

// ---------------------------------------------------------------------------
// WidgetId
// ---------------------------------------------------------------------------

/// Handle into the widget tree.  Zero is reserved for the root.
pub type WidgetId = u16;

// ---------------------------------------------------------------------------
// Common geometry
// ---------------------------------------------------------------------------

/// Position within the parent coordinate space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

/// Size in pixels.  `0` means "auto / fill parent".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

/// Sizing hint used by the layout engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeHint {
    /// Fixed pixel size.
    Fixed(u32),
    /// Percentage of parent (0.0 .. 1.0).
    Percent(f32),
    /// Flex-grow weight (like CSS `flex-grow`).
    Flex(f32),
    /// Size to content.
    Auto,
}

impl Default for SizeHint {
    fn default() -> Self {
        Self::Auto
    }
}

// ---------------------------------------------------------------------------
// Common props shared by every widget
// ---------------------------------------------------------------------------

/// Properties shared by all widget variants.
#[derive(Clone, Debug)]
pub struct CommonProps {
    pub id: WidgetId,
    /// Computed position — written by the layout engine.
    pub pos: Pos,
    /// Computed size — written by the layout engine.
    pub size: Size,
    /// Size hints consumed by the layout engine.
    pub width_hint: SizeHint,
    pub height_hint: SizeHint,
    /// Padding inside the widget boundary (left, top, right, bottom).
    pub padding: (u16, u16, u16, u16),
    pub visible: bool,
    pub dirty: bool,
}

impl Default for CommonProps {
    fn default() -> Self {
        Self {
            id: 0,
            pos: Pos::default(),
            size: Size::default(),
            width_hint: SizeHint::Auto,
            height_hint: SizeHint::Auto,
            padding: (0, 0, 0, 0),
            visible: true,
            dirty: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Font size hint
// ---------------------------------------------------------------------------

/// Semantic font size — resolved by the theme at render time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontSize {
    Small,
    #[default]
    Normal,
    Large,
}

// ---------------------------------------------------------------------------
// Callback wrappers (fn pointers so widgets stay Send)
// ---------------------------------------------------------------------------

/// Press callback: receives the widget id that was pressed.
pub type OnPress = fn(WidgetId);

/// Text-change callback: receives widget id and the new text.
pub type OnChange = fn(WidgetId, &str);

// ---------------------------------------------------------------------------
// Widget variants
// ---------------------------------------------------------------------------

/// Layout container — arranges children in a row or column.
#[derive(Clone, Debug)]
pub struct ContainerWidget {
    pub common: CommonProps,
    pub direction: Direction,
    pub gap: u16,
    pub align: Align,
    pub cross_align: Align,
    pub scroll_offset: i32,
    pub bg_color: Option<Color>,
}

impl Default for ContainerWidget {
    fn default() -> Self {
        Self {
            common: CommonProps::default(),
            direction: Direction::Column,
            gap: 0,
            align: Align::Start,
            cross_align: Align::Start,
            scroll_offset: 0,
            bg_color: None,
        }
    }
}

/// Single- or multi-line text label.
#[derive(Clone, Debug)]
pub struct LabelWidget {
    pub common: CommonProps,
    pub text: HString<256>,
    pub color: Color,
    pub font_size: FontSize,
    pub max_lines: u16,
    pub word_wrap: bool,
}

impl Default for LabelWidget {
    fn default() -> Self {
        Self {
            common: CommonProps::default(),
            text: HString::new(),
            color: Color::Text,
            font_size: FontSize::Normal,
            max_lines: 0, // 0 = unlimited
            word_wrap: true,
        }
    }
}

/// Pressable button with text label.
#[derive(Clone, Debug)]
pub struct ButtonWidget {
    pub common: CommonProps,
    pub text: HString<64>,
    pub on_press: Option<OnPress>,
    pub bg_color: Color,
    pub text_color: Color,
    pub border_radius: u16,
}

impl Default for ButtonWidget {
    fn default() -> Self {
        Self {
            common: CommonProps::default(),
            text: HString::new(),
            on_press: None,
            bg_color: Color::Primary,
            text_color: Color::Background,
            border_radius: 4,
        }
    }
}

/// Editable single-line text field.
#[derive(Clone, Debug)]
pub struct TextInputWidget {
    pub common: CommonProps,
    pub text: HString<256>,
    pub placeholder: HString<64>,
    pub cursor_pos: u16,
    pub password_mode: bool,
    pub on_change: Option<OnChange>,
    pub border_color: Color,
    pub text_color: Color,
}

impl Default for TextInputWidget {
    fn default() -> Self {
        Self {
            common: CommonProps::default(),
            text: HString::new(),
            placeholder: HString::new(),
            cursor_pos: 0,
            password_mode: false,
            on_change: None,
            border_color: Color::TextSecondary,
            text_color: Color::Text,
        }
    }
}

/// 1-bit packed bitmap image.
#[derive(Clone, Debug)]
pub struct ImageWidget {
    pub common: CommonProps,
    /// Image width in pixels.
    pub img_width: u32,
    /// Image height in pixels.
    pub img_height: u32,
    /// Pointer to 1-bit packed pixel data (row-major, MSB first).
    /// Length must be at least `ceil(img_width * img_height / 8)` bytes.
    ///
    /// Safety: the caller must ensure the pointer remains valid for the
    /// lifetime of the widget.
    pub data: *const u8,
    /// Foreground color for set bits.
    pub fg_color: Color,
    /// Background color for clear bits.
    pub bg_color: Color,
}

impl Default for ImageWidget {
    fn default() -> Self {
        Self {
            common: CommonProps::default(),
            img_width: 0,
            img_height: 0,
            data: core::ptr::null(),
            fg_color: Color::Text,
            bg_color: Color::Background,
        }
    }
}

// SAFETY: The *const u8 in ImageWidget is only read during rendering (single
// task) and the pointer is provided by the app which guarantees its lifetime.
unsafe impl Send for ImageWidget {}

// ---------------------------------------------------------------------------
// Widget enum
// ---------------------------------------------------------------------------

/// A widget in the UI tree.
#[derive(Clone, Debug)]
pub enum Widget {
    Container(ContainerWidget),
    Label(LabelWidget),
    Button(ButtonWidget),
    TextInput(TextInputWidget),
    Image(ImageWidget),
}

impl Widget {
    /// Get a shared reference to the common properties.
    pub fn common(&self) -> &CommonProps {
        match self {
            Widget::Container(w) => &w.common,
            Widget::Label(w) => &w.common,
            Widget::Button(w) => &w.common,
            Widget::TextInput(w) => &w.common,
            Widget::Image(w) => &w.common,
        }
    }

    /// Get a mutable reference to the common properties.
    pub fn common_mut(&mut self) -> &mut CommonProps {
        match self {
            Widget::Container(w) => &mut w.common,
            Widget::Label(w) => &mut w.common,
            Widget::Button(w) => &mut w.common,
            Widget::TextInput(w) => &mut w.common,
            Widget::Image(w) => &mut w.common,
        }
    }
}
