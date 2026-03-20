// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TCA8418 keyboard matrix driver header
#pragma once

#include "hal/input.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    i2c_master_bus_handle_t i2c_bus;
    uint8_t                 i2c_addr;   /* Default 0x34 */
    gpio_num_t              pin_int;    /* Interrupt pin, active low. GPIO_NUM_NC to disable. */
} kbd_tca8418_config_t;

/* Return the driver vtable instance. */
const hal_input_driver_t *drv_kbd_tca8418_get(void);

#ifdef __cplusplus
}
#endif
