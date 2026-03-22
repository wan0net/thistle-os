#pragma once

#include "esp_err.h"
#include "lvgl.h"

/* Flush callback type — WM variants pass the appropriate one to init */
typedef void (*ui_flush_fn_t)(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map);

/* Initialize LVGL, create display driver, set up screen layout.
 * flush_cb: display flush callback (epaper or LCD variant)
 * use_deferred_refresh: true for e-paper (debounce panel refresh) */
esp_err_t ui_manager_init(ui_flush_fn_t flush_cb, bool use_deferred_refresh);

/* Flush callbacks — defined in manager.c, used by WM variants */
void ui_flush_cb_epaper(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map);
void ui_flush_cb_lcd(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map);

/* Start the LVGL render loop. Call AFTER all UI objects are created. */
esp_err_t ui_manager_start(void);

/* Get the app content area (apps create their UI as children of this) */
lv_obj_t *ui_manager_get_app_area(void);

/* Get the root screen object */
lv_obj_t *ui_manager_get_screen(void);

/* Request a display refresh (for e-paper: batches and defers) */
void ui_manager_request_refresh(void);

/* Force an immediate full refresh */
void ui_manager_force_refresh(void);

/* Lock/unlock LVGL mutex (must hold when modifying LVGL objects) */
void ui_manager_lock(void);
void ui_manager_unlock(void);

/* Show splash screen overlay — auto-hides after duration_ms milliseconds */
void ui_manager_show_splash(uint32_t duration_ms);
