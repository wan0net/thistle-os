// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * lvgl_wm.c — LVGL-based window manager for ThistleOS
 *
 * Implements the display_server_wm_t vtable including the widget API.
 * Widget handles are LVGL object pointers cast to uint32_t.
 */

#include "thistle/display_server.h"
#include "ui/manager.h"
#include "ui/theme.h"
#include "esp_log.h"
#include "lvgl.h"
#include <string.h>
#include <stdint.h>

static const char *TAG = "lvgl_wm";

/* ── Handle conversion ───────────────────────────────────────────────── */

static inline lv_obj_t *h2obj(uint32_t h) { return (lv_obj_t *)(uintptr_t)h; }
static inline uint32_t obj2h(lv_obj_t *o) { return (uint32_t)(uintptr_t)o; }

/* ── WM lifecycle callbacks ──────────────────────────────────────────── */

static esp_err_t lvgl_wm_init(void)
{
    ESP_LOGI(TAG, "LVGL window manager initializing");
    esp_err_t ret = ui_manager_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ui_manager_init failed: %s", esp_err_to_name(ret));
        return ret;
    }
    ESP_LOGI(TAG, "LVGL window manager ready");
    return ESP_OK;
}

static void lvgl_wm_deinit(void) {}
static void lvgl_wm_render(void) {}
static void lvgl_wm_on_theme_changed(const char *p) { (void)p; }
static void lvgl_wm_on_app_launched(const char *id, surface_id_t s) { (void)id; (void)s; }
static void lvgl_wm_on_app_stopped(const char *id) { (void)id; }
static void lvgl_wm_on_app_switched(const char *id) { (void)id; }
static bool lvgl_wm_on_input(const hal_input_event_t *e) { (void)e; return false; }

/* ── Widget API: creation ────────────────────────────────────────────── */

static uint32_t wgt_get_app_root(void)
{
    return obj2h(ui_manager_get_app_area());
}

static uint32_t wgt_create_container(uint32_t parent)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *c = lv_obj_create(p);
    lv_obj_set_style_bg_opa(c, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(c, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(c, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(c, 0, LV_PART_MAIN);
    return obj2h(c);
}

static uint32_t wgt_create_label(uint32_t parent, const char *text)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *lbl = lv_label_create(p);
    if (text) lv_label_set_text(lbl, text);
    return obj2h(lbl);
}

static uint32_t wgt_create_button(uint32_t parent, const char *text)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *btn = lv_button_create(p);
    if (text) {
        lv_obj_t *lbl = lv_label_create(btn);
        lv_label_set_text(lbl, text);
        lv_obj_center(lbl);
    }
    return obj2h(btn);
}

static uint32_t wgt_create_text_input(uint32_t parent, const char *placeholder)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *ta = lv_textarea_create(p);
    lv_textarea_set_one_line(ta, true);
    if (placeholder) lv_textarea_set_placeholder_text(ta, placeholder);
    return obj2h(ta);
}

static void wgt_destroy(uint32_t widget)
{
    if (widget) lv_obj_delete(h2obj(widget));
}

/* ── Widget API: properties ──────────────────────────────────────────── */

static void wgt_set_text(uint32_t widget, const char *text)
{
    if (!widget || !text) return;
    lv_obj_t *obj = h2obj(widget);
    // Try as label first, then textarea, then button's child label
    if (lv_obj_check_type(obj, &lv_label_class)) {
        lv_label_set_text(obj, text);
    } else if (lv_obj_check_type(obj, &lv_textarea_class)) {
        lv_textarea_set_text(obj, text);
    } else {
        // Button — set first child label
        lv_obj_t *child = lv_obj_get_child(obj, 0);
        if (child && lv_obj_check_type(child, &lv_label_class)) {
            lv_label_set_text(child, text);
        }
    }
}

static const char *wgt_get_text(uint32_t widget)
{
    if (!widget) return "";
    lv_obj_t *obj = h2obj(widget);
    if (lv_obj_check_type(obj, &lv_label_class)) {
        return lv_label_get_text(obj);
    } else if (lv_obj_check_type(obj, &lv_textarea_class)) {
        return lv_textarea_get_text(obj);
    }
    lv_obj_t *child = lv_obj_get_child(obj, 0);
    if (child && lv_obj_check_type(child, &lv_label_class)) {
        return lv_label_get_text(child);
    }
    return "";
}

static void wgt_set_size(uint32_t widget, int w, int h)
{
    if (!widget) return;
    int lw = (w == -1) ? LV_PCT(100) : (w == -2) ? LV_SIZE_CONTENT : w;
    int lh = (h == -1) ? LV_PCT(100) : (h == -2) ? LV_SIZE_CONTENT : h;
    lv_obj_set_size(h2obj(widget), lw, lh);
}

static void wgt_set_pos(uint32_t widget, int x, int y)
{
    if (!widget) return;
    lv_obj_set_pos(h2obj(widget), x, y);
}

