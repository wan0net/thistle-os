// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * lvgl_wm.c — LVGL-based window manager for ThistleOS
 *
 * Wraps the existing LVGL UI framework (manager.c, statusbar.c, theme.c)
 * as a display_server_wm_t implementation. This is the default WM.
 *
 * In the future, this will be compiled as a standalone .wm.elf loaded
 * from SPIFFS. For now, it's compiled into the kernel for backward compat.
 */

#include "thistle/display_server.h"
#include "ui/manager.h"
#include "esp_log.h"

static const char *TAG = "lvgl_wm";

/* ── WM vtable callbacks ────────────────────────────────────────────── */

static esp_err_t lvgl_wm_init(void)
{
    ESP_LOGI(TAG, "LVGL window manager initializing");

    /* ui_manager_init() sets up LVGL, display, input devices, status bar,
     * theme engine, splash screen, and the LVGL timer task. */
    esp_err_t ret = ui_manager_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ui_manager_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "LVGL window manager ready");
    return ESP_OK;
}

static void lvgl_wm_deinit(void)
{
    ESP_LOGI(TAG, "LVGL window manager deinitializing");
    /* LVGL doesn't have a clean deinit — this would need to destroy
     * all objects and free the display. For now, it's a no-op since
     * WM switching requires a reboot. */
}

static void lvgl_wm_render(void)
{
    /* LVGL's rendering is driven by its own timer task (lvgl_task in manager.c).
     * The display server tick calls this, but LVGL is already self-driving.
     * Nothing to do here — the LVGL task handles lv_timer_handler(). */
}

static void lvgl_wm_on_theme_changed(const char *theme_path)
{
    /* Future: reload theme from theme_path */
    (void)theme_path;
}

static void lvgl_wm_on_app_launched(const char *app_id, surface_id_t surface)
{
    (void)app_id;
    (void)surface;
    /* The existing app_manager + ui_manager already handle this */
}

static void lvgl_wm_on_app_stopped(const char *app_id)
{
    (void)app_id;
}

static void lvgl_wm_on_app_switched(const char *app_id)
{
    (void)app_id;
}

static bool lvgl_wm_on_input(const hal_input_event_t *event)
{
    (void)event;
    /* LVGL input is already wired via HAL callbacks in manager.c.
     * Return false to let the display server continue routing. */
    return false;
}

/* ── Public WM vtable ───────────────────────────────────────────────── */

static const display_server_wm_t s_lvgl_wm = {
    .init              = lvgl_wm_init,
    .deinit            = lvgl_wm_deinit,
    .render            = lvgl_wm_render,
    .on_theme_changed  = lvgl_wm_on_theme_changed,
    .on_app_launched   = lvgl_wm_on_app_launched,
    .on_app_stopped    = lvgl_wm_on_app_stopped,
    .on_app_switched   = lvgl_wm_on_app_switched,
    .on_input          = lvgl_wm_on_input,
    .name              = "lvgl-wm",
    .version           = "1.0.0",
};

const display_server_wm_t *lvgl_wm_get(void)
{
    return &s_lvgl_wm;
}
