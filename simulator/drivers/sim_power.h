// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Simulator — fake power HAL driver
#pragma once

#include "hal/power.h"

const hal_power_driver_t *sim_power_get(void);
void sim_power_set(uint16_t voltage_mv, uint8_t percent, hal_power_state_t state);
