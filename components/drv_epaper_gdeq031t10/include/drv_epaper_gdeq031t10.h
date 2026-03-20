// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — GDEQ031T10 e-paper display driver header
#pragma once

#include "hal/display.h"
#include "driver/spi_master.h"
#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    spi_host_device_t spi_host;
    gpio_num_t        pin_cs;
    gpio_num_t        pin_dc;
    gpio_num_t        pin_rst;
    gpio_num_t        pin_busy;
    int               spi_clock_hz;
} epaper_gdeq031t10_config_t;

/* Return the driver vtable instance. */
const hal_display_driver_t *drv_epaper_gdeq031t10_get(void);

#ifdef __cplusplus
}
#endif
