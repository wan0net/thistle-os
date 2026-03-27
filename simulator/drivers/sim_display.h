#pragma once

#include "hal/display.h"

const hal_display_driver_t *sim_display_get(void);

/* Initialize SDL2 window (called internally by display init) */
void sim_display_sdl_init(void);

/* Configure display resolution — must be called before sim_display_get() is registered */
void sim_display_set_resolution(int width, int height);

/* Set the window title to include the device name */
void sim_display_set_title(const char *device_name);