static void wgt_set_visible(uint32_t widget, bool visible)
{
    if (!widget) return;
    if (visible) lv_obj_clear_flag(h2obj(widget), LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(h2obj(widget), LV_OBJ_FLAG_HIDDEN);
}

static void wgt_set_bg_color(uint32_t widget, uint32_t color)
{
    if (!widget) return;
    uint8_t r = (color >> 16) & 0xFF;
    uint8_t g = (color >> 8) & 0xFF;
    uint8_t b = color & 0xFF;
    lv_obj_set_style_bg_color(h2obj(widget), lv_color_make(r, g, b), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(h2obj(widget), LV_OPA_COVER, LV_PART_MAIN);
}

static void wgt_set_text_color(uint32_t widget, uint32_t color)
{
    if (!widget) return;
    uint8_t r = (color >> 16) & 0xFF;
    uint8_t g = (color >> 8) & 0xFF;
    uint8_t b = color & 0xFF;
    lv_obj_set_style_text_color(h2obj(widget), lv_color_make(r, g, b), LV_PART_MAIN);
}

static void wgt_set_font_size(uint32_t widget, int size)
{
    if (!widget) return;
    const lv_font_t *f = &lv_font_montserrat_14;
    if (size >= 22) f = &lv_font_montserrat_22;
    else if (size >= 18) f = &lv_font_montserrat_18;
    lv_obj_set_style_text_font(h2obj(widget), f, LV_PART_MAIN);
}

/* ── Widget API: layout ──────────────────────────────────────────────── */

static void wgt_set_layout(uint32_t widget, int layout)
{
    if (!widget) return;
    switch (layout) {
        case 1: lv_obj_set_flex_flow(h2obj(widget), LV_FLEX_FLOW_COLUMN); break;
        case 2: lv_obj_set_flex_flow(h2obj(widget), LV_FLEX_FLOW_ROW); break;
        default: break;
    }
}

static void wgt_set_align(uint32_t widget, int main_a, int cross_a)
{
    if (!widget) return;
    lv_flex_align_t m = LV_FLEX_ALIGN_START;
    lv_flex_align_t c = LV_FLEX_ALIGN_START;
    switch (main_a) { case 1: m = LV_FLEX_ALIGN_CENTER; break; case 2: m = LV_FLEX_ALIGN_END; break; case 3: m = LV_FLEX_ALIGN_SPACE_BETWEEN; break; }
    switch (cross_a) { case 1: c = LV_FLEX_ALIGN_CENTER; break; case 2: c = LV_FLEX_ALIGN_END; break; }
    lv_obj_set_flex_align(h2obj(widget), m, c, c);
}

static void wgt_set_gap(uint32_t widget, int gap)
{
    if (!widget) return;
    lv_obj_set_style_pad_row(h2obj(widget), gap, LV_PART_MAIN);
    lv_obj_set_style_pad_column(h2obj(widget), gap, LV_PART_MAIN);
}

static void wgt_set_flex_grow(uint32_t widget, int grow)
{
    if (!widget) return;
    lv_obj_set_flex_grow(h2obj(widget), (uint8_t)grow);
}

static void wgt_set_scrollable(uint32_t widget, bool scrollable)
{
    if (!widget) return;
    if (scrollable) lv_obj_add_flag(h2obj(widget), LV_OBJ_FLAG_SCROLLABLE);
    else lv_obj_clear_flag(h2obj(widget), LV_OBJ_FLAG_SCROLLABLE);
}

static void wgt_set_padding(uint32_t widget, int t, int r, int b, int l)
{
    if (!widget) return;
    lv_obj_t *o = h2obj(widget);
    lv_obj_set_style_pad_top(o, t, LV_PART_MAIN);
    lv_obj_set_style_pad_right(o, r, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(o, b, LV_PART_MAIN);
    lv_obj_set_style_pad_left(o, l, LV_PART_MAIN);
}

static void wgt_set_border_width(uint32_t widget, int w)
{
    if (!widget) return;
    lv_obj_set_style_border_width(h2obj(widget), w, LV_PART_MAIN);
}

static void wgt_set_radius(uint32_t widget, int r)
{
    if (!widget) return;
    lv_obj_set_style_radius(h2obj(widget), r, LV_PART_MAIN);
}

/* ── Widget API: events ──────────────────────────────────────────────── */

typedef struct {
    void (*cb)(uint32_t, int, void *);
    void *user_data;
    int event_type;
} wgt_event_ctx_t;

static void wgt_lvgl_event_bridge(lv_event_t *e)
{
    wgt_event_ctx_t *ctx = (wgt_event_ctx_t *)lv_event_get_user_data(e);
    if (ctx && ctx->cb) {
        lv_obj_t *target = lv_event_get_target(e);
        ctx->cb(obj2h(target), ctx->event_type, ctx->user_data);
    }
}

static void wgt_on_event(uint32_t widget, int event_type, void (*cb)(uint32_t, int, void*), void *ud)
{
    if (!widget || !cb) return;
    // Allocate context — lives for the widget's lifetime (leaked on destroy, acceptable)
    wgt_event_ctx_t *ctx = (wgt_event_ctx_t *)lv_malloc(sizeof(wgt_event_ctx_t));
    if (!ctx) return;
    ctx->cb = cb;
    ctx->user_data = ud;
    ctx->event_type = event_type;

    lv_event_code_t code = LV_EVENT_CLICKED;
    switch (event_type) {
        case 0: code = LV_EVENT_CLICKED; break;
        case 1: code = LV_EVENT_VALUE_CHANGED; break;
        case 2: code = LV_EVENT_KEY; break;
    }
    lv_obj_add_event_cb(h2obj(widget), wgt_lvgl_event_bridge, code, ctx);
}

/* ── Widget API: text input ──────────────────────────────────────────── */

static void wgt_set_password_mode(uint32_t widget, bool pw)
{
    if (!widget) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_password_mode(h2obj(widget), pw);
}

static void wgt_set_one_line(uint32_t widget, bool one)
{
    if (!widget) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_one_line(h2obj(widget), one);
}

static void wgt_set_placeholder(uint32_t widget, const char *text)
{
    if (!widget || !text) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_placeholder_text(h2obj(widget), text);
}

/* ── Widget API: theme ───────────────────────────────────────────────── */

static uint32_t color_to_rgb(lv_color_t c) {
    return ((uint32_t)c.red << 16) | ((uint32_t)c.green << 8) | c.blue;
}

static uint32_t wgt_theme_primary(void)      { return color_to_rgb(theme_get_colors()->primary); }
static uint32_t wgt_theme_bg(void)           { return color_to_rgb(theme_get_colors()->bg); }
static uint32_t wgt_theme_surface(void)      { return color_to_rgb(theme_get_colors()->surface); }
static uint32_t wgt_theme_text(void)         { return color_to_rgb(theme_get_colors()->text); }
static uint32_t wgt_theme_text_secondary(void) { return color_to_rgb(theme_get_colors()->text_secondary); }

/* ── Public WM vtable ───────────────────────────────────────────────── */

static const display_server_wm_t s_lvgl_wm = {
    /* Lifecycle */
    .init              = lvgl_wm_init,
    .deinit            = lvgl_wm_deinit,
    .render            = lvgl_wm_render,
    .on_theme_changed  = lvgl_wm_on_theme_changed,
    .on_app_launched   = lvgl_wm_on_app_launched,
    .on_app_stopped    = lvgl_wm_on_app_stopped,
    .on_app_switched   = lvgl_wm_on_app_switched,
    .on_input          = lvgl_wm_on_input,

    /* Widget API */
    .widget_get_app_root     = wgt_get_app_root,
    .widget_create_container = wgt_create_container,
    .widget_create_label     = wgt_create_label,
    .widget_create_button    = wgt_create_button,
    .widget_create_text_input = wgt_create_text_input,
    .widget_destroy          = wgt_destroy,
    .widget_set_text         = wgt_set_text,
    .widget_get_text         = wgt_get_text,
    .widget_set_size         = wgt_set_size,
    .widget_set_pos          = wgt_set_pos,
    .widget_set_visible      = wgt_set_visible,
    .widget_set_bg_color     = wgt_set_bg_color,
    .widget_set_text_color   = wgt_set_text_color,
    .widget_set_font_size    = wgt_set_font_size,
    .widget_set_layout       = wgt_set_layout,
    .widget_set_align        = wgt_set_align,
    .widget_set_gap          = wgt_set_gap,
    .widget_set_flex_grow    = wgt_set_flex_grow,
    .widget_set_scrollable   = wgt_set_scrollable,
    .widget_set_padding      = wgt_set_padding,
    .widget_set_border_width = wgt_set_border_width,
    .widget_set_radius       = wgt_set_radius,
    .widget_on_event         = wgt_on_event,
    .widget_set_password_mode = wgt_set_password_mode,
    .widget_set_one_line     = wgt_set_one_line,
    .widget_set_placeholder  = wgt_set_placeholder,
    .widget_theme_primary    = wgt_theme_primary,
    .widget_theme_bg         = wgt_theme_bg,
    .widget_theme_surface    = wgt_theme_surface,
    .widget_theme_text       = wgt_theme_text,
    .widget_theme_text_secondary = wgt_theme_text_secondary,

    /* Info */
    .name              = "lvgl-wm",
    .version           = "2.0.0",
};

const display_server_wm_t *lvgl_wm_get(void)
{
    return &s_lvgl_wm;
}

/* Helper for widget_shims.c to get the active WM pointer */
const void *display_server_get_active_wm(void)
{
    return (const void *)&s_lvgl_wm;
}
