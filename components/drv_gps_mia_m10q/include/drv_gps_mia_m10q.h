// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — U-blox MIA-M10Q GPS driver header
#pragma once

#include "hal/gps.h"
#include "driver/uart.h"
#include "driver/gpio.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uart_port_t uart_num;
    gpio_num_t  pin_tx;
    gpio_num_t  pin_rx;
    uint32_t    baud_rate;  // Default: 9600
} gps_mia_m10q_config_t;

/* Return the driver vtable instance. */
const hal_gps_driver_t *drv_gps_mia_m10q_get(void);

#ifdef __cplusplus
}
#endif
