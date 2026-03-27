// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — thistle-tk window manager backend
//
// Implements the WM widget vtable using the thistle-tk widget tree and
// embedded-graphics renderer. Can replace LVGL as the window manager for
// both e-paper and LCD displays.
//
// The module holds a global `UiTree` behind a mutex. Widget API calls from
// apps (thistle_ui_* -> wm_widget_* -> here) manipulate the tree. The
// render function runs layout, renders to a framebuffer, and flushes to the
// HAL display driver.

use std::os::raw::c_char;
use std::sync::Mutex;

use thistle_tk::color::Color;
use thistle_tk::layout::{Align, Direction, Rect};
use thistle_tk::render::{self, MonoMapper, RgbMapper};
use thistle_tk::theme::Theme;
use thistle_tk::tree::UiTree;
use thistle_tk::widget::{
    ButtonWidget, ContainerWidget, DividerWidget, LabelWidget, ListItemWidget, ProgressBarWidget,
    Size, SizeHint, SpacerWidget, StatusBarWidget, TextInputWidget, Widget, WidgetId,
};

// ---------------------------------------------------------------------------
// Framebuffer wrapper implementing embedded-graphics DrawTarget
// ---------------------------------------------------------------------------

use embedded_graphics::pixelcolor::{BinaryColor, PixelColor, Rgb565, IntoStorage};
use embedded_graphics::prelude::*;

/// A simple framebuffer that implements `DrawTarget`.
/// For e-paper: BinaryColor, row-major packed bits (MSB first).
/// For LCD: Rgb565, row-major 16-bit pixels (little-endian).
struct Framebuffer<C: PixelColor> {
    buf: Vec<u8>,
    width: u32,
    height: u32,
    _color: core::marker::PhantomData<C>,
}

impl Framebuffer<BinaryColor> {
    fn new_mono(w: u32, h: u32) -> Self {
        let byte_count = ((w * h + 7) / 8) as usize;
        // Start with all white (0xFF = all bits Off for BinaryColor)
        Self {
            buf: vec![0xFF; byte_count],
            width: w,
            height: h,
            _color: core::marker::PhantomData,
        }
    }

    fn clear_white(&mut self) {
        for b in self.buf.iter_mut() {
            *b = 0xFF;
        }
    }
}

impl DrawTarget for Framebuffer<BinaryColor> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let x = point.x;
            let y = point.y;
            if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                continue;
            }
            let bit_index = (y as u32) * self.width + (x as u32);
            let byte_index = (bit_index / 8) as usize;
            let bit_offset = 7 - (bit_index % 8);
            match color {
                BinaryColor::On => {
                    // On = black = bit cleared
                    self.buf[byte_index] &= !(1 << bit_offset);
                }
                BinaryColor::Off => {
                    // Off = white = bit set
                    self.buf[byte_index] |= 1 << bit_offset;
                }
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Framebuffer<BinaryColor> {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(self.width, self.height)
    }
}

impl Framebuffer<Rgb565> {
    fn new_rgb(w: u32, h: u32) -> Self {
        let byte_count = (w * h * 2) as usize;
        Self {
            buf: vec![0; byte_count],
            width: w,
            height: h,
            _color: core::marker::PhantomData,
        }
    }

    fn clear_black(&mut self) {
        for b in self.buf.iter_mut() {
            *b = 0;
        }
    }
}

impl DrawTarget for Framebuffer<Rgb565> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let x = point.x;
            let y = point.y;
            if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                continue;
            }
            let offset = ((y as u32 * self.width + x as u32) * 2) as usize;
            let val: u16 = color.into_storage();
            self.buf[offset] = (val & 0xFF) as u8;
            self.buf[offset + 1] = (val >> 8) as u8;
        }
        Ok(())
    }
}

impl OriginDimensions for Framebuffer<Rgb565> {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(self.width, self.height)
    }
}

// ---------------------------------------------------------------------------
// Display mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Mono,
    Rgb,
}

// ---------------------------------------------------------------------------
// Global WM state
// ---------------------------------------------------------------------------

