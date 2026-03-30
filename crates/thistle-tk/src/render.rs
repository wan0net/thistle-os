// SPDX-License-Identifier: BSD-3-Clause
//! Renderer — walks the widget tree and draws to any `DrawTarget`.
//!
//! The renderer is generic over a [`ColorMapper`] trait that converts semantic
//! [`Color`]s into the display's native pixel format.  Two mappers are
//! provided:
//!
//! - [`MonoMapper`] — maps to [`BinaryColor`] (for e-paper)
//! - [`RgbMapper`] — maps to [`Rgb565`] (for colour LCD)
//!
//! The render function uses only `embedded-graphics` drawing primitives so it
//! stays fully `no_std` and platform-independent.

use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_7X14, FONT_10X20},
        MonoFont, MonoTextStyle,
    },
    pixelcolor::{BinaryColor, PixelColor, Rgb565},
    prelude::*,
    primitives::{Circle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, Line},
    text::Text,
};

use crate::color::Color;
use crate::theme::Theme;
use crate::tree::UiTree;
use crate::widget::{FontSize, Widget};

// ---------------------------------------------------------------------------
// ColorMapper trait + built-in mappers
// ---------------------------------------------------------------------------

/// Converts a semantic [`Color`] (resolved through a [`Theme`]) into a
/// display-native pixel colour.
pub trait ColorMapper {
    /// The pixel colour type of the target display.
    type TargetColor: PixelColor;

    /// Map a semantic colour to a concrete pixel colour.
    fn map(&self, color: Color, theme: &Theme) -> Self::TargetColor;
}

/// Maps semantic colours to [`BinaryColor`] for 1-bit e-paper displays.
pub struct MonoMapper;

impl ColorMapper for MonoMapper {
    type TargetColor = BinaryColor;

    fn map(&self, color: Color, theme: &Theme) -> BinaryColor {
        let (r, g, b) = theme.resolve(color);
        Theme::to_binary(r, g, b)
    }
}

/// Maps semantic colours to [`Rgb565`] for colour LCD displays.
pub struct RgbMapper;

impl ColorMapper for RgbMapper {
    type TargetColor = Rgb565;

    fn map(&self, color: Color, theme: &Theme) -> Rgb565 {
        let (r, g, b) = theme.resolve(color);
        Rgb565::new(r >> 3, g >> 2, b >> 3)
    }
}

// ---------------------------------------------------------------------------
// Public render entry point
// ---------------------------------------------------------------------------

/// Render the entire widget tree to the given `DrawTarget`.
///
/// # Type parameters
/// - `D` — the display / draw target.
/// - `M` — a [`ColorMapper`] whose `TargetColor` matches the display's colour.
pub fn render<D, M>(tree: &UiTree, theme: &Theme, mapper: &M, target: &mut D)
where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    render_node(tree, tree.root(), theme, mapper, target, 0);
}

// ---------------------------------------------------------------------------
// Common background + border rendering for all widgets
// ---------------------------------------------------------------------------

