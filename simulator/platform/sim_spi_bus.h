/*
 * Virtual SPI bus for simulator.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>

#define SIM_SPI_MAX_BUSES   2
#define SIM_SPI_MAX_DEVICES 4

/* Initialize the virtual SPI bus system */
void sim_spi_bus_init(void);

/* Get opaque bus handle for HAL registration */
void *sim_spi_bus_get(int index);
