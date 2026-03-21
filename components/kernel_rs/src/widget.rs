// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Widget API dispatcher
//
// Routes widget API calls to the active window manager's implementation.
// Apps call thistle_ui_* functions, the kernel dispatches to the WM vtable.

use std::os::raw::{c_char, c_void};

const THISTLE_WIDGET_NONE: u32 = 0;

// Get the active WM's widget function pointers from the display server
extern "C" {
    fn display_server_get_wm_vtable() -> *const c_void;
}

// The WM vtable struct layout must match display_server_wm_t in C.
// We only need the widget function pointers, which start after the
// fixed fields. Rather than redefine the entire struct, we use a
// C helper to get each function pointer.
extern "C" {
    fn wm_widget_get_app_root() -> u32;
    fn wm_widget_create_container(parent: u32) -> u32;
    fn wm_widget_create_label(parent: u32, text: *const c_char) -> u32;
    fn wm_widget_create_button(parent: u32, text: *const c_char) -> u32;
    fn wm_widget_create_text_input(parent: u32, placeholder: *const c_char) -> u32;
    fn wm_widget_destroy(widget: u32);
    fn wm_widget_set_text(widget: u32, text: *const c_char);
    fn wm_widget_get_text(widget: u32) -> *const c_char;
    fn wm_widget_set_size(widget: u32, w: i32, h: i32);
    fn wm_widget_set_pos(widget: u32, x: i32, y: i32);
    fn wm_widget_set_visible(widget: u32, visible: bool);
    fn wm_widget_set_bg_color(widget: u32, color: u32);
    fn wm_widget_set_text_color(widget: u32, color: u32);
    fn wm_widget_set_font_size(widget: u32, size: i32);
    fn wm_widget_set_layout(widget: u32, layout: i32);
    fn wm_widget_set_align(widget: u32, main_align: i32, cross_align: i32);
    fn wm_widget_set_gap(widget: u32, gap: i32);
    fn wm_widget_set_flex_grow(widget: u32, grow: i32);
    fn wm_widget_set_scrollable(widget: u32, scrollable: bool);
    fn wm_widget_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32);
    fn wm_widget_set_border_width(widget: u32, w: i32);
    fn wm_widget_set_radius(widget: u32, r: i32);
    fn wm_widget_on_event(widget: u32, event_type: i32, cb: *const c_void, ud: *mut c_void);
    fn wm_widget_set_password_mode(widget: u32, pw: bool);
    fn wm_widget_set_one_line(widget: u32, one_line: bool);
    fn wm_widget_set_placeholder(widget: u32, text: *const c_char);
    fn wm_widget_theme_primary() -> u32;
    fn wm_widget_theme_bg() -> u32;
    fn wm_widget_theme_surface() -> u32;
    fn wm_widget_theme_text() -> u32;
    fn wm_widget_theme_text_secondary() -> u32;
}

// ── FFI exports (syscall table entries) ─────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_get_app_root() -> u32 {
    wm_widget_get_app_root()
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_create_container(parent: u32) -> u32 {
    wm_widget_create_container(parent)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_create_label(parent: u32, text: *const c_char) -> u32 {
    wm_widget_create_label(parent, text)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_create_button(parent: u32, text: *const c_char) -> u32 {
    wm_widget_create_button(parent, text)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_create_text_input(parent: u32, placeholder: *const c_char) -> u32 {
    wm_widget_create_text_input(parent, placeholder)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_destroy(widget: u32) {
    wm_widget_destroy(widget)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_text(widget: u32, text: *const c_char) {
    wm_widget_set_text(widget, text)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_get_text(widget: u32) -> *const c_char {
    wm_widget_get_text(widget)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_size(widget: u32, w: i32, h: i32) {
    wm_widget_set_size(widget, w, h)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_pos(widget: u32, x: i32, y: i32) {
    wm_widget_set_pos(widget, x, y)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_visible(widget: u32, visible: bool) {
    wm_widget_set_visible(widget, visible)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_bg_color(widget: u32, color: u32) {
    wm_widget_set_bg_color(widget, color)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_text_color(widget: u32, color: u32) {
    wm_widget_set_text_color(widget, color)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_font_size(widget: u32, size: i32) {
    wm_widget_set_font_size(widget, size)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_layout(widget: u32, layout: i32) {
    wm_widget_set_layout(widget, layout)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_align(widget: u32, main_a: i32, cross_a: i32) {
    wm_widget_set_align(widget, main_a, cross_a)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_gap(widget: u32, gap: i32) {
    wm_widget_set_gap(widget, gap)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_flex_grow(widget: u32, grow: i32) {
    wm_widget_set_flex_grow(widget, grow)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_scrollable(widget: u32, scrollable: bool) {
    wm_widget_set_scrollable(widget, scrollable)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32) {
    wm_widget_set_padding(widget, t, r, b, l)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_border_width(widget: u32, w: i32) {
    wm_widget_set_border_width(widget, w)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_radius(widget: u32, r: i32) {
    wm_widget_set_radius(widget, r)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_on_event(widget: u32, event_type: i32, cb: *const c_void, ud: *mut c_void) {
    wm_widget_on_event(widget, event_type, cb, ud)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_password_mode(widget: u32, pw: bool) {
    wm_widget_set_password_mode(widget, pw)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_one_line(widget: u32, one_line: bool) {
    wm_widget_set_one_line(widget, one_line)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_set_placeholder(widget: u32, text: *const c_char) {
    wm_widget_set_placeholder(widget, text)
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_theme_primary() -> u32 {
    wm_widget_theme_primary()
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_theme_bg() -> u32 {
    wm_widget_theme_bg()
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_theme_surface() -> u32 {
    wm_widget_theme_surface()
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_theme_text() -> u32 {
    wm_widget_theme_text()
}

#[no_mangle]
pub unsafe extern "C" fn thistle_ui_theme_text_secondary() -> u32 {
    wm_widget_theme_text_secondary()
}
