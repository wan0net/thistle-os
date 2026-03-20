#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef enum {
    HAL_DISPLAY_TYPE_LCD,
    HAL_DISPLAY_TYPE_EPAPER,
} hal_display_type_t;

typedef enum {
    HAL_DISPLAY_REFRESH_FULL,      // Full refresh (e-paper: no ghosting, slow)
    HAL_DISPLAY_REFRESH_PARTIAL,   // Partial area refresh
    HAL_DISPLAY_REFRESH_FAST,      // Fast refresh (e-paper: some ghosting)
} hal_display_refresh_mode_t;

typedef struct {
    uint16_t x1, y1, x2, y2;
} hal_area_t;

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*flush)(const hal_area_t *area, const uint8_t *color_data);
    esp_err_t (*set_brightness)(uint8_t percent);
    esp_err_t (*sleep)(bool enter);
    esp_err_t (*set_refresh_mode)(hal_display_refresh_mode_t mode);
    uint16_t width;
    uint16_t height;
    hal_display_type_t type;
    const char *name;
} hal_display_driver_t;