struct TkWmState {
    tree: UiTree,
    theme: Theme,
    mode: DisplayMode,
    mono_fb: Option<Framebuffer<BinaryColor>>,
    rgb_fb: Option<Framebuffer<Rgb565>>,
    width: u32,
    height: u32,
    dirty: bool,
    /// Static buffer for get_text return value (must outlive FFI call)
    text_buf: [u8; 257],
}

static TK_WM: Mutex<Option<TkWmState>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// HAL FFI
// ---------------------------------------------------------------------------

use crate::hal_registry::HalArea;

/// Return the display width from the HAL registry, or 320 if no driver is registered.
fn hal_display_width() -> u16 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        unsafe { (*reg.display).width }
    } else {
        320
    }
}

/// Return the display height from the HAL registry, or 240 if no driver is registered.
fn hal_display_height() -> u16 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        unsafe { (*reg.display).height }
    } else {
        240
    }
}

/// Flush pixel data to the display driver.
///
/// # Safety
/// `area` and `data` must be valid pointers for the duration of the call.
unsafe fn tk_wm_hal_flush_rs(area: *const HalArea, data: *const u8) -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        if let Some(flush_fn) = (*reg.display).flush {
            return flush_fn(area, data);
        }
    }
    -1
}

/// Trigger a display refresh (e-paper full/partial commit).
unsafe fn tk_wm_hal_refresh_rs() -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        if let Some(refresh_fn) = (*reg.display).refresh {
            return refresh_fn();
        }
    }
    -1
}

