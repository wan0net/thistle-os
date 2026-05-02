// SPDX-License-Identifier: BSD-3-Clause
// thistle-tk WM — weak stubs and WM vtable construction
//
// HAL display bridge functions (tk_wm_hal_flush_rs, etc.) have been moved
// into Rust (components/kernel_rs/src/tk_wm.rs) now that the HAL registry
// is implemented in Rust.
//
// This file retains:
//   1. Weak stubs — satisfy the linker during static-lib extraction.
//   2. thistle_tk_wm_get() — constructs the display_server_wm_t vtable.

#include "thistle/display_server.h"
#include <stdbool.h>
#include <stdint.h>

// ── Weak stubs for Rust tk_wm functions ─────────────────────────────
// The Rust static lib provides strong symbols; these weak stubs satisfy
// the linker during static-lib extraction (same pattern as wm_widget_*).
#define W __attribute__((weak))
W int      tk_wm_init(void) { return 0; }
W void     tk_wm_deinit(void) {}
W void     tk_wm_do_refresh(void) {}
W void     tk_wm_render(void) {}
W void     tk_wm_on_theme_changed(const char *p) { (void)p; }
W void     tk_wm_on_app_launched(const char *id, uint32_t s) { (void)id; (void)s; }
W void     tk_wm_on_app_stopped(const char *id) { (void)id; }
W void     tk_wm_on_app_switched(const char *id) { (void)id; }
W bool     tk_wm_on_input(const hal_input_event_t *e) { (void)e; return false; }
W uint32_t tk_wm_widget_get_app_root(void) { return 0; }
W uint32_t tk_wm_widget_create_container(uint32_t p) { (void)p; return 0; }
W uint32_t tk_wm_widget_create_label(uint32_t p, const char *t) { (void)p; (void)t; return 0; }
W uint32_t tk_wm_widget_create_button(uint32_t p, const char *t) { (void)p; (void)t; return 0; }
W uint32_t tk_wm_widget_create_text_input(uint32_t p, const char *t) { (void)p; (void)t; return 0; }
W void     tk_wm_widget_destroy(uint32_t w) { (void)w; }
W void     tk_wm_widget_set_text(uint32_t w, const char *t) { (void)w; (void)t; }
W const char *tk_wm_widget_get_text(uint32_t w) { (void)w; return ""; }
W void     tk_wm_widget_set_size(uint32_t w, int a, int b) { (void)w; (void)a; (void)b; }
W void     tk_wm_widget_set_pos(uint32_t w, int x, int y) { (void)w; (void)x; (void)y; }
W void     tk_wm_widget_set_visible(uint32_t w, bool v) { (void)w; (void)v; }
W void     tk_wm_widget_set_bg_color(uint32_t w, uint32_t c) { (void)w; (void)c; }
W void     tk_wm_widget_set_text_color(uint32_t w, uint32_t c) { (void)w; (void)c; }
W void     tk_wm_widget_set_font_size(uint32_t w, int s) { (void)w; (void)s; }
W void     tk_wm_widget_set_layout(uint32_t w, int l) { (void)w; (void)l; }
W void     tk_wm_widget_set_align(uint32_t w, int m, int c) { (void)w; (void)m; (void)c; }
W void     tk_wm_widget_set_gap(uint32_t w, int g) { (void)w; (void)g; }
W void     tk_wm_widget_set_flex_grow(uint32_t w, int g) { (void)w; (void)g; }
W void     tk_wm_widget_set_scrollable(uint32_t w, bool s) { (void)w; (void)s; }
W void     tk_wm_widget_set_padding(uint32_t w, int t, int r, int b, int l) { (void)w; (void)t; (void)r; (void)b; (void)l; }
W void     tk_wm_widget_set_border_width(uint32_t w, int bw) { (void)w; (void)bw; }
W void     tk_wm_widget_set_radius(uint32_t w, int r) { (void)w; (void)r; }
W void     tk_wm_widget_on_event(uint32_t w, int e, void (*cb)(uint32_t, int, void*), void *ud) { (void)w; (void)e; (void)cb; (void)ud; }
W void     tk_wm_widget_set_password_mode(uint32_t w, bool p) { (void)w; (void)p; }
W void     tk_wm_widget_set_one_line(uint32_t w, bool o) { (void)w; (void)o; }
W void     tk_wm_widget_set_placeholder(uint32_t w, const char *t) { (void)w; (void)t; }
W uint32_t tk_wm_widget_theme_primary(void) { return 0x000000; }
W uint32_t tk_wm_widget_theme_bg(void) { return 0xFFFFFF; }
W uint32_t tk_wm_widget_theme_surface(void) { return 0xF0F0F0; }
W uint32_t tk_wm_widget_theme_text(void) { return 0x000000; }
W uint32_t tk_wm_widget_theme_text_secondary(void) { return 0x808080; }
#undef W

