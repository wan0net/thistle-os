// SPDX-License-Identifier: BSD-3-Clause
// Widget API shims — bridge between Rust widget dispatcher and C WM vtable
//
// The Rust kernel calls wm_widget_*() functions. These shims read the
// active WM's vtable from the display server and call the appropriate
// function pointer. If the WM doesn't implement a function, it's a no-op.

#include "thistle/display_server.h"
#include <stddef.h>

// Helper to get the active WM vtable
static const display_server_wm_t *get_wm(void) {
    extern const char *display_server_get_wm_name(void);
    // The WM vtable is stored in the display server. We access it via
    // a direct extern since the display server is in the Rust kernel.
    extern const void *display_server_get_active_wm(void);
    return (const display_server_wm_t *)display_server_get_active_wm();
}

// ── Widget creation ─────────────────────────────────────────────────

uint32_t wm_widget_get_app_root(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_get_app_root) ? wm->widget_get_app_root() : 0;
}

uint32_t wm_widget_create_container(uint32_t parent) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_create_container) ? wm->widget_create_container(parent) : 0;
}

uint32_t wm_widget_create_label(uint32_t parent, const char *text) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_create_label) ? wm->widget_create_label(parent, text) : 0;
}

uint32_t wm_widget_create_button(uint32_t parent, const char *text) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_create_button) ? wm->widget_create_button(parent, text) : 0;
}

uint32_t wm_widget_create_text_input(uint32_t parent, const char *placeholder) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_create_text_input) ? wm->widget_create_text_input(parent, placeholder) : 0;
}

void wm_widget_destroy(uint32_t widget) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_destroy) wm->widget_destroy(widget);
}

// ── Widget properties ───────────────────────────────────────────────

void wm_widget_set_text(uint32_t widget, const char *text) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_text) wm->widget_set_text(widget, text);
}

const char *wm_widget_get_text(uint32_t widget) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_get_text) ? wm->widget_get_text(widget) : "";
}

void wm_widget_set_size(uint32_t widget, int w, int h) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_size) wm->widget_set_size(widget, w, h);
}

void wm_widget_set_pos(uint32_t widget, int x, int y) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_pos) wm->widget_set_pos(widget, x, y);
}

void wm_widget_set_visible(uint32_t widget, bool visible) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_visible) wm->widget_set_visible(widget, visible);
}

void wm_widget_set_bg_color(uint32_t widget, uint32_t color) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_bg_color) wm->widget_set_bg_color(widget, color);
}

void wm_widget_set_text_color(uint32_t widget, uint32_t color) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_text_color) wm->widget_set_text_color(widget, color);
}

void wm_widget_set_font_size(uint32_t widget, int size) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_font_size) wm->widget_set_font_size(widget, size);
}

// ── Layout ──────────────────────────────────────────────────────────

void wm_widget_set_layout(uint32_t widget, int layout) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_layout) wm->widget_set_layout(widget, layout);
}

void wm_widget_set_align(uint32_t widget, int main_align, int cross_align) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_align) wm->widget_set_align(widget, main_align, cross_align);
}

void wm_widget_set_gap(uint32_t widget, int gap) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_gap) wm->widget_set_gap(widget, gap);
}

void wm_widget_set_flex_grow(uint32_t widget, int grow) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_flex_grow) wm->widget_set_flex_grow(widget, grow);
}

void wm_widget_set_scrollable(uint32_t widget, bool scrollable) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_scrollable) wm->widget_set_scrollable(widget, scrollable);
}

void wm_widget_set_padding(uint32_t widget, int t, int r, int b, int l) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_padding) wm->widget_set_padding(widget, t, r, b, l);
}

void wm_widget_set_border_width(uint32_t widget, int w) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_border_width) wm->widget_set_border_width(widget, w);
}

void wm_widget_set_radius(uint32_t widget, int r) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_radius) wm->widget_set_radius(widget, r);
}

// ── Events ──────────────────────────────────────────────────────────

void wm_widget_on_event(uint32_t widget, int event_type, void (*cb)(uint32_t, int, void*), void *ud) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_on_event) wm->widget_on_event(widget, event_type, cb, ud);
}

// ── Text input ──────────────────────────────────────────────────────

void wm_widget_set_password_mode(uint32_t widget, bool pw) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_password_mode) wm->widget_set_password_mode(widget, pw);
}

void wm_widget_set_one_line(uint32_t widget, bool one_line) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_one_line) wm->widget_set_one_line(widget, one_line);
}

void wm_widget_set_placeholder(uint32_t widget, const char *text) {
    const display_server_wm_t *wm = get_wm();
    if (wm && wm->widget_set_placeholder) wm->widget_set_placeholder(widget, text);
}

// ── Theme ───────────────────────────────────────────────────────────

uint32_t wm_widget_theme_primary(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_theme_primary) ? wm->widget_theme_primary() : 0x2563EB;
}

uint32_t wm_widget_theme_bg(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_theme_bg) ? wm->widget_theme_bg() : 0x111110;
}

uint32_t wm_widget_theme_surface(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_theme_surface) ? wm->widget_theme_surface() : 0x1C1C1B;
}

uint32_t wm_widget_theme_text(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_theme_text) ? wm->widget_theme_text() : 0xEDEDED;
}

uint32_t wm_widget_theme_text_secondary(void) {
    const display_server_wm_t *wm = get_wm();
    return (wm && wm->widget_theme_text_secondary) ? wm->widget_theme_text_secondary() : 0xA09F9B;
}
