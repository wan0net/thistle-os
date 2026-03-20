// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — PCM5102A audio DAC driver header
#pragma once

#include "hal/audio.h"
#include "driver/gpio.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    int        i2s_num;    // I2S port number
    gpio_num_t pin_bck;    // Bit clock
    gpio_num_t pin_ws;     // Word select (LR clock)
    gpio_num_t pin_data;   // Serial data out
} audio_pcm5102a_config_t;

/* Return the driver vtable instance. */
const hal_audio_driver_t *drv_audio_pcm5102a_get(void);

#ifdef __cplusplus
}
#endif