/// Draw the common background fill, pressed overlay, border, and focus ring
/// for any widget.  Called before widget-specific rendering.
fn draw_widget_bg_and_border<D, M>(
    widget: &Widget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
    scroll_y: i32,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = widget.common();
    let y = c.pos.y - scroll_y;
    let rect = Rectangle::new(
        Point::new(c.pos.x, y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );

    // Background fill (from CommonProps — canonical location).
    if let Some(bg) = c.bg_color {
        let bg_color = if c.pressed {
            // When pressed, use the theme's pressed color instead.
            let (r, g, b) = theme.pressed;
            mapper.map(Color::Rgb(r, g, b), theme)
        } else {
            mapper.map(bg, theme)
        };
        let style = PrimitiveStyleBuilder::new().fill_color(bg_color).build();
        if c.border_radius > 0 {
            let rounded = RoundedRectangle::with_equal_corners(
                rect,
                embedded_graphics::geometry::Size::new(
                    c.border_radius as u32,
                    c.border_radius as u32,
                ),
            );
            let _ = rounded.into_styled(style).draw(target);
        } else {
            let _ = rect.into_styled(style).draw(target);
        }
    } else if c.pressed {
        // No bg_color but pressed — draw the pressed overlay.
        let (r, g, b) = theme.pressed;
        let pressed_color = mapper.map(Color::Rgb(r, g, b), theme);
        let style = PrimitiveStyleBuilder::new().fill_color(pressed_color).build();
        if c.border_radius > 0 {
            let rounded = RoundedRectangle::with_equal_corners(
                rect,
                embedded_graphics::geometry::Size::new(
                    c.border_radius as u32,
                    c.border_radius as u32,
                ),
            );
            let _ = rounded.into_styled(style).draw(target);
        } else {
            let _ = rect.into_styled(style).draw(target);
        }
    }

    // Border
    if c.border_width > 0 {
        let border_color = mapper.map(c.border_color, theme);
        let style = PrimitiveStyleBuilder::new()
            .stroke_color(border_color)
            .stroke_width(c.border_width as u32)
            .build();
        if c.border_radius > 0 {
            let rounded = RoundedRectangle::with_equal_corners(
                rect,
                embedded_graphics::geometry::Size::new(
                    c.border_radius as u32,
                    c.border_radius as u32,
                ),
            );
            let _ = rounded.into_styled(style).draw(target);
        } else {
            let _ = rect.into_styled(style).draw(target);
        }
    }

    // Focus ring — draw an extra 1px border in the focus color.
    if c.focused {
        let (r, g, b) = theme.focus_border;
        let focus_color = mapper.map(Color::Rgb(r, g, b), theme);
        let style = PrimitiveStyleBuilder::new()
            .stroke_color(focus_color)
            .stroke_width(2)
            .build();
        if c.border_radius > 0 {
            let rounded = RoundedRectangle::with_equal_corners(
                rect,
                embedded_graphics::geometry::Size::new(
                    c.border_radius as u32,
                    c.border_radius as u32,
                ),
            );
            let _ = rounded.into_styled(style).draw(target);
        } else {
            let _ = rect.into_styled(style).draw(target);
        }
    }
}

// ---------------------------------------------------------------------------
// Recursive per-widget rendering
// ---------------------------------------------------------------------------

fn render_node<D, M>(
    tree: &UiTree,
    id: crate::widget::WidgetId,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
    scroll_y: i32,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let Some(widget) = tree.get(id) else {
        return;
    };
    if !widget.common().visible {
        return;
    }

    // Draw common background, borders, and focus ring for every widget.
    draw_widget_bg_and_border(widget, theme, mapper, target, scroll_y);

    match widget {
        Widget::Container(c) => {
            // Container-specific bg_color fallback (backward compat) — only
            // draw if CommonProps.bg_color was not already set.
            if c.common.bg_color.is_none() {
                if let Some(bg) = c.bg_color {
                    let color = mapper.map(bg, theme);
                    let rect = widget_rect_scrolled(widget, scroll_y);
                    let style = PrimitiveStyleBuilder::new().fill_color(color).build();
                    let _ = rect.into_styled(style).draw(target);
                }
            }
        }
        Widget::Label(l) => {
            draw_label(l, scroll_y, theme, mapper, target);
        }
        Widget::Button(b) => {
            draw_button(b, scroll_y, theme, mapper, target);
        }
        Widget::TextInput(t) => {
            draw_text_input(t, scroll_y, theme, mapper, target);
        }
        Widget::Image(img) => {
            draw_image(img, scroll_y, theme, mapper, target);
        }
        Widget::ListItem(li) => {
            draw_list_item(li, scroll_y, theme, mapper, target);
        }
        Widget::ProgressBar(pb) => {
            draw_progress_bar(pb, scroll_y, theme, mapper, target);
        }
        Widget::Divider(d) => {
            draw_divider(d, scroll_y, theme, mapper, target);
        }
        Widget::Spacer(_) => {
            // Spacers take up space but render nothing.
        }
        Widget::StatusBar(sb) => {
            draw_status_bar(sb, scroll_y, theme, mapper, target);
        }
        Widget::Switch(sw) => {
            draw_switch(sw, scroll_y, theme, mapper, target);
        }
        Widget::Checkbox(cb) => {
            draw_checkbox(cb, scroll_y, theme, mapper, target);
        }
    }

    // Compute the effective scroll offset for children of this node.
    let child_scroll = if let Some(Widget::Container(c)) = tree.get(id) {
        scroll_y + c.scroll_offset
    } else {
        scroll_y
    };

    // For scrollable containers, get bounds for culling.
    let clip_bounds = if child_scroll != scroll_y {
        // This container applies its own scroll — cull children outside it.
        tree.get(id).map(|w| {
            let c = w.common();
            (c.pos.y - scroll_y, c.pos.y - scroll_y + c.size.h as i32)
        })
    } else {
        None
    };

    // Render children in order (painter's algorithm — last child on top).
    for &child_id in tree.children(id) {
        // Basic culling: skip children entirely outside the scrollable container.
        if let Some((clip_top, clip_bottom)) = clip_bounds {
            if let Some(child_w) = tree.get(child_id) {
                let cc = child_w.common();
                let child_top = cc.pos.y - child_scroll;
                let child_bottom = child_top + cc.size.h as i32;
                if child_bottom <= clip_top || child_top >= clip_bottom {
                    continue;
                }
            }
        }
        render_node(tree, child_id, theme, mapper, target, child_scroll);
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

fn widget_rect_scrolled(widget: &Widget, scroll_y: i32) -> Rectangle {
    let c = widget.common();
    Rectangle::new(
        Point::new(c.pos.x, c.pos.y - scroll_y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    )
}

fn draw_label<D, M>(
    label: &crate::widget::LabelWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let color = mapper.map(label.color, theme);
    let font = font_for_size(&label.font_size);
    let style = MonoTextStyle::new(font, color);
    let char_w = font_char_width(&label.font_size);
    let line_h = font_height(&label.font_size);

    let x = label.common.pos.x;
    let y = label.common.pos.y - scroll_y;
    let max_w = label.common.size.w;

    if !label.word_wrap || max_w == 0 {
        // Single line — no wrapping.
        let ty = y + line_h as i32;
        let _ = Text::new(label.text.as_str(), Point::new(x, ty), style).draw(target);
        return;
    }

    // Word-wrap: split text into lines that fit within max_w.
    let chars_per_line = if char_w > 0 { (max_w / char_w).max(1) as usize } else { 40 };
    let max_lines = if label.max_lines == 0 { usize::MAX } else { label.max_lines as usize };

    let mut line_y = y + line_h as i32;
    let mut lines_drawn = 0usize;
    let mut remaining = label.text.as_str();

    while !remaining.is_empty() && lines_drawn < max_lines {
        if remaining.len() <= chars_per_line {
            // Fits on one line.
            let _ = Text::new(remaining, Point::new(x, line_y), style).draw(target);
            break;
        }

        // Find a break point.
        let slice = &remaining[..chars_per_line.min(remaining.len())];
        let break_at = if let Some(space_pos) = slice.rfind(' ') {
            space_pos + 1 // break after the space
        } else {
            chars_per_line.min(remaining.len()) // hard break
        };

        let line = remaining[..break_at].trim_end();
        let _ = Text::new(line, Point::new(x, line_y), style).draw(target);

        remaining = remaining[break_at..].trim_start();
        line_y += line_h as i32;
        lines_drawn += 1;
    }
}

fn draw_button<D, M>(
    button: &crate::widget::ButtonWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &button.common;
    // Use pressed color when the button is being pressed.
    let bg = if c.pressed {
        let (r, g, b) = theme.pressed;
        mapper.map(Color::Rgb(r, g, b), theme)
    } else {
        mapper.map(button.bg_color, theme)
    };
    let rect = Rectangle::new(
        Point::new(c.pos.x, c.pos.y - scroll_y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );

    let fill_style = PrimitiveStyleBuilder::new().fill_color(bg).build();

    if button.border_radius > 0 {
        let rounded = RoundedRectangle::with_equal_corners(
            rect,
            embedded_graphics::geometry::Size::new(
                button.border_radius as u32,
                button.border_radius as u32,
            ),
        );
        let _ = rounded.into_styled(fill_style).draw(target);
    } else {
        let _ = rect.into_styled(fill_style).draw(target);
    }

    // Center the text inside the button.
    let text_color = mapper.map(button.text_color, theme);
    let font = font_for_size(&FontSize::Normal);
    let text_style = MonoTextStyle::new(font, text_color);
    let text_w = text_width(button.text.as_str(), &FontSize::Normal) as i32;
    let text_h = font_height(&FontSize::Normal) as i32;
    let tx = c.pos.x + ((c.size.w as i32 - text_w) / 2).max(0);
    let ty = (c.pos.y - scroll_y) + ((c.size.h as i32 - text_h) / 2) + text_h; // baseline
    let _ = Text::new(button.text.as_str(), Point::new(tx, ty), text_style).draw(target);
}

fn draw_text_input<D, M>(
    input: &crate::widget::TextInputWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &input.common;
    let dy = c.pos.y - scroll_y;

    // Border rectangle — use focus color when focused.
    let border_color = if c.focused {
        let (r, g, b) = theme.focus_border;
        mapper.map(Color::Rgb(r, g, b), theme)
    } else {
        mapper.map(input.border_color, theme)
    };
    let stroke_w = if c.focused { 2 } else { 1 };
    let rect = Rectangle::new(
        Point::new(c.pos.x, dy),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let border_style = PrimitiveStyleBuilder::new()
        .stroke_color(border_color)
        .stroke_width(stroke_w)
        .build();
    let _ = rect.into_styled(border_style).draw(target);

    // Text (or placeholder).
    let text_color = mapper.map(input.text_color, theme);
    let display_text = if input.text.is_empty() {
        input.placeholder.as_str()
    } else {
        input.text.as_str()
    };

    let font = font_for_size(&FontSize::Normal);
    let text_style = MonoTextStyle::new(font, text_color);
    let fh = font_height(&FontSize::Normal) as i32;
    let tx = c.pos.x + 2;
    let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;
    let _ = Text::new(display_text, Point::new(tx, ty), text_style).draw(target);

    // Cursor line.
    if !input.text.is_empty() || input.cursor_pos == 0 {
        let cursor_x = tx + input.cursor_pos as i32 * font_char_width(&FontSize::Normal) as i32;
        let cursor_top = dy + 2;
        let cursor_bottom = dy + c.size.h as i32 - 2;
        let cursor_color = mapper.map(Color::Text, theme);
        let line_style = PrimitiveStyleBuilder::new()
            .stroke_color(cursor_color)
            .stroke_width(1)
            .build();
        let _ = Line::new(
            Point::new(cursor_x, cursor_top),
            Point::new(cursor_x, cursor_bottom),
        )
        .into_styled(line_style)
        .draw(target);
    }
}

fn draw_image<D, M>(
    image: &crate::widget::ImageWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    if image.data.is_null() || image.img_width == 0 || image.img_height == 0 {
        return;
    }

    let fg = mapper.map(image.fg_color, theme);
    let bg = mapper.map(image.bg_color, theme);
    let ox = image.common.pos.x;
    let oy = image.common.pos.y - scroll_y;

    for row in 0..image.img_height {
        for col in 0..image.img_width {
            let bit_index = row * image.img_width + col;
            let byte_index = (bit_index / 8) as usize;
            let bit_offset = 7 - (bit_index % 8); // MSB first

            // SAFETY: caller guarantees data pointer validity and sufficient length.
            let byte = unsafe { *image.data.add(byte_index) };
            let set = (byte >> bit_offset) & 1 != 0;

            let color = if set { fg } else { bg };
            let _ = Pixel(
                Point::new(ox + col as i32, oy + row as i32),
                color,
            )
            .draw(target);
        }
    }
}

fn draw_list_item<D, M>(
    li: &crate::widget::ListItemWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &li.common;
    let dy = c.pos.y - scroll_y;

    // Selected highlight background
    if li.selected {
        let bg = mapper.map(Color::Surface, theme);
        let rect = Rectangle::new(
            Point::new(c.pos.x, dy),
            embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
        );
        let style = PrimitiveStyleBuilder::new().fill_color(bg).build();
        let _ = rect.into_styled(style).draw(target);
    }

    // Title (Normal size)
    let title_color = mapper.map(li.title_color, theme);
    let title_font = font_for_size(&FontSize::Normal);
    let title_style = MonoTextStyle::new(title_font, title_color);
    let tx = c.pos.x + 4;
    let ty = dy + font_height(&FontSize::Normal) as i32;
    let _ = Text::new(li.title.as_str(), Point::new(tx, ty), title_style).draw(target);

    // Subtitle (below title, Small size)
    if !li.subtitle.is_empty() {
        let sub_color = mapper.map(li.subtitle_color, theme);
        let sub_font = font_for_size(&FontSize::Small);
        let sub_style = MonoTextStyle::new(sub_font, sub_color);
        let sub_y = ty + font_height(&FontSize::Small) as i32 + 2;
        let _ = Text::new(li.subtitle.as_str(), Point::new(tx, sub_y), sub_style).draw(target);
    }

    // Badge (right-aligned, Small size)
    if !li.badge.is_empty() {
        let badge_color = mapper.map(li.badge_color, theme);
        let badge_font = font_for_size(&FontSize::Small);
        let badge_style = MonoTextStyle::new(badge_font, badge_color);
        let badge_w = text_width(li.badge.as_str(), &FontSize::Small) as i32;
        let bx = c.pos.x + c.size.w as i32 - badge_w - 6;
        let _ = Text::new(li.badge.as_str(), Point::new(bx, ty), badge_style).draw(target);
    }
}

fn draw_progress_bar<D, M>(
    pb: &crate::widget::ProgressBarWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &pb.common;
    let dy = c.pos.y - scroll_y;

    // Track
    let track_color = mapper.map(pb.track_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, dy),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let track_style = PrimitiveStyleBuilder::new().fill_color(track_color).build();
    let _ = rect.into_styled(track_style).draw(target);

    // Filled portion
    let max = if pb.max_value == 0 { 100 } else { pb.max_value as u32 };
    let fill_w = (c.size.w * pb.value.min(pb.max_value) as u32) / max;
    if fill_w > 0 {
        let bar_color = mapper.map(pb.bar_color, theme);
        let fill_rect = Rectangle::new(
            Point::new(c.pos.x, dy),
            embedded_graphics::geometry::Size::new(fill_w, c.size.h),
        );
        let bar_style = PrimitiveStyleBuilder::new().fill_color(bar_color).build();
        let _ = fill_rect.into_styled(bar_style).draw(target);
    }
}

fn draw_divider<D, M>(
    d: &crate::widget::DividerWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &d.common;
    let dy = c.pos.y - scroll_y;
    let color = mapper.map(d.color, theme);
    let style = PrimitiveStyleBuilder::new().stroke_color(color).stroke_width(d.thickness as u32).build();
    let start = Point::new(c.pos.x, dy);
    let end = if matches!(d.direction, crate::layout::Direction::Row) {
        Point::new(c.pos.x + c.size.w as i32, dy)
    } else {
        Point::new(c.pos.x, dy + c.size.h as i32)
    };
    let _ = Line::new(start, end).into_styled(style).draw(target);
}

fn draw_status_bar<D, M>(
    sb: &crate::widget::StatusBarWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &sb.common;
    let dy = c.pos.y - scroll_y;

    // Background fill
    let bg = mapper.map(sb.bg_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, dy),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let bg_style = PrimitiveStyleBuilder::new().fill_color(bg).build();
    let _ = rect.into_styled(bg_style).draw(target);

    let text_color = mapper.map(sb.text_color, theme);
    let font = font_for_size(&FontSize::Small);
    let text_style = MonoTextStyle::new(font, text_color);
    let fh = font_height(&FontSize::Small) as i32;
    let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;

    // Left text
    if !sb.left_text.is_empty() {
        let _ = Text::new(sb.left_text.as_str(), Point::new(c.pos.x + 4, ty), text_style).draw(target);
    }

    // Center text
    if !sb.center_text.is_empty() {
        let tw = text_width(sb.center_text.as_str(), &FontSize::Small) as i32;
        let cx = c.pos.x + (c.size.w as i32 - tw) / 2;
        let _ = Text::new(sb.center_text.as_str(), Point::new(cx, ty), text_style).draw(target);
    }

    // Right text
    if !sb.right_text.is_empty() {
        let tw = text_width(sb.right_text.as_str(), &FontSize::Small) as i32;
        let rx = c.pos.x + c.size.w as i32 - tw - 4;
        let _ = Text::new(sb.right_text.as_str(), Point::new(rx, ty), text_style).draw(target);
    }
}

fn draw_switch<D, M>(
    sw: &crate::widget::SwitchWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &sw.common;
    let dy = c.pos.y - scroll_y;
    let w = c.size.w;
    let h = c.size.h;

    // Track — rounded rectangle.
    let track_color = if sw.on {
        mapper.map(sw.on_color, theme)
    } else {
        mapper.map(sw.off_color, theme)
    };
    let radius = h / 2;
    let rect = Rectangle::new(
        Point::new(c.pos.x, dy),
        embedded_graphics::geometry::Size::new(w, h),
    );
    let track_style = PrimitiveStyleBuilder::new().fill_color(track_color).build();
    let rounded = RoundedRectangle::with_equal_corners(
        rect,
        embedded_graphics::geometry::Size::new(radius, radius),
    );
    let _ = rounded.into_styled(track_style).draw(target);

    // Thumb — filled circle (white), inset 2px.
    let thumb_diameter = h.saturating_sub(4);
    let thumb_y = dy + 2;
    let thumb_x = if sw.on {
        c.pos.x + w as i32 - thumb_diameter as i32 - 2
    } else {
        c.pos.x + 2
    };
    let thumb_color = mapper.map(Color::White, theme);
    let thumb_style = PrimitiveStyleBuilder::new().fill_color(thumb_color).build();
    let _ = Circle::new(
        Point::new(thumb_x, thumb_y),
        thumb_diameter,
    )
    .into_styled(thumb_style)
    .draw(target);
}

fn draw_checkbox<D, M>(
    cb: &crate::widget::CheckboxWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &cb.common;
    let dy = c.pos.y - scroll_y;
    let box_size: u32 = 16;

    // Center the check box vertically within the widget height.
    let box_y = dy + ((c.size.h as i32 - box_size as i32) / 2).max(0);
    let box_rect = Rectangle::new(
        Point::new(c.pos.x, box_y),
        embedded_graphics::geometry::Size::new(box_size, box_size),
    );

    if cb.checked {
        // Filled square.
        let fill_color = mapper.map(Color::Primary, theme);
        let style = PrimitiveStyleBuilder::new().fill_color(fill_color).build();
        let _ = box_rect.into_styled(style).draw(target);

        // Draw a simple checkmark using two lines (white).
        let check_color = mapper.map(Color::White, theme);
        let line_style = PrimitiveStyleBuilder::new()
            .stroke_color(check_color)
            .stroke_width(2)
            .build();
        // Short descending stroke then long ascending stroke.
        let bx = c.pos.x;
        let _ = Line::new(
            Point::new(bx + 3, box_y + box_size as i32 / 2),
            Point::new(bx + 6, box_y + box_size as i32 - 4),
        )
        .into_styled(line_style)
        .draw(target);
        let _ = Line::new(
            Point::new(bx + 6, box_y + box_size as i32 - 4),
            Point::new(bx + box_size as i32 - 3, box_y + 3),
        )
        .into_styled(line_style)
        .draw(target);
    } else {
        // Border only.
        let border_color = mapper.map(Color::TextSecondary, theme);
        let style = PrimitiveStyleBuilder::new()
            .stroke_color(border_color)
            .stroke_width(1)
            .build();
        let _ = box_rect.into_styled(style).draw(target);
    }

    // Label to the right of the checkbox.
    if !cb.label.is_empty() {
        let text_color = mapper.map(Color::Text, theme);
        let font = font_for_size(&FontSize::Normal);
        let text_style = MonoTextStyle::new(font, text_color);
        let fh = font_height(&FontSize::Normal) as i32;
        let tx = c.pos.x + box_size as i32 + 6;
        let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;
        let _ = Text::new(cb.label.as_str(), Point::new(tx, ty), text_style).draw(target);
    }
}

fn font_height(size: &FontSize) -> u32 {
    match size {
        FontSize::Small => 10,   // FONT_6X10
        FontSize::Normal => 14,  // FONT_7X14
        FontSize::Large => 20,   // FONT_10X20
    }
}

fn font_for_size(size: &FontSize) -> &'static MonoFont<'static> {
    match size {
        FontSize::Small => &FONT_6X10,
        FontSize::Normal => &FONT_7X14,
        FontSize::Large => &FONT_10X20,
    }
}

fn font_char_width(size: &FontSize) -> u32 {
    match size {
        FontSize::Small => 6,    // FONT_6X10
        FontSize::Normal => 7,   // FONT_7X14
        FontSize::Large => 10,   // FONT_10X20
    }
}

fn text_width(text: &str, size: &FontSize) -> u32 {
    text.len() as u32 * font_char_width(size)
}

#[cfg(test)]
mod tests {
    use crate::tree::UiTree;
    use crate::widget::{
        ContainerWidget, LabelWidget, Pos, Size as WidgetSize, Widget,
    };
    use super::{render, MonoMapper};
    use crate::theme::Theme;
    use embedded_graphics::{
        mock_display::MockDisplay,
        pixelcolor::BinaryColor,
    };

    #[test]
    fn render_empty_tree() {
        let tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        let theme = Theme::monochrome();
        let mut display = MockDisplay::<BinaryColor>::new();
        render(&tree, &theme, &MonoMapper, &mut display);
        // Should not panic — the empty container has no bg so nothing is drawn.
    }

    #[test]
    fn render_label_does_not_panic() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        {
            let root = tree.get_mut(tree.root()).unwrap();
            let c = root.common_mut();
            c.size = WidgetSize { w: 64, h: 64 };
        }
        let child = tree.add_child(tree.root(), {
            let mut l = LabelWidget::default();
            let _ = l.text.push_str("Hi");
            Widget::Label(l)
        }).unwrap();
        {
            let w = tree.get_mut(child).unwrap();
            let c = w.common_mut();
            c.pos = Pos { x: 0, y: 0 };
            c.size = WidgetSize { w: 64, h: 16 };
        }

        let theme = Theme::monochrome();
        let mut display = MockDisplay::<BinaryColor>::new();
        display.set_allow_overdraw(true);
        render(&tree, &theme, &MonoMapper, &mut display);
    }
}
