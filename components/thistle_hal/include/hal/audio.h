#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

typedef struct {
    uint32_t sample_rate;
    uint8_t bits_per_sample;
    uint8_t channels;
} hal_audio_config_t;

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*play)(const uint8_t *data, size_t len);
    esp_err_t (*stop)(void);
    esp_err_t (*set_volume)(uint8_t percent);
    esp_err_t (*configure)(const hal_audio_config_t *cfg);
    const char *name;
} hal_audio_driver_t;
