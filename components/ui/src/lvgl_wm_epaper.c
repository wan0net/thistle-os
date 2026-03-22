// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * lvgl_wm_epaper.c — E-paper LVGL window manager for ThistleOS
 *
 * Initializes the UI manager with the e-paper flush callback and
 * deferred refresh enabled. No splash screen (every refresh is
 * expensive on e-paper).
 */

#include "thistle/display_server.h"
#include "ui/manager.h"
#include "ui/lvgl_wm.h"
#include "esp_log.h"
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "lvgl_wm_epaper";

/* ── Extern widget functions from lvgl_wm.c ──────────────────────────── */

extern uint32_t    lvgl_wm_widget_get_app_root(void);
extern uint32_t    lvgl_wm_widget_create_container(uint32_t parent);
extern uint32_t    lvgl_wm_widget_create_label(uint32_t parent, const char *text);
extern uint32_t    lvgl_wm_widget_create_button(uint32_t parent, const char *text);
extern uint32_t    lvgl_wm_widget_create_text_input(uint32_t parent, const char *placeholder);
extern void        lvgl_wm_widget_destroy(uint32_t widget);
extern void        lvgl_wm_widget_set_text(uint32_t widget, const char *text);
extern const char *lvgl_wm_widget_get_text(uint32_t widget);
extern void        lvgl_wm_widget_set_size(uint32_t widget, int w, int h);
extern void        lvgl_wm_widget_set_pos(uint32_t widget, int x, int y);
extern void        lvgl_wm_widget_set_visible(uint32_t widget, bool visible);
extern void        lvgl_wm_widget_set_bg_color(uint32_t widget, uint32_t color);
extern void        lvgl_wm_widget_set_text_color(uint32_t widget, uint32_t color);
extern void        lvgl_wm_widget_set_font_size(uint32_t widget, int size);
extern void        lvgl_wm_widget_set_layout(uint32_t widget, int layout);
extern void        lvgl_wm_widget_set_align(uint32_t widget, int main_a, int cross_a);
extern void        lvgl_wm_widget_set_gap(uint32_t widget, int gap);
extern void        lvgl_wm_widget_set_flex_grow(uint32_t widget, int grow);
extern void        lvgl_wm_widget_set_scrollable(uint32_t widget, bool scrollable);
extern void        lvgl_wm_widget_set_padding(uint32_t widget, int t, int r, int b, int l);
extern void        lvgl_wm_widget_set_border_width(uint32_t widget, int w);
extern void        lvgl_wm_widget_set_radius(uint32_t widget, int r);
extern void        lvgl_wm_widget_on_event(uint32_t widget, int event_type, void (*cb)(uint32_t, int, void*), void *ud);
extern void        lvgl_wm_widget_set_password_mode(uint32_t widget, bool pw);
extern void        lvgl_wm_widget_set_one_line(uint32_t widget, bool one_line);
extern void        lvgl_wm_widget_set_placeholder(uint32_t widget, const char *text);
extern uint32_t    lvgl_wm_widget_theme_primary(void);
extern uint32_t    lvgl_wm_widget_theme_bg(void);
extern uint32_t    lvgl_wm_widget_theme_surface(void);
extern uint32_t    lvgl_wm_widget_theme_text(void);
extern uint32_t    lvgl_wm_widget_theme_text_secondary(void);

/* ── WM lifecycle callbacks ──────────────────────────────────────────── */

static esp_err_t lvgl_epaper_wm_init(void)
{
    ESP_LOGI(TAG, "LVGL e-paper window manager initializing");
    esp_err_t ret = ui_manager_init(ui_flush_cb_epaper, true);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ui_manager_init failed: %s", esp_err_to_name(ret));
        return ret;
    }
    ESP_LOGI(TAG, "LVGL e-paper window manager ready");
    return ESP_OK;
}

static void lvgl_epaper_wm_deinit(void) {}
static void lvgl_epaper_wm_render(void) {}
static void lvgl_epaper_wm_on_theme_changed(const char *p) { (void)p; }
static void lvgl_epaper_wm_on_app_launched(const char *id, surface_id_t s) { (void)id; (void)s; }
static void lvgl_epaper_wm_on_app_stopped(const char *id) { (void)id; }
static void lvgl_epaper_wm_on_app_switched(const char *id) { (void)id; }
static bool lvgl_epaper_wm_on_input(const hal_input_event_t *e) { (void)e; return false; }

/* ── Public WM vtable ───────────────────────────────────────────────── */

static const display_server_wm_t s_lvgl_epaper_wm = {
    /* Lifecycle */
    .init              = lvgl_epaper_wm_init,
    .deinit            = lvgl_epaper_wm_deinit,
    .render            = lvgl_epaper_wm_render,
    .on_theme_changed  = lvgl_epaper_wm_on_theme_changed,
    .on_app_launched   = lvgl_epaper_wm_on_app_launched,
    .on_app_stopped    = lvgl_epaper_wm_on_app_stopped,
    .on_app_switched   = lvgl_epaper_wm_on_app_switched,
    .on_input          = lvgl_epaper_wm_on_input,

    /* Widget API */
    .widget_get_app_root     = lvgl_wm_widget_get_app_root,
    .widget_create_container = lvgl_wm_widget_create_container,
    .widget_create_label     = lvgl_wm_widget_create_label,
    .widget_create_button    = lvgl_wm_widget_create_button,
    .widget_create_text_input = lvgl_wm_widget_create_text_input,
    .widget_destroy          = lvgl_wm_widget_destroy,
    .widget_set_text         = lvgl_wm_widget_set_text,
    .widget_get_text         = lvgl_wm_widget_get_text,
    .widget_set_size         = lvgl_wm_widget_set_size,
    .widget_set_pos          = lvgl_wm_widget_set_pos,
    .widget_set_visible      = lvgl_wm_widget_set_visible,
    .widget_set_bg_color     = lvgl_wm_widget_set_bg_color,
    .widget_set_text_color   = lvgl_wm_widget_set_text_color,
    .widget_set_font_size    = lvgl_wm_widget_set_font_size,
    .widget_set_layout       = lvgl_wm_widget_set_layout,
    .widget_set_align        = lvgl_wm_widget_set_align,
    .widget_set_gap          = lvgl_wm_widget_set_gap,
    .widget_set_flex_grow    = lvgl_wm_widget_set_flex_grow,
    .widget_set_scrollable   = lvgl_wm_widget_set_scrollable,
    .widget_set_padding      = lvgl_wm_widget_set_padding,
    .widget_set_border_width = lvgl_wm_widget_set_border_width,
    .widget_set_radius       = lvgl_wm_widget_set_radius,
    .widget_on_event         = lvgl_wm_widget_on_event,
    .widget_set_password_mode = lvgl_wm_widget_set_password_mode,
    .widget_set_one_line     = lvgl_wm_widget_set_one_line,
    .widget_set_placeholder  = lvgl_wm_widget_set_placeholder,
    .widget_theme_primary    = lvgl_wm_widget_theme_primary,
    .widget_theme_bg         = lvgl_wm_widget_theme_bg,
    .widget_theme_surface    = lvgl_wm_widget_theme_surface,
    .widget_theme_text       = lvgl_wm_widget_theme_text,
    .widget_theme_text_secondary = lvgl_wm_widget_theme_text_secondary,

    /* Info */
    .name              = "lvgl-epaper-wm",
    .version           = "2.0.0",
};

const display_server_wm_t *lvgl_epaper_wm_get(void)
{
    return &s_lvgl_epaper_wm;
}
