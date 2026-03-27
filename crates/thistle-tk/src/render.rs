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
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{BinaryColor, PixelColor, Rgb565},
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle, Line},
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
    render_node(tree, tree.root(), theme, mapper, target);
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

    match widget {
        Widget::Container(c) => {
            // Optionally fill the container background.
            if let Some(bg) = c.bg_color {
                let color = mapper.map(bg, theme);
                let rect = widget_rect(widget);
                let style = PrimitiveStyleBuilder::new().fill_color(color).build();
                let _ = rect.into_styled(style).draw(target);
            }
        }
        Widget::Label(l) => {
            draw_label(l, theme, mapper, target);
        }
        Widget::Button(b) => {
            draw_button(b, theme, mapper, target);
        }
        Widget::TextInput(t) => {
            draw_text_input(t, theme, mapper, target);
        }
        Widget::Image(img) => {
            draw_image(img, theme, mapper, target);
        }
        Widget::ListItem(li) => {
            draw_list_item(li, theme, mapper, target);
        }
        Widget::ProgressBar(pb) => {
            draw_progress_bar(pb, theme, mapper, target);
        }
        Widget::Divider(d) => {
            draw_divider(d, theme, mapper, target);
        }
        Widget::Spacer(_) => {
            // Spacers take up space but render nothing.
        }
        Widget::StatusBar(sb) => {
            draw_status_bar(sb, theme, mapper, target);
        }
    }

    // Render children in order (painter's algorithm — last child on top).
    for &child_id in tree.children(id) {
        render_node(tree, child_id, theme, mapper, target);
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

fn widget_rect(widget: &Widget) -> Rectangle {
    let c = widget.common();
    Rectangle::new(
        Point::new(c.pos.x, c.pos.y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    )
}

fn draw_label<D, M>(
    label: &crate::widget::LabelWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let color = mapper.map(label.color, theme);
    let style = MonoTextStyle::new(&FONT_6X10, color);
    let _ = label.font_size; // TODO: select font based on size

    let x = label.common.pos.x;
    // embedded-graphics Text baseline is at y, so offset by font height.
    let y = label.common.pos.y + font_height(&label.font_size) as i32;

    let _ = Text::new(label.text.as_str(), Point::new(x, y), style).draw(target);
}

fn draw_button<D, M>(
    button: &crate::widget::ButtonWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &button.common;
    let bg = mapper.map(button.bg_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, c.pos.y),
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
    let text_style = MonoTextStyle::new(&FONT_6X10, text_color);
    let text_w = button.text.len() as i32 * 6; // 6px per char for FONT_6X10
    let text_h = 10i32;
    let tx = c.pos.x + ((c.size.w as i32 - text_w) / 2).max(0);
    let ty = c.pos.y + ((c.size.h as i32 - text_h) / 2) + text_h; // baseline
    let _ = Text::new(button.text.as_str(), Point::new(tx, ty), text_style).draw(target);
}

fn draw_text_input<D, M>(
    input: &crate::widget::TextInputWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &input.common;

    // Border rectangle.
    let border_color = mapper.map(input.border_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, c.pos.y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let border_style = PrimitiveStyleBuilder::new()
        .stroke_color(border_color)
        .stroke_width(1)
        .build();
    let _ = rect.into_styled(border_style).draw(target);

    // Text (or placeholder).
    let text_color = mapper.map(input.text_color, theme);
    let display_text = if input.text.is_empty() {
        input.placeholder.as_str()
    } else {
        input.text.as_str()
    };

    let text_style = MonoTextStyle::new(&FONT_6X10, text_color);
    let tx = c.pos.x + 2;
    let ty = c.pos.y + ((c.size.h as i32 - 10) / 2) + 10;
    let _ = Text::new(display_text, Point::new(tx, ty), text_style).draw(target);

    // Cursor line.
    if !input.text.is_empty() || input.cursor_pos == 0 {
        let cursor_x = tx + input.cursor_pos as i32 * 6;
        let cursor_top = c.pos.y + 2;
        let cursor_bottom = c.pos.y + c.size.h as i32 - 2;
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
    let oy = image.common.pos.y;

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
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &li.common;

    // Selected highlight background
    if li.selected {
        let bg = mapper.map(Color::Surface, theme);
        let rect = Rectangle::new(
            Point::new(c.pos.x, c.pos.y),
            embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
        );
        let style = PrimitiveStyleBuilder::new().fill_color(bg).build();
        let _ = rect.into_styled(style).draw(target);
    }

    // Title
    let title_color = mapper.map(li.title_color, theme);
    let title_style = MonoTextStyle::new(&FONT_6X10, title_color);
    let tx = c.pos.x + 4;
    let ty = c.pos.y + 12;
    let _ = Text::new(li.title.as_str(), Point::new(tx, ty), title_style).draw(target);

    // Subtitle (below title, dimmer)
    if !li.subtitle.is_empty() {
        let sub_color = mapper.map(li.subtitle_color, theme);
        let sub_style = MonoTextStyle::new(&FONT_6X10, sub_color);
        let _ = Text::new(li.subtitle.as_str(), Point::new(tx, ty + 12), sub_style).draw(target);
    }

    // Badge (right-aligned)
    if !li.badge.is_empty() {
        let badge_color = mapper.map(li.badge_color, theme);
        let badge_style = MonoTextStyle::new(&FONT_6X10, badge_color);
        let badge_w = li.badge.len() as i32 * 6;
        let bx = c.pos.x + c.size.w as i32 - badge_w - 6;
        let _ = Text::new(li.badge.as_str(), Point::new(bx, ty), badge_style).draw(target);
    }
}

fn draw_progress_bar<D, M>(
    pb: &crate::widget::ProgressBarWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &pb.common;

    // Track
    let track_color = mapper.map(pb.track_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, c.pos.y),
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
            Point::new(c.pos.x, c.pos.y),
            embedded_graphics::geometry::Size::new(fill_w, c.size.h),
        );
        let bar_style = PrimitiveStyleBuilder::new().fill_color(bar_color).build();
        let _ = fill_rect.into_styled(bar_style).draw(target);
    }
}

fn draw_divider<D, M>(
    d: &crate::widget::DividerWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &d.common;
    let color = mapper.map(d.color, theme);
    let style = PrimitiveStyleBuilder::new().stroke_color(color).stroke_width(d.thickness as u32).build();
    let start = Point::new(c.pos.x, c.pos.y);
    let end = if matches!(d.direction, crate::layout::Direction::Row) {
        Point::new(c.pos.x + c.size.w as i32, c.pos.y)
    } else {
        Point::new(c.pos.x, c.pos.y + c.size.h as i32)
    };
    let _ = Line::new(start, end).into_styled(style).draw(target);
}

fn draw_status_bar<D, M>(
    sb: &crate::widget::StatusBarWidget,
    theme: &Theme,
    mapper: &M,
    target: &mut D,
) where
    D: DrawTarget<Color = M::TargetColor>,
    M: ColorMapper,
{
    let c = &sb.common;

    // Background fill
    let bg = mapper.map(sb.bg_color, theme);
    let rect = Rectangle::new(
        Point::new(c.pos.x, c.pos.y),
        embedded_graphics::geometry::Size::new(c.size.w, c.size.h),
    );
    let bg_style = PrimitiveStyleBuilder::new().fill_color(bg).build();
    let _ = rect.into_styled(bg_style).draw(target);

    let text_color = mapper.map(sb.text_color, theme);
    let text_style = MonoTextStyle::new(&FONT_6X10, text_color);
    let ty = c.pos.y + ((c.size.h as i32 - 10) / 2) + 10;

    // Left text
    if !sb.left_text.is_empty() {
        let _ = Text::new(sb.left_text.as_str(), Point::new(c.pos.x + 4, ty), text_style).draw(target);
    }

    // Center text
    if !sb.center_text.is_empty() {
        let tw = sb.center_text.len() as i32 * 6;
        let cx = c.pos.x + (c.size.w as i32 - tw) / 2;
        let _ = Text::new(sb.center_text.as_str(), Point::new(cx, ty), text_style).draw(target);
    }

    // Right text
    if !sb.right_text.is_empty() {
        let tw = sb.right_text.len() as i32 * 6;
        let rx = c.pos.x + c.size.w as i32 - tw - 4;
        let _ = Text::new(sb.right_text.as_str(), Point::new(rx, ty), text_style).draw(target);
    }
}

fn font_height(size: &FontSize) -> u32 {
    // FONT_6X10 is our only font for now — all sizes map to 10px.
    // When additional fonts are added, this function will select appropriately.
    match size {
        FontSize::Small => 10,
        FontSize::Normal => 10,
        FontSize::Large => 10,
    }
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
