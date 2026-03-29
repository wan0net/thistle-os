/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Simulator IMU HAL driver
 */
#pragma once
#include "hal/imu.h"

const hal_imu_driver_t *sim_imu_get(void);
void sim_imu_set_accel(float x, float y, float z);
void sim_imu_set_gyro(float x, float y, float z);
