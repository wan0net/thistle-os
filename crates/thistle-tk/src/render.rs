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
    pixelcolor::{BinaryColor, PixelColor, Rgb565},
    prelude::*,
    primitives::{Circle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, Line},
};

use u8g2_fonts::{
    FontRenderer,
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
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

/// Render only the dirty region of the widget tree.
///
/// Returns the dirty rectangle that was rendered, or `None` if nothing was
/// dirty.  The caller (window manager) can use the returned rectangle to
/// perform a partial display flush — critical for e-paper where only the
/// changed region should be sent to the panel.
pub fn render_dirty<D, M>(
    tree: &mut UiTree,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) -> Option<Rectangle>
where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let dirty = tree.get_dirty_rect()?;

    // For now, render the full tree — the caller can use the dirty rect to
    // only flush that region to the display hardware.
    render(tree, theme, mapper, target);

    tree.clear_dirty_rect();
    Some(dirty)
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
        Widget::Slider(sl) => {
            draw_slider(sl, scroll_y, theme, mapper, target);
        }
        Widget::Dropdown(dd) => {
            draw_dropdown(dd, scroll_y, theme, mapper, target);
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

    // Draw scrollbar for scrollable containers after children are rendered.
    if let Some(Widget::Container(container)) = tree.get(id) {
        let container_y = container.common.pos.y;
        let content_height = tree
            .children(id)
            .iter()
            .filter_map(|&child_id| {
                tree.get(child_id).map(|w| {
                    let c = w.common();
                    (c.pos.y - container_y) + c.size.h as i32
                })
            })
            .max()
            .unwrap_or(0);
        draw_scrollbar(container, content_height, theme, mapper, target, scroll_y);
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
    let line_h = font_height(&label.font_size);

    let x = label.common.pos.x;
    let y = label.common.pos.y - scroll_y;
    let max_w = label.common.size.w;

    if !label.word_wrap || max_w == 0 {
        // Single line — no wrapping.
        let ty = y + line_h as i32;
        draw_text_u8g2(label.text.as_str(), x, ty, &label.font_size, color, target);
        return;
    }

    // Proportional word-wrap: measure words incrementally.
    let max_lines = if label.max_lines == 0 { usize::MAX } else { label.max_lines as usize };
    let mut line_y = y + line_h as i32;
    let mut lines_drawn = 0usize;

    // Build lines word-by-word, measuring with proportional metrics.
    let mut line_start = 0usize; // byte offset into text where current line starts
    let text = label.text.as_str();
    let words = text.split_whitespace();
    // We track byte ranges into the original text to avoid alloc.
    let mut line_end = 0usize;
    let mut first_word_on_line = true;

    for word in words {
        // Compute the byte offset of this word in the original text.
        let word_start = word.as_ptr() as usize - text.as_ptr() as usize;
        let word_end = word_start + word.len();

        let test_end = word_end;
        let test_str = &text[line_start..test_end];
        let w = text_width_u8g2(test_str, &label.font_size);

        if w > max_w && !first_word_on_line {
            // Flush current line (up to line_end).
            let line = text[line_start..line_end].trim_end();
            draw_text_u8g2(line, x, line_y, &label.font_size, color, target);
            line_y += line_h as i32;
            lines_drawn += 1;
            if lines_drawn >= max_lines {
                return;
            }
            line_start = word_start;
            line_end = word_end;
            first_word_on_line = true;
        } else {
            line_end = word_end;
            first_word_on_line = false;
        }
    }

    // Flush remaining text.
    if line_start < text.len() && lines_drawn < max_lines {
        let line = text[line_start..line_end].trim_end();
        if !line.is_empty() {
            draw_text_u8g2(line, x, line_y, &label.font_size, color, target);
        }
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
    let text_w = text_width_u8g2(button.text.as_str(), &FontSize::Normal) as i32;
    let text_h = font_height(&FontSize::Normal) as i32;
    let tx = c.pos.x + ((c.size.w as i32 - text_w) / 2).max(0);
    let ty = (c.pos.y - scroll_y) + ((c.size.h as i32 - text_h) / 2) + text_h; // baseline
    draw_text_u8g2(button.text.as_str(), tx, ty, &FontSize::Normal, text_color, target);
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

    let fh = font_height(&FontSize::Normal) as i32;
    let tx = c.pos.x + 2;
    let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;
    draw_text_u8g2(display_text, tx, ty, &FontSize::Normal, text_color, target);

    // Cursor line — compute position from proportional text width.
    if !input.text.is_empty() || input.cursor_pos == 0 {
        let text_before_cursor = &input.text.as_str()[..input.cursor_pos as usize];
        let cursor_x = tx + text_width_u8g2(text_before_cursor, &FontSize::Normal) as i32;
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
    let tx = c.pos.x + 4;
    let ty = dy + font_height(&FontSize::Normal) as i32;
    draw_text_u8g2(li.title.as_str(), tx, ty, &FontSize::Normal, title_color, target);

    // Subtitle (below title, Small size)
    if !li.subtitle.is_empty() {
        let sub_color = mapper.map(li.subtitle_color, theme);
        let sub_y = ty + font_height(&FontSize::Small) as i32 + 2;
        draw_text_u8g2(li.subtitle.as_str(), tx, sub_y, &FontSize::Small, sub_color, target);
    }

    // Badge (right-aligned, Small size)
    if !li.badge.is_empty() {
        let badge_color = mapper.map(li.badge_color, theme);
        let badge_w = text_width_u8g2(li.badge.as_str(), &FontSize::Small) as i32;
        let bx = c.pos.x + c.size.w as i32 - badge_w - 6;
        draw_text_u8g2(li.badge.as_str(), bx, ty, &FontSize::Small, badge_color, target);
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
    let fh = font_height(&FontSize::Small) as i32;
    let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;

    // Left text
    if !sb.left_text.is_empty() {
        draw_text_u8g2(sb.left_text.as_str(), c.pos.x + 4, ty, &FontSize::Small, text_color, target);
    }

    // Center text
    if !sb.center_text.is_empty() {
        let tw = text_width_u8g2(sb.center_text.as_str(), &FontSize::Small) as i32;
        let cx = c.pos.x + (c.size.w as i32 - tw) / 2;
        draw_text_u8g2(sb.center_text.as_str(), cx, ty, &FontSize::Small, text_color, target);
    }

    // Right text
    if !sb.right_text.is_empty() {
        let tw = text_width_u8g2(sb.right_text.as_str(), &FontSize::Small) as i32;
        let rx = c.pos.x + c.size.w as i32 - tw - 4;
        draw_text_u8g2(sb.right_text.as_str(), rx, ty, &FontSize::Small, text_color, target);
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
        let fh = font_height(&FontSize::Normal) as i32;
        let tx = c.pos.x + box_size as i32 + 6;
        let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;
        draw_text_u8g2(cb.label.as_str(), tx, ty, &FontSize::Normal, text_color, target);
    }
}

fn draw_scrollbar<D, M>(
    container: &crate::widget::ContainerWidget,
    content_height: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
    scroll_y: i32,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &container.common;
    let container_h = c.size.h as i32;
    if content_height <= container_h {
        return; // no overflow, no scrollbar
    }

    let track_x = c.pos.x + c.size.w as i32 - 4;
    let track_y = c.pos.y - scroll_y;
    let track_h = container_h;

    // Thumb proportional to visible portion.
    let thumb_h = ((container_h as f32 / content_height as f32) * track_h as f32) as i32;
    let thumb_h = thumb_h.max(10); // minimum 10px
    let scroll_range = content_height - container_h;
    let thumb_y = if scroll_range > 0 {
        track_y
            + ((container.scroll_offset as f32 / scroll_range as f32)
                * (track_h - thumb_h) as f32) as i32
    } else {
        track_y
    };

    // Draw thumb (filled rectangle).
    let track_color = mapper.map(Color::TextSecondary, theme);
    let thumb_rect = Rectangle::new(
        Point::new(track_x, thumb_y),
        embedded_graphics::geometry::Size::new(3, thumb_h as u32),
    );
    let style = PrimitiveStyleBuilder::new().fill_color(track_color).build();
    let _ = thumb_rect.into_styled(style).draw(target);
}

fn draw_slider<D, M>(
    sl: &crate::widget::SliderWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &sl.common;
    let dy = c.pos.y - scroll_y;
    let w = c.size.w;
    let h = c.size.h;

    // Track — horizontal rounded rectangle (full width, thin).
    let track_h: u32 = 4;
    let track_y = dy + (h as i32 - track_h as i32) / 2;
    let track_color = mapper.map(sl.track_color, theme);
    let track_rect = Rectangle::new(
        Point::new(c.pos.x, track_y),
        embedded_graphics::geometry::Size::new(w, track_h),
    );
    let track_style = PrimitiveStyleBuilder::new().fill_color(track_color).build();
    let rounded_track = RoundedRectangle::with_equal_corners(
        track_rect,
        embedded_graphics::geometry::Size::new(2, 2),
    );
    let _ = rounded_track.into_styled(track_style).draw(target);

    // Filled portion from left.
    let range = (sl.max - sl.min).max(1) as u32;
    let val = sl.value.clamp(sl.min, sl.max) - sl.min;
    let fill_w = (w * val as u32) / range;
    if fill_w > 0 {
        let fill_color = mapper.map(sl.fill_color, theme);
        let fill_rect = Rectangle::new(
            Point::new(c.pos.x, track_y),
            embedded_graphics::geometry::Size::new(fill_w, track_h),
        );
        let fill_style = PrimitiveStyleBuilder::new().fill_color(fill_color).build();
        let rounded_fill = RoundedRectangle::with_equal_corners(
            fill_rect,
            embedded_graphics::geometry::Size::new(2, 2),
        );
        let _ = rounded_fill.into_styled(fill_style).draw(target);
    }

    // Thumb — circle at current value position.
    let thumb_diameter: u32 = h.min(16);
    let thumb_x = c.pos.x + fill_w as i32 - (thumb_diameter as i32 / 2);
    let thumb_y = dy + (h as i32 - thumb_diameter as i32) / 2;
    let thumb_color = mapper.map(sl.thumb_color, theme);
    let thumb_style = PrimitiveStyleBuilder::new().fill_color(thumb_color).build();
    let _ = Circle::new(
        Point::new(thumb_x, thumb_y),
        thumb_diameter,
    )
    .into_styled(thumb_style)
    .draw(target);
}

fn draw_dropdown<D, M>(
    dd: &crate::widget::DropdownWidget,
    scroll_y: i32,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &dd.common;
    let dy = c.pos.y - scroll_y;

    // Closed state: rectangle with selected text + down arrow.
    let bg = mapper.map(dd.bg_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, dy),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let bg_style = PrimitiveStyleBuilder::new().fill_color(bg).build();
    let _ = rect.into_styled(bg_style).draw(target);

    // Border.
    let border_color = mapper.map(Color::TextSecondary, theme);
    let border_style = PrimitiveStyleBuilder::new()
        .stroke_color(border_color)
        .stroke_width(1)
        .build();
    let _ = rect.into_styled(border_style).draw(target);

    // Selected option text.
    let text_color = mapper.map(dd.text_color, theme);
    let fh = font_height(&FontSize::Normal) as i32;
    let tx = c.pos.x + 4;
    let ty = dy + ((c.size.h as i32 - fh) / 2) + fh;

    let selected_text = dd
        .options
        .get(dd.selected as usize)
        .map(|s| s.as_str())
        .unwrap_or("");
    draw_text_u8g2(selected_text, tx, ty, &FontSize::Normal, text_color, target);

    // Down arrow indicator on the right.
    let arrow_x = c.pos.x + c.size.w as i32 - 14;
    draw_text_u8g2("v", arrow_x, ty, &FontSize::Normal, text_color, target);

    // Open state: draw option list below.
    if dd.open {
        let item_h = c.size.h;
        let list_y = dy + c.size.h as i32;
        let option_count = dd.options.len() as u32;
        let list_h = item_h * option_count;

        // List background.
        let list_rect = Rectangle::new(
            Point::new(c.pos.x, list_y),
            embedded_graphics::geometry::Size::new(c.size.w, list_h),
        );
        let _ = list_rect.into_styled(bg_style).draw(target);
        let _ = list_rect.into_styled(border_style).draw(target);

        for (i, option) in dd.options.iter().enumerate() {
            let oy = list_y + (i as i32 * item_h as i32);

            // Highlight selected option.
            if i == dd.selected as usize {
                let highlight_color = mapper.map(Color::Primary, theme);
                let highlight_rect = Rectangle::new(
                    Point::new(c.pos.x, oy),
                    embedded_graphics::geometry::Size::new(c.size.w, item_h),
                );
                let highlight_style = PrimitiveStyleBuilder::new()
                    .fill_color(highlight_color)
                    .build();
                let _ = highlight_rect.into_styled(highlight_style).draw(target);
            }

            let oty = oy + ((item_h as i32 - fh) / 2) + fh;
            draw_text_u8g2(option.as_str(), tx, oty, &FontSize::Normal, text_color, target);
        }
    }
}

fn font_height(size: &FontSize) -> u32 {
    match size {
        FontSize::Small => 12,   // helvR10 actual height
        FontSize::Normal => 16,  // helvR14 actual height
        FontSize::Large => 21,   // helvR18 actual height
    }
}

/// Render text using u8g2 proportional fonts.
fn draw_text_u8g2<D: DrawTarget>(
    text: &str,
    x: i32,
    y: i32,
    size: &FontSize,
    color: D::Color,
    target: &mut D,
) {
    let fc = FontColor::Transparent(color);
    let pos = Point::new(x, y);
    match size {
        FontSize::Small => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR10_tr>();
            let _ = font.render_aligned(
                text, pos, VerticalPosition::Baseline, HorizontalAlignment::Left, fc, target,
            );
        }
        FontSize::Normal => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR14_tr>();
            let _ = font.render_aligned(
                text, pos, VerticalPosition::Baseline, HorizontalAlignment::Left, fc, target,
            );
        }
        FontSize::Large => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR18_tr>();
            let _ = font.render_aligned(
                text, pos, VerticalPosition::Baseline, HorizontalAlignment::Left, fc, target,
            );
        }
    }
}

/// Measure the rendered width of text using u8g2 proportional fonts.
fn text_width_u8g2(text: &str, size: &FontSize) -> u32 {
    let fallback = || text.len() as u32 * match size {
        FontSize::Small => 6,
        FontSize::Normal => 7,
        FontSize::Large => 10,
    };
    let dims = match size {
        FontSize::Small => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR10_tr>();
            font.get_rendered_dimensions(text, Point::zero(), VerticalPosition::Baseline)
        }
        FontSize::Normal => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR14_tr>();
            font.get_rendered_dimensions(text, Point::zero(), VerticalPosition::Baseline)
        }
        FontSize::Large => {
            let font = FontRenderer::new::<fonts::u8g2_font_helvR18_tr>();
            font.get_rendered_dimensions(text, Point::zero(), VerticalPosition::Baseline)
        }
    };
    dims.ok()
        .and_then(|d| d.bounding_box)
        .map(|bb| bb.size.width)
        .unwrap_or_else(fallback)
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
