#pragma once

#include "hal/input.h"

const hal_input_driver_t *sim_input_get(void);

/* Call from main loop to pump SDL events -> HAL input events */
void sim_input_poll_sdl(void);

/* Inject one touch edge in native display coordinates. */
bool sim_input_inject_touch(uint16_t x, uint16_t y, bool pressed);
