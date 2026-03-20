#pragma once

#include "hal/display.h"

const hal_display_driver_t *sim_display_get(void);

/* Initialize SDL2 window (called internally by display init) */
void sim_display_sdl_init(void);
