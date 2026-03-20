// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — LTR-553ALS light/proximity sensor driver header
#pragma once

#include "esp_err.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    i2c_master_bus_handle_t i2c_bus;
    uint8_t                 i2c_addr;   // Default: 0x23
    gpio_num_t              pin_int;    // Interrupt pin
} light_ltr553_config_t;

typedef struct {
    uint16_t als_lux;       // Ambient light in lux
    uint16_t ps_proximity;  // Proximity value (0–2047)
} ltr553_data_t;

esp_err_t drv_ltr553_init(const light_ltr553_config_t *config);
void      drv_ltr553_deinit(void);
esp_err_t drv_ltr553_read(ltr553_data_t *data);

#ifdef __cplusplus
}
#endif
