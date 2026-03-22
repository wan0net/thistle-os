// SPDX-License-Identifier: BSD-3-Clause
#pragma once

#include "thistle/display_server.h"

/* Get the LVGL e-paper window manager vtable.
 * Uses deferred refresh with debounce, no splash screen. */
const display_server_wm_t *lvgl_epaper_wm_get(void);

/* Get the LVGL LCD window manager vtable.
 * Uses direct flush, shows splash screen on init. */
const display_server_wm_t *lvgl_lcd_wm_get(void);