// ── Static vtable instance ──────────────────────────────────────────

static const display_server_wm_t s_tk_wm = {
    /* Lifecycle */
    .init               = tk_wm_init,
    .deinit             = tk_wm_deinit,

    /* Display */
    .render             = tk_wm_render,
    .on_theme_changed   = tk_wm_on_theme_changed,

    /* App lifecycle */
    .on_app_launched    = tk_wm_on_app_launched,
    .on_app_stopped     = tk_wm_on_app_stopped,
    .on_app_switched    = tk_wm_on_app_switched,

    /* Input */
    .on_input           = tk_wm_on_input,

    /* Widget API */
    .widget_get_app_root        = tk_wm_widget_get_app_root,
    .widget_create_container    = tk_wm_widget_create_container,
    .widget_create_label        = tk_wm_widget_create_label,
    .widget_create_button       = tk_wm_widget_create_button,
    .widget_create_text_input   = tk_wm_widget_create_text_input,
    .widget_destroy             = tk_wm_widget_destroy,
    .widget_set_text            = tk_wm_widget_set_text,
    .widget_get_text            = tk_wm_widget_get_text,
    .widget_set_size            = tk_wm_widget_set_size,
    .widget_set_pos             = tk_wm_widget_set_pos,
    .widget_set_visible         = tk_wm_widget_set_visible,
    .widget_set_bg_color        = tk_wm_widget_set_bg_color,
    .widget_set_text_color      = tk_wm_widget_set_text_color,
    .widget_set_font_size       = tk_wm_widget_set_font_size,
    .widget_set_layout          = tk_wm_widget_set_layout,
    .widget_set_align           = tk_wm_widget_set_align,
    .widget_set_gap             = tk_wm_widget_set_gap,
    .widget_set_flex_grow       = tk_wm_widget_set_flex_grow,
    .widget_set_scrollable      = tk_wm_widget_set_scrollable,
    .widget_set_padding         = tk_wm_widget_set_padding,
    .widget_set_border_width    = tk_wm_widget_set_border_width,
    .widget_set_radius          = tk_wm_widget_set_radius,
    .widget_on_event            = tk_wm_widget_on_event,
    .widget_set_password_mode   = tk_wm_widget_set_password_mode,
    .widget_set_one_line        = tk_wm_widget_set_one_line,
    .widget_set_placeholder     = tk_wm_widget_set_placeholder,
    .widget_theme_primary       = tk_wm_widget_theme_primary,
    .widget_theme_bg            = tk_wm_widget_theme_bg,
    .widget_theme_surface       = tk_wm_widget_theme_surface,
    .widget_theme_text          = tk_wm_widget_theme_text,
    .widget_theme_text_secondary = tk_wm_widget_theme_text_secondary,

    /* Info */
    .name    = "thistle-tk",
    .version = "0.1.0",
};

// ── Public accessor ─────────────────────────────────────────────────

const display_server_wm_t *thistle_tk_wm_get(void)
{
    return &s_tk_wm;
}
