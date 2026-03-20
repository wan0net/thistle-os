#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    double latitude;
    double longitude;
    float altitude_m;
    float speed_kmh;
    float heading_deg;
    uint8_t satellites;
    bool fix_valid;
    uint32_t timestamp;   // UTC time as unix timestamp
} hal_gps_position_t;

typedef void (*hal_gps_cb_t)(const hal_gps_position_t *pos, void *user_data);

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*enable)(void);
    esp_err_t (*disable)(void);
    esp_err_t (*get_position)(hal_gps_position_t *pos);
    esp_err_t (*register_callback)(hal_gps_cb_t cb, void *user_data);
    esp_err_t (*sleep)(bool enter);
    const char *name;
} hal_gps_driver_t;