/// Check whether the display driver exposes a refresh() function (e-paper vs LCD).
fn tk_wm_hal_has_refresh_rs() -> bool {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        unsafe { (*reg.display).refresh.is_some() }
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Helper: convert u32 color (0xRRGGBB) to thistle-tk Color
// ---------------------------------------------------------------------------

fn color_from_u32(c: u32) -> Color {
    let r = ((c >> 16) & 0xFF) as u8;
    let g = ((c >> 8) & 0xFF) as u8;
    let b = (c & 0xFF) as u8;
    Color::Rgb(r, g, b)
}

fn color_to_u32(c: Color, theme: &Theme) -> u32 {
    let (r, g, b) = theme.resolve(c);
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

// ---------------------------------------------------------------------------
// C-safe string helpers
// ---------------------------------------------------------------------------

unsafe fn cstr_to_str<'a>(s: *const c_char) -> &'a str {
    if s.is_null() {
        return "";
    }
    std::ffi::CStr::from_ptr(s).to_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// WM lifecycle — called from C vtable
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn tk_wm_init() -> i32 {
    let width = hal_display_width() as u32;
    let height = hal_display_height() as u32;

    let has_refresh = tk_wm_hal_has_refresh_rs();
    let mode = if has_refresh {
        DisplayMode::Mono
    } else {
        DisplayMode::Rgb
    };

    let theme = match mode {
        DisplayMode::Mono => Theme::monochrome(),
        DisplayMode::Rgb => Theme::link42(),
    };

    // Create root container filling the viewport
    let mut root = ContainerWidget::default();
    root.common.size = Size { w: width, h: height };
    root.direction = Direction::Column;
    root.bg_color = Some(Color::Background);
    let tree = UiTree::new(Widget::Container(root));

    let (mono_fb, rgb_fb) = match mode {
        DisplayMode::Mono => (Some(Framebuffer::new_mono(width, height)), None),
        DisplayMode::Rgb => (None, Some(Framebuffer::new_rgb(width, height))),
    };

    let mut lock = TK_WM.lock().unwrap();
    *lock = Some(TkWmState {
        tree,
        theme,
        mode,
        mono_fb,
        rgb_fb,
        width,
        height,
        dirty: true,
        text_buf: [0u8; 257],
    });

    0 // ESP_OK
}

#[no_mangle]
pub extern "C" fn tk_wm_deinit() {
    let mut lock = TK_WM.lock().unwrap();
    *lock = None;
}

#[no_mangle]
pub extern "C" fn tk_wm_render() {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };

    if !state.dirty {
        return;
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: state.width,
        h: state.height,
    };

    // Run layout
    thistle_tk::layout::layout(&mut state.tree, viewport);

    // Render to framebuffer and flush
    let area = HalArea {
        x1: 0,
        y1: 0,
        x2: state.width.saturating_sub(1) as u16,
        y2: state.height.saturating_sub(1) as u16,
    };

    match state.mode {
        DisplayMode::Mono => {
            if let Some(ref mut fb) = state.mono_fb {
                fb.clear_white();
                render::render(&state.tree, &state.theme, &MonoMapper, fb);
                unsafe {
                    tk_wm_hal_flush_rs(&area, fb.buf.as_ptr());
                    tk_wm_hal_refresh_rs();
                }
            }
        }
        DisplayMode::Rgb => {
            if let Some(ref mut fb) = state.rgb_fb {
                fb.clear_black();
                render::render(&state.tree, &state.theme, &RgbMapper, fb);
                unsafe {
                    tk_wm_hal_flush_rs(&area, fb.buf.as_ptr());
                }
            }
        }
    }

    state.tree.clear_dirty();
    state.dirty = false;
}

fn mark_dirty(state: &mut TkWmState, id: WidgetId) {
    state.tree.mark_dirty(id);
    state.dirty = true;
}

// ---------------------------------------------------------------------------
// Widget API — called from C vtable, matches wm_widget_* signatures
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn tk_wm_widget_get_app_root() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(state) => state.tree.root() as u32,
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_create_container(parent: u32) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return 0,
    };
    let widget = Widget::Container(ContainerWidget::default());
    match state.tree.add_child(parent as WidgetId, widget) {
        Some(id) => {
            state.dirty = true;
            id as u32
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_create_label(parent: u32, text: *const c_char) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return 0,
    };
    let mut label = LabelWidget::default();
    let _ = label.text.push_str(cstr_to_str(text));
    let widget = Widget::Label(label);
    match state.tree.add_child(parent as WidgetId, widget) {
        Some(id) => {
            state.dirty = true;
            id as u32
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_create_button(parent: u32, text: *const c_char) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return 0,
    };
    let mut button = ButtonWidget::default();
    let _ = button.text.push_str(cstr_to_str(text));
    let widget = Widget::Button(button);
    match state.tree.add_child(parent as WidgetId, widget) {
        Some(id) => {
            state.dirty = true;
            id as u32
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_create_text_input(
    parent: u32,
    placeholder: *const c_char,
) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return 0,
    };
    let mut input = TextInputWidget::default();
    let _ = input.placeholder.push_str(cstr_to_str(placeholder));
    let widget = Widget::TextInput(input);
    match state.tree.add_child(parent as WidgetId, widget) {
        Some(id) => {
            state.dirty = true;
            id as u32
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// New widget creation FFI (Phase 2)
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_create_list_item(
    parent: u32, title: *const c_char, subtitle: *const c_char,
) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return 0 };
    let mut li = ListItemWidget::default();
    if !title.is_null() {
        if let Ok(s) = std::ffi::CStr::from_ptr(title).to_str() {
            let _ = li.title.push_str(s);
        }
    }
    if !subtitle.is_null() {
        if let Ok(s) = std::ffi::CStr::from_ptr(subtitle).to_str() {
            let _ = li.subtitle.push_str(s);
        }
    }
    match state.tree.add_child(parent as WidgetId, Widget::ListItem(li)) {
        Some(id) => { state.dirty = true; id as u32 }
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_create_progress_bar(parent: u32, value: i32) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return 0 };
    let mut pb = ProgressBarWidget::default();
    pb.value = (value as u8).min(100);
    match state.tree.add_child(parent as WidgetId, Widget::ProgressBar(pb)) {
        Some(id) => { state.dirty = true; id as u32 }
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_create_divider(parent: u32) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return 0 };
    match state.tree.add_child(parent as WidgetId, Widget::Divider(DividerWidget::default())) {
        Some(id) => { state.dirty = true; id as u32 }
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_create_spacer(parent: u32) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return 0 };
    let mut sp = SpacerWidget::default();
    sp.common.height_hint = SizeHint::Flex(1.0);
    match state.tree.add_child(parent as WidgetId, Widget::Spacer(sp)) {
        Some(id) => { state.dirty = true; id as u32 }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_create_status_bar(
    parent: u32, left: *const c_char, center: *const c_char, right: *const c_char,
) -> u32 {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return 0 };
    let mut sb = StatusBarWidget::default();
    if !left.is_null() {
        if let Ok(s) = std::ffi::CStr::from_ptr(left).to_str() { let _ = sb.left_text.push_str(s); }
    }
    if !center.is_null() {
        if let Ok(s) = std::ffi::CStr::from_ptr(center).to_str() { let _ = sb.center_text.push_str(s); }
    }
    if !right.is_null() {
        if let Ok(s) = std::ffi::CStr::from_ptr(right).to_str() { let _ = sb.right_text.push_str(s); }
    }
    match state.tree.add_child(parent as WidgetId, Widget::StatusBar(sb)) {
        Some(id) => { state.dirty = true; id as u32 }
        None => 0,
    }
}

/// Set the progress bar value (0-100).
#[no_mangle]
pub extern "C" fn tk_wm_widget_set_progress(widget: u32, value: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return };
    if let Some(Widget::ProgressBar(pb)) = state.tree.get_mut(widget as WidgetId) {
        pb.value = (value as u8).min(100);
        pb.common.dirty = true;
        state.dirty = true;
    }
}

/// Set the list item badge text.
#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_set_badge(widget: u32, badge: *const c_char) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return };
    if let Some(Widget::ListItem(li)) = state.tree.get_mut(widget as WidgetId) {
        li.badge.clear();
        if !badge.is_null() {
            if let Ok(s) = std::ffi::CStr::from_ptr(badge).to_str() {
                let _ = li.badge.push_str(s);
            }
        }
        li.common.dirty = true;
        state.dirty = true;
    }
}

/// Set list item selected state.
#[no_mangle]
pub extern "C" fn tk_wm_widget_set_selected(widget: u32, selected: bool) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() { Some(s) => s, None => return };
    if let Some(Widget::ListItem(li)) = state.tree.get_mut(widget as WidgetId) {
        li.selected = selected;
        li.common.dirty = true;
        state.dirty = true;
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_destroy(widget: u32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    if state.tree.remove(widget as WidgetId) {
        state.dirty = true;
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_set_text(widget: u32, text: *const c_char) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let text_str = cstr_to_str(text);
    if let Some(w) = state.tree.get_mut(id) {
        match w {
            Widget::Label(l) => {
                l.text.clear();
                let _ = l.text.push_str(text_str);
            }
            Widget::Button(b) => {
                b.text.clear();
                let _ = b.text.push_str(text_str);
            }
            Widget::TextInput(t) => {
                t.text.clear();
                let _ = t.text.push_str(text_str);
            }
            _ => {}
        }
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_get_text(widget: u32) -> *const c_char {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return b"\0".as_ptr() as *const c_char,
    };
    let id = widget as WidgetId;
    let text = match state.tree.get(id) {
        Some(Widget::Label(l)) => l.text.as_str(),
        Some(Widget::Button(b)) => b.text.as_str(),
        Some(Widget::TextInput(t)) => t.text.as_str(),
        _ => "",
    };
    // Copy to static buffer with null terminator
    let bytes = text.as_bytes();
    let len = bytes.len().min(256);
    state.text_buf[..len].copy_from_slice(&bytes[..len]);
    state.text_buf[len] = 0;
    state.text_buf.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_size(widget: u32, w: i32, h: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(wgt) = state.tree.get_mut(id) {
        let c = wgt.common_mut();
        if w > 0 {
            c.width_hint = SizeHint::Fixed(w as u32);
        }
        if h > 0 {
            c.height_hint = SizeHint::Fixed(h as u32);
        }
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_pos(widget: u32, x: i32, y: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(wgt) = state.tree.get_mut(id) {
        let c = wgt.common_mut();
        c.pos.x = x;
        c.pos.y = y;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_visible(widget: u32, visible: bool) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(wgt) = state.tree.get_mut(id) {
        wgt.common_mut().visible = visible;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_bg_color(widget: u32, color: u32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let tk_color = color_from_u32(color);
    if let Some(wgt) = state.tree.get_mut(id) {
        match wgt {
            Widget::Container(c) => c.bg_color = Some(tk_color),
            Widget::Button(b) => b.bg_color = tk_color,
            _ => {}
        }
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_text_color(widget: u32, color: u32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let tk_color = color_from_u32(color);
    if let Some(wgt) = state.tree.get_mut(id) {
        match wgt {
            Widget::Label(l) => l.color = tk_color,
            Widget::Button(b) => b.text_color = tk_color,
            Widget::TextInput(t) => t.text_color = tk_color,
            _ => {}
        }
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_font_size(widget: u32, size: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let font_size = match size {
        0..=10 => thistle_tk::widget::FontSize::Small,
        11..=16 => thistle_tk::widget::FontSize::Normal,
        _ => thistle_tk::widget::FontSize::Large,
    };
    if let Some(Widget::Label(l)) = state.tree.get_mut(id) {
        l.font_size = font_size;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_layout(widget: u32, layout: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    // layout: 0 = column, 1 = row (matches LVGL LV_FLEX_FLOW convention)
    let dir = if layout == 1 {
        Direction::Row
    } else {
        Direction::Column
    };
    if let Some(Widget::Container(c)) = state.tree.get_mut(id) {
        c.direction = dir;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_align(widget: u32, main_align: i32, cross_align: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let to_align = |a: i32| -> Align {
        match a {
            0 => Align::Start,
            1 => Align::Center,
            2 => Align::End,
            3 => Align::SpaceBetween,
            _ => Align::Start,
        }
    };
    if let Some(Widget::Container(c)) = state.tree.get_mut(id) {
        c.align = to_align(main_align);
        c.cross_align = to_align(cross_align);
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_gap(widget: u32, gap: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(Widget::Container(c)) = state.tree.get_mut(id) {
        c.gap = gap.max(0) as u16;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_flex_grow(widget: u32, grow: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(wgt) = state.tree.get_mut(id) {
        // flex-grow applies to the main axis size hint
        wgt.common_mut().height_hint = SizeHint::Flex(grow.max(0) as f32);
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_scrollable(widget: u32, _scrollable: bool) {
    // Scrolling not yet implemented in thistle-tk; mark dirty so it
    // re-renders if other properties also changed.
    let mut lock = TK_WM.lock().unwrap();
    if let Some(state) = lock.as_mut() {
        state.dirty = true;
        let _ = widget;
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(wgt) = state.tree.get_mut(id) {
        wgt.common_mut().padding = (
            l.max(0) as u16,
            t.max(0) as u16,
            r.max(0) as u16,
            b.max(0) as u16,
        );
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_border_width(_widget: u32, _w: i32) {
    // Border width not yet a separate property in thistle-tk
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_radius(widget: u32, r: i32) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(Widget::Button(b)) = state.tree.get_mut(id) {
        b.border_radius = r.max(0) as u16;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_on_event(
    widget: u32,
    event_type: i32,
    cb: *const std::os::raw::c_void,
    _ud: *mut std::os::raw::c_void,
) {
    // Map C callbacks to thistle-tk fn-pointer callbacks.
    // event_type 0 = press (matches LV_EVENT_CLICKED convention).
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if event_type == 0 {
        // Press event on a button
        if let Some(Widget::Button(b)) = state.tree.get_mut(id) {
            if cb.is_null() {
                b.on_press = None;
            } else {
                // Transmute the C callback to a Rust fn pointer.
                // The C callback signature is void(*)(uint32_t, int, void*)
                // but thistle-tk uses fn(WidgetId). We store a wrapper.
                // For now, store as on_press — the C callback user_data
                // is not propagated (apps should use the widget id).
                let _cb_fn = cb; // TODO: proper callback bridging
                b.on_press = None; // placeholder — needs callback adapter
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_password_mode(widget: u32, pw: bool) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(Widget::TextInput(t)) = state.tree.get_mut(id) {
        t.password_mode = pw;
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_set_one_line(widget: u32, one_line: bool) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    if let Some(Widget::Label(l)) = state.tree.get_mut(id) {
        l.max_lines = if one_line { 1 } else { 0 };
        mark_dirty(state, id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_widget_set_placeholder(widget: u32, text: *const c_char) {
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return,
    };
    let id = widget as WidgetId;
    let text_str = cstr_to_str(text);
    if let Some(Widget::TextInput(t)) = state.tree.get_mut(id) {
        t.placeholder.clear();
        let _ = t.placeholder.push_str(text_str);
        mark_dirty(state, id);
    }
}

// ---------------------------------------------------------------------------
// Theme color accessors
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn tk_wm_widget_theme_primary() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(s) => color_to_u32(Color::Primary, &s.theme),
        None => 0x000000,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_theme_bg() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(s) => color_to_u32(Color::Background, &s.theme),
        None => 0xFFFFFF,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_theme_surface() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(s) => color_to_u32(Color::Surface, &s.theme),
        None => 0xF0F0F0,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_theme_text() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(s) => color_to_u32(Color::Text, &s.theme),
        None => 0x000000,
    }
}

#[no_mangle]
pub extern "C" fn tk_wm_widget_theme_text_secondary() -> u32 {
    let lock = TK_WM.lock().unwrap();
    match lock.as_ref() {
        Some(s) => color_to_u32(Color::TextSecondary, &s.theme),
        None => 0x808080,
    }
}

// ---------------------------------------------------------------------------
// Input handling
// ---------------------------------------------------------------------------

/// Input event from HAL (matches hal_input_event_t layout)
#[repr(C)]
struct HalInputEvent {
    event_type: u32,
    timestamp: u32,
    data: [u16; 2],
}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_on_input(event: *const HalInputEvent) -> bool {
    if event.is_null() {
        return false;
    }
    let evt = &*event;
    let mut lock = TK_WM.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return false,
    };

    // Map HAL event types to thistle-tk InputEvent
    // HAL: 0=key_down, 1=key_up, 2=touch_down, 3=touch_up, 4=touch_move
    let tk_event = match evt.event_type {
        0 => thistle_tk::input::InputEvent::KeyDown {
            code: evt.data[0] as u32,
        },
        1 => thistle_tk::input::InputEvent::KeyUp {
            code: evt.data[0] as u32,
        },
        2 => thistle_tk::input::InputEvent::TouchDown {
            x: evt.data[0] as i32,
            y: evt.data[1] as i32,
        },
        3 => thistle_tk::input::InputEvent::TouchUp {
            x: evt.data[0] as i32,
            y: evt.data[1] as i32,
        },
        4 => thistle_tk::input::InputEvent::TouchMove {
            x: evt.data[0] as i32,
            y: evt.data[1] as i32,
        },
        _ => return false,
    };

    let handled = thistle_tk::input::dispatch_input(&mut state.tree, &tk_event);
    if handled {
        state.dirty = true;
    }
    handled
}

// ---------------------------------------------------------------------------
// Theme change
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn tk_wm_on_theme_changed(_theme_path: *const c_char) {
    // TODO: load theme from JSON file at theme_path
    // For now, just mark dirty to trigger a re-render with the current theme
    let mut lock = TK_WM.lock().unwrap();
    if let Some(state) = lock.as_mut() {
        state.dirty = true;
    }
}

// App lifecycle stubs — the thistle-tk WM doesn't need these yet but the
// vtable requires them.
#[no_mangle]
pub unsafe extern "C" fn tk_wm_on_app_launched(_app_id: *const c_char, _surface: u32) {}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_on_app_stopped(_app_id: *const c_char) {}

#[no_mangle]
pub unsafe extern "C" fn tk_wm_on_app_switched(_app_id: *const c_char) {}
