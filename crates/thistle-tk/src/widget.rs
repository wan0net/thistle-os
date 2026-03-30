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
    /// Border width in pixels (0 = no border).
    pub border_width: u16,
    /// Border color (semantic).
    pub border_color: Color,
    /// Corner radius for rounded borders (0 = square corners).
    pub border_radius: u16,
    /// Background color. Canonical location — checked before widget-specific bg.
    pub bg_color: Option<Color>,
    /// `true` while the widget is being touched / pressed down.
    pub pressed: bool,
    /// `true` when the widget holds keyboard focus.
    pub focused: bool,
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
            border_width: 0,
            border_color: Color::TextSecondary,
            border_radius: 0,
            bg_color: None,
            pressed: false,
            focused: false,
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
// ListItem — contact rows, message previews, settings items
// ---------------------------------------------------------------------------

/// A list row with title, subtitle, optional badge, and press handler.
#[derive(Clone, Debug)]
pub struct ListItemWidget {
    pub common: CommonProps,
    pub title: HString<64>,
    pub subtitle: HString<128>,
    pub badge: HString<8>,
    pub title_color: Color,
    pub subtitle_color: Color,
    pub badge_color: Color,
    pub selected: bool,
    pub on_press: Option<OnPress>,
}

impl Default for ListItemWidget {
    fn default() -> Self {
        Self {
            common: CommonProps { height_hint: SizeHint::Fixed(40), ..CommonProps::default() },
            title: HString::new(),
            subtitle: HString::new(),
            badge: HString::new(),
            title_color: Color::Text,
            subtitle_color: Color::TextSecondary,
            badge_color: Color::Primary,
            selected: false,
            on_press: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ProgressBar — battery, signal, transfer progress
// ---------------------------------------------------------------------------

/// Horizontal progress bar with optional percentage text.
#[derive(Clone, Debug)]
pub struct ProgressBarWidget {
    pub common: CommonProps,
    pub value: u8,
    pub max_value: u8,
    pub bar_color: Color,
    pub track_color: Color,
    pub show_text: bool,
}

impl Default for ProgressBarWidget {
    fn default() -> Self {
        Self {
            common: CommonProps { height_hint: SizeHint::Fixed(8), ..CommonProps::default() },
            value: 0,
            max_value: 100,
            bar_color: Color::Primary,
            track_color: Color::Surface,
            show_text: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Divider — horizontal or vertical separator
// ---------------------------------------------------------------------------

/// A thin separator line.
#[derive(Clone, Debug)]
pub struct DividerWidget {
    pub common: CommonProps,
    pub color: Color,
    pub thickness: u16,
    pub direction: Direction,
}

impl Default for DividerWidget {
    fn default() -> Self {
        Self {
            common: CommonProps { height_hint: SizeHint::Fixed(1), ..CommonProps::default() },
            color: Color::Surface,
            thickness: 1,
            direction: Direction::Row, // horizontal
        }
    }
}

// ---------------------------------------------------------------------------
// Spacer — empty flex space
// ---------------------------------------------------------------------------

/// Takes up space in a flex layout without rendering anything.
#[derive(Clone, Debug, Default)]
pub struct SpacerWidget {
    pub common: CommonProps,
}

// ---------------------------------------------------------------------------
// StatusBar — fixed top bar with left/center/right text
// ---------------------------------------------------------------------------

/// Fixed-height bar with three text regions.
#[derive(Clone, Debug)]
pub struct StatusBarWidget {
    pub common: CommonProps,
    pub left_text: HString<32>,
    pub center_text: HString<32>,
    pub right_text: HString<32>,
    pub bg_color: Color,
    pub text_color: Color,
}

impl Default for StatusBarWidget {
    fn default() -> Self {
        Self {
            common: CommonProps { height_hint: SizeHint::Fixed(16), ..CommonProps::default() },
            left_text: HString::new(),
            center_text: HString::new(),
            right_text: HString::new(),
            bg_color: Color::Surface,
            text_color: Color::Text,
        }
    }
}

// ---------------------------------------------------------------------------
// Switch — toggle on/off
// ---------------------------------------------------------------------------

/// Toggle switch (on/off).
#[derive(Clone, Debug)]
pub struct SwitchWidget {
    pub common: CommonProps,
    pub on: bool,
    pub on_color: Color,
    pub off_color: Color,
    pub on_change: Option<OnChange>,
}

impl Default for SwitchWidget {
    fn default() -> Self {
        Self {
            common: CommonProps {
                width_hint: SizeHint::Fixed(44),
                height_hint: SizeHint::Fixed(24),
                ..CommonProps::default()
            },
            on: false,
            on_color: Color::Primary,
            off_color: Color::TextSecondary,
            on_change: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Checkbox — check/uncheck with label
// ---------------------------------------------------------------------------

/// Checkbox with an optional label to the right.
#[derive(Clone, Debug)]
pub struct CheckboxWidget {
    pub common: CommonProps,
    pub checked: bool,
    pub label: HString<64>,
    pub on_change: Option<OnChange>,
}

impl Default for CheckboxWidget {
    fn default() -> Self {
        Self {
            common: CommonProps {
                height_hint: SizeHint::Fixed(20),
                ..CommonProps::default()
            },
            checked: false,
            label: HString::new(),
            on_change: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Slider — horizontal slider (0-100 range)
// ---------------------------------------------------------------------------

/// Horizontal slider with a draggable thumb.
#[derive(Clone, Debug)]
pub struct SliderWidget {
    pub common: CommonProps,
    pub value: u8,
    pub min: u8,
    pub max: u8,
    pub track_color: Color,
    pub fill_color: Color,
    pub thumb_color: Color,
    pub on_change: Option<OnChange>,
}

impl Default for SliderWidget {
    fn default() -> Self {
        Self {
            common: CommonProps {
                width_hint: SizeHint::Fixed(120),
                height_hint: SizeHint::Fixed(24),
                ..CommonProps::default()
            },
            value: 50,
            min: 0,
            max: 100,
            track_color: Color::TextSecondary,
            fill_color: Color::Primary,
            thumb_color: Color::Primary,
            on_change: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Dropdown — select from a list of options
// ---------------------------------------------------------------------------

/// Dropdown select with a list of options.
#[derive(Clone, Debug)]
pub struct DropdownWidget {
    pub common: CommonProps,
    pub options: heapless::Vec<HString<32>, 16>,
    pub selected: u8,
    pub open: bool,
    pub bg_color: Color,
    pub text_color: Color,
    pub on_change: Option<OnChange>,
}

impl Default for DropdownWidget {
    fn default() -> Self {
        Self {
            common: CommonProps {
                height_hint: SizeHint::Fixed(28),
                ..CommonProps::default()
            },
            options: heapless::Vec::new(),
            selected: 0,
            open: false,
            bg_color: Color::Surface,
            text_color: Color::Text,
            on_change: None,
        }
    }
}

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
    ListItem(ListItemWidget),
    ProgressBar(ProgressBarWidget),
    Divider(DividerWidget),
    Spacer(SpacerWidget),
    StatusBar(StatusBarWidget),
    Switch(SwitchWidget),
    Checkbox(CheckboxWidget),
    Slider(SliderWidget),
    Dropdown(DropdownWidget),
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
            Widget::ListItem(w) => &w.common,
            Widget::ProgressBar(w) => &w.common,
            Widget::Divider(w) => &w.common,
            Widget::Spacer(w) => &w.common,
            Widget::StatusBar(w) => &w.common,
            Widget::Switch(w) => &w.common,
            Widget::Checkbox(w) => &w.common,
            Widget::Slider(w) => &w.common,
            Widget::Dropdown(w) => &w.common,
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
            Widget::ListItem(w) => &mut w.common,
            Widget::ProgressBar(w) => &mut w.common,
            Widget::Divider(w) => &mut w.common,
            Widget::Spacer(w) => &mut w.common,
            Widget::StatusBar(w) => &mut w.common,
            Widget::Switch(w) => &mut w.common,
            Widget::Checkbox(w) => &mut w.common,
            Widget::Slider(w) => &mut w.common,
            Widget::Dropdown(w) => &mut w.common,
        }
    }
}
