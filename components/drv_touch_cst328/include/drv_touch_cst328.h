// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CST328 capacitive touch driver header
#pragma once

#include "hal/input.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    i2c_master_bus_handle_t i2c_bus;
    uint8_t                 i2c_addr;   /* Default 0x1A */
    gpio_num_t              pin_int;    /* Interrupt pin. GPIO_NUM_NC to disable. */
    gpio_num_t              pin_rst;    /* Reset pin.    GPIO_NUM_NC to disable. */
    uint16_t                max_x;      /* Panel width  (e.g. 320) */
    uint16_t                max_y;      /* Panel height (e.g. 240) */
} touch_cst328_config_t;

/* Return the driver vtable instance. */
const hal_input_driver_t *drv_touch_cst328_get(void);

#ifdef __cplusplus
}
#endif
