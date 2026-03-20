// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ST7789 LCD display driver header
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
    gpio_num_t        pin_bl;       // Backlight PWM pin
    int               spi_clock_hz;
} lcd_st7789_config_t;

/* Return the driver vtable instance. */
const hal_display_driver_t *drv_lcd_st7789_get(void);

#ifdef __cplusplus
}
#endif
