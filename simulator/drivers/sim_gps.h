/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Simulator GPS HAL driver
 */
#pragma once
#include "hal/gps.h"

const hal_gps_driver_t *sim_gps_get(void);
void sim_gps_set_position(double lat, double lon, float alt, uint8_t sats, bool fix);
