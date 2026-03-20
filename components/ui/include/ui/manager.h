#pragma once

#include "esp_err.h"
#include "lvgl.h"

/* Initialize LVGL, create display driver, set up screen layout */
esp_err_t ui_manager_init(void);

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
