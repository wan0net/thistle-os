// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SX1262 LoRa radio driver header
#pragma once

#include "hal/radio.h"
#include "driver/spi_master.h"
#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    spi_host_device_t spi_host;
    gpio_num_t        pin_cs;
    gpio_num_t        pin_reset;
    gpio_num_t        pin_busy;
    gpio_num_t        pin_dio1;     // Interrupt line
    int               spi_clock_hz;
} radio_sx1262_config_t;

/* Return the driver vtable instance. */
const hal_radio_driver_t *drv_radio_sx1262_get(void);

#ifdef __cplusplus
}
#endif
