#pragma once

#include "esp_err.h"
#include "lvgl.h"
#include <stdint.h>

typedef struct {
    lv_color_t primary;
    lv_color_t secondary;
    lv_color_t bg;
    lv_color_t surface;
    lv_color_t text;
    lv_color_t text_secondary;
    uint8_t radius;
    uint8_t padding;
} theme_colors_t;

/* Initialize theme engine with default theme */
esp_err_t theme_init(lv_display_t *disp);

/* Load a theme from JSON file path */
esp_err_t theme_load(const char *json_path);

/* Get current theme colors */
const theme_colors_t *theme_get_colors(void);

/* Apply current theme to an LVGL display */
esp_err_t theme_apply(lv_display_t *disp);
