// SPDX-License-Identifier: BSD-3-Clause
#pragma once

#include "thistle/display_server.h"

/* Get the LVGL window manager vtable.
 * Register with: display_server_register_wm(lvgl_wm_get()); */
const display_server_wm_t *lvgl_wm_get(void);
