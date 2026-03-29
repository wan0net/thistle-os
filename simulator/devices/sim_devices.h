/*
 * Simulator device models — register on virtual I2C/SPI bus.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

#include <stdint.h>
#include <stdbool.h>

/* Register a device model on the virtual I2C bus */
void dev_pcf8563_register(int bus_index, uint16_t addr);
void dev_qmi8658c_register(int bus_index, uint16_t addr);
void dev_tca8418_register(int bus_index, uint16_t addr);
void dev_cst328_register(int bus_index, uint16_t addr);
void dev_ltr553_register(int bus_index, uint16_t addr);

/* Input injection (called from SDL event handler) */
void dev_tca8418_inject_key(uint8_t keycode, bool press);
void dev_cst328_inject_touch(uint16_t x, uint16_t y, bool down);

/* Sensor value injection */
void dev_qmi8658c_set_accel(float x, float y, float z);
void dev_qmi8658c_set_gyro(float x, float y, float z);
void dev_ltr553_set_lux(uint16_t lux);
void dev_ltr553_set_proximity(uint16_t prox);
