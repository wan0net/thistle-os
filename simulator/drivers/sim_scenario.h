/*
 * Simulator scenario engine — loads initial state for fake HAL drivers.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>

/* Load a scenario from a JSON file. Returns 0 on success, -1 on failure.
 * If path is NULL, defaults are used (no-op). */
int sim_scenario_load(const char *path);

/* Accessors — return defaults if no scenario loaded */
void sim_scenario_get_power(uint16_t *voltage_mv, uint8_t *percent, int *state);
void sim_scenario_get_gps(double *lat, double *lon, float *alt, uint8_t *sats, bool *fix);
void sim_scenario_get_imu(float accel[3], float gyro[3]);
