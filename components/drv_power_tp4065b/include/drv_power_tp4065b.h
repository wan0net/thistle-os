// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TP4065B power/battery driver header
#pragma once

#include "hal/power.h"
#include "driver/gpio.h"
#include "esp_adc/adc_oneshot.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    adc_channel_t adc_channel;      // ADC channel for battery voltage sense
    gpio_num_t    pin_charge_status; // GPIO: low = charging, high = done/not charging
} power_tp4065b_config_t;

/* Return the driver vtable instance. */
const hal_power_driver_t *drv_power_tp4065b_get(void);

#ifdef __cplusplus
}
#endif
