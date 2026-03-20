// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Bosch BHI260AP IMU driver header
#pragma once

#include "hal/imu.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    i2c_master_bus_handle_t i2c_bus;
    uint8_t                 i2c_addr;   // Default: 0x28
    gpio_num_t              pin_int;    // Interrupt pin
} imu_bhi260ap_config_t;

/* Return the driver vtable instance. */
const hal_imu_driver_t *drv_imu_bhi260ap_get(void);

#ifdef __cplusplus
}
#endif
