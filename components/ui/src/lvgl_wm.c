// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * lvgl_wm.c — Shared LVGL widget API for ThistleOS window managers
 *
 * Contains the widget implementation functions used by both the e-paper
 * and LCD WM variants. Widget handles are LVGL object pointers cast to
 * uint32_t. All functions are non-static so the WM variant files can
 * reference them in their vtables.
 */

#include "thistle/display_server.h"
#include "ui/manager.h"
#include "ui/theme.h"
#include "esp_log.h"
#include "lvgl.h"
#include <string.h>
#include <stdint.h>

/* ── Handle conversion ───────────────────────────────────────────────── */

static inline lv_obj_t *h2obj(uint32_t h) { return (lv_obj_t *)(uintptr_t)h; }
static inline uint32_t obj2h(lv_obj_t *o) { return (uint32_t)(uintptr_t)o; }

/* ── Widget API: creation ────────────────────────────────────────────── */

uint32_t lvgl_wm_widget_get_app_root(void)
{
    return obj2h(ui_manager_get_app_area());
}

uint32_t lvgl_wm_widget_create_container(uint32_t parent)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *c = lv_obj_create(p);
    lv_obj_set_style_bg_opa(c, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(c, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(c, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(c, 0, LV_PART_MAIN);
    return obj2h(c);
}

uint32_t lvgl_wm_widget_create_label(uint32_t parent, const char *text)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *lbl = lv_label_create(p);
    if (text) lv_label_set_text(lbl, text);
    return obj2h(lbl);
}

uint32_t lvgl_wm_widget_create_button(uint32_t parent, const char *text)
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

uint32_t lvgl_wm_widget_create_text_input(uint32_t parent, const char *placeholder)
{
    lv_obj_t *p = parent ? h2obj(parent) : ui_manager_get_app_area();
    lv_obj_t *ta = lv_textarea_create(p);
    lv_textarea_set_one_line(ta, true);
    if (placeholder) lv_textarea_set_placeholder_text(ta, placeholder);
    return obj2h(ta);
}

void lvgl_wm_widget_destroy(uint32_t widget)
{
    if (widget) lv_obj_delete(h2obj(widget));
}

/* ── Widget API: properties ──────────────────────────────────────────── */

void lvgl_wm_widget_set_text(uint32_t widget, const char *text)
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

const char *lvgl_wm_widget_get_text(uint32_t widget)
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

void lvgl_wm_widget_set_size(uint32_t widget, int w, int h)
{
    if (!widget) return;
    int lw = (w == -1) ? LV_PCT(100) : (w == -2) ? LV_SIZE_CONTENT : w;
    int lh = (h == -1) ? LV_PCT(100) : (h == -2) ? LV_SIZE_CONTENT : h;
    lv_obj_set_size(h2obj(widget), lw, lh);
}

void lvgl_wm_widget_set_pos(uint32_t widget, int x, int y)
{
    if (!widget) return;
    lv_obj_set_pos(h2obj(widget), x, y);
}

void lvgl_wm_widget_set_visible(uint32_t widget, bool visible)
{
    if (!widget) return;
    if (visible) lv_obj_clear_flag(h2obj(widget), LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(h2obj(widget), LV_OBJ_FLAG_HIDDEN);
}

void lvgl_wm_widget_set_bg_color(uint32_t widget, uint32_t color)
{
    if (!widget) return;
    uint8_t r = (color >> 16) & 0xFF;
    uint8_t g = (color >> 8) & 0xFF;
    uint8_t b = color & 0xFF;
    lv_obj_set_style_bg_color(h2obj(widget), lv_color_make(r, g, b), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(h2obj(widget), LV_OPA_COVER, LV_PART_MAIN);
}

void lvgl_wm_widget_set_text_color(uint32_t widget, uint32_t color)
{
    if (!widget) return;
    uint8_t r = (color >> 16) & 0xFF;
    uint8_t g = (color >> 8) & 0xFF;
    uint8_t b = color & 0xFF;
    lv_obj_set_style_text_color(h2obj(widget), lv_color_make(r, g, b), LV_PART_MAIN);
}

void lvgl_wm_widget_set_font_size(uint32_t widget, int size)
{
    if (!widget) return;
    const lv_font_t *f = &lv_font_montserrat_14;
    if (size >= 22) f = &lv_font_montserrat_22;
    else if (size >= 18) f = &lv_font_montserrat_18;
    lv_obj_set_style_text_font(h2obj(widget), f, LV_PART_MAIN);
}

/* ── Widget API: layout ──────────────────────────────────────────────── */

void lvgl_wm_widget_set_layout(uint32_t widget, int layout)
{
    if (!widget) return;
    switch (layout) {
        case 1: lv_obj_set_flex_flow(h2obj(widget), LV_FLEX_FLOW_COLUMN); break;
        case 2: lv_obj_set_flex_flow(h2obj(widget), LV_FLEX_FLOW_ROW); break;
        default: break;
    }
}

void lvgl_wm_widget_set_align(uint32_t widget, int main_a, int cross_a)
{
    if (!widget) return;
    lv_flex_align_t m = LV_FLEX_ALIGN_START;
    lv_flex_align_t c = LV_FLEX_ALIGN_START;
    switch (main_a) { case 1: m = LV_FLEX_ALIGN_CENTER; break; case 2: m = LV_FLEX_ALIGN_END; break; case 3: m = LV_FLEX_ALIGN_SPACE_BETWEEN; break; }
    switch (cross_a) { case 1: c = LV_FLEX_ALIGN_CENTER; break; case 2: c = LV_FLEX_ALIGN_END; break; }
    lv_obj_set_flex_align(h2obj(widget), m, c, c);
}

void lvgl_wm_widget_set_gap(uint32_t widget, int gap)
{
    if (!widget) return;
    lv_obj_set_style_pad_row(h2obj(widget), gap, LV_PART_MAIN);
    lv_obj_set_style_pad_column(h2obj(widget), gap, LV_PART_MAIN);
}

void lvgl_wm_widget_set_flex_grow(uint32_t widget, int grow)
{
    if (!widget) return;
    lv_obj_set_flex_grow(h2obj(widget), (uint8_t)grow);
}

void lvgl_wm_widget_set_scrollable(uint32_t widget, bool scrollable)
{
    if (!widget) return;
    if (scrollable) lv_obj_add_flag(h2obj(widget), LV_OBJ_FLAG_SCROLLABLE);
    else lv_obj_clear_flag(h2obj(widget), LV_OBJ_FLAG_SCROLLABLE);
}

void lvgl_wm_widget_set_padding(uint32_t widget, int t, int r, int b, int l)
{
    if (!widget) return;
    lv_obj_t *o = h2obj(widget);
    lv_obj_set_style_pad_top(o, t, LV_PART_MAIN);
    lv_obj_set_style_pad_right(o, r, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(o, b, LV_PART_MAIN);
    lv_obj_set_style_pad_left(o, l, LV_PART_MAIN);
}

void lvgl_wm_widget_set_border_width(uint32_t widget, int w)
{
    if (!widget) return;
    lv_obj_set_style_border_width(h2obj(widget), w, LV_PART_MAIN);
}

void lvgl_wm_widget_set_radius(uint32_t widget, int r)
{
    if (!widget) return;
    lv_obj_set_style_radius(h2obj(widget), r, LV_PART_MAIN);
}

/* ── Widget API: events ──────────────────────────────────────────────── */

typedef struct {
    void (*cb)(uint32_t, int, void *);
    void *user_data;
    int event_type;
} lvgl_wm_event_ctx_t;

static void lvgl_wm_lvgl_event_bridge(lv_event_t *e)
{
    lvgl_wm_event_ctx_t *ctx = (lvgl_wm_event_ctx_t *)lv_event_get_user_data(e);
    if (ctx && ctx->cb) {
        lv_obj_t *target = lv_event_get_target(e);
        ctx->cb(obj2h(target), ctx->event_type, ctx->user_data);
    }
}

void lvgl_wm_widget_on_event(uint32_t widget, int event_type, void (*cb)(uint32_t, int, void*), void *ud)
{
    if (!widget || !cb) return;
    // Allocate context — lives for the widget's lifetime (leaked on destroy, acceptable)
    lvgl_wm_event_ctx_t *ctx = (lvgl_wm_event_ctx_t *)lv_malloc(sizeof(lvgl_wm_event_ctx_t));
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
    lv_obj_add_event_cb(h2obj(widget), lvgl_wm_lvgl_event_bridge, code, ctx);
}

/* ── Widget API: text input ──────────────────────────────────────────── */

void lvgl_wm_widget_set_password_mode(uint32_t widget, bool pw)
{
    if (!widget) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_password_mode(h2obj(widget), pw);
}

void lvgl_wm_widget_set_one_line(uint32_t widget, bool one)
{
    if (!widget) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_one_line(h2obj(widget), one);
}

void lvgl_wm_widget_set_placeholder(uint32_t widget, const char *text)
{
    if (!widget || !text) return;
    if (lv_obj_check_type(h2obj(widget), &lv_textarea_class))
        lv_textarea_set_placeholder_text(h2obj(widget), text);
}

/* ── Widget API: theme ───────────────────────────────────────────────── */

static uint32_t color_to_rgb(lv_color_t c) {
    return ((uint32_t)c.red << 16) | ((uint32_t)c.green << 8) | c.blue;
}

uint32_t lvgl_wm_widget_theme_primary(void)        { return color_to_rgb(theme_get_colors()->primary); }
uint32_t lvgl_wm_widget_theme_bg(void)             { return color_to_rgb(theme_get_colors()->bg); }
uint32_t lvgl_wm_widget_theme_surface(void)        { return color_to_rgb(theme_get_colors()->surface); }
uint32_t lvgl_wm_widget_theme_text(void)           { return color_to_rgb(theme_get_colors()->text); }
uint32_t lvgl_wm_widget_theme_text_secondary(void) { return color_to_rgb(theme_get_colors()->text_secondary); }
