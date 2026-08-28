#pragma once

#include "hal/display.h"

const hal_display_driver_t *sim_display_get(void);

/* Initialize SDL2 window (called internally by display init) */
void sim_display_sdl_init(void);

/* Configure display resolution — must be called before sim_display_get() is registered */
void sim_display_set_resolution(int width, int height);

/* Select strict 1-bit input and e-paper HAL capabilities. */
void sim_display_set_epaper(bool enabled);

/* Set the window title to include the device name */
void sim_display_set_title(const char *device_name);

/* Get the current display scale factor (2, 3, or 4) */
int sim_display_get_scale(void);

/* Export the current native-resolution RGB565 framebuffer as a binary PPM. */
bool sim_display_save_ppm(const char *path);
