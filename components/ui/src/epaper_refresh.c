#include "ui/epaper_refresh.h"
#include "esp_log.h"
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "epaper_ref";

static uint16_t s_disp_w   = 0;
static uint16_t s_disp_h   = 0;
static bool     s_dirty     = false;
static uint32_t s_count     = 0;

/* Current dirty bounding box */
static uint16_t s_x1 = 0;
static uint16_t s_y1 = 0;
static uint16_t s_x2 = 0;
static uint16_t s_y2 = 0;

esp_err_t epaper_refresh_init(uint16_t display_width, uint16_t display_height)
{
    if (display_width == 0 || display_height == 0) {
        ESP_LOGE(TAG, "invalid display dimensions: %dx%d", display_width, display_height);
        return ESP_ERR_INVALID_ARG;
    }

    s_disp_w = display_width;
    s_disp_h = display_height;
    s_dirty  = false;
    s_count  = 0;
    s_x1 = 0;
    s_y1 = 0;
    s_x2 = 0;
    s_y2 = 0;

    ESP_LOGI(TAG, "e-paper refresh tracker initialized (%dx%d)", display_width, display_height);
    return ESP_OK;
}

void epaper_refresh_mark_dirty(uint16_t x1, uint16_t y1, uint16_t x2, uint16_t y2)
{
    /* Clamp to display bounds */
    if (x2 >= s_disp_w) { x2 = s_disp_w - 1; }
    if (y2 >= s_disp_h) { y2 = s_disp_h - 1; }
    if (x1 > x2 || y1 > y2) {
        return;
    }

    if (!s_dirty) {
        /* First dirty region — initialise bounding box */
        s_x1 = x1;
        s_y1 = y1;
        s_x2 = x2;
        s_y2 = y2;
        s_dirty = true;
    } else {
        /* Expand existing bounding box */
        if (x1 < s_x1) { s_x1 = x1; }
        if (y1 < s_y1) { s_y1 = y1; }
        if (x2 > s_x2) { s_x2 = x2; }
        if (y2 > s_y2) { s_y2 = y2; }
    }
}

void epaper_refresh_mark_full(void)
{
    s_x1    = 0;
    s_y1    = 0;
    s_x2    = (s_disp_w > 0) ? (s_disp_w - 1) : 0;
    s_y2    = (s_disp_h > 0) ? (s_disp_h - 1) : 0;
    s_dirty = true;
}

bool epaper_refresh_is_dirty(void)
{
    return s_dirty;
}

void epaper_refresh_get_bounds(uint16_t *x1, uint16_t *y1, uint16_t *x2, uint16_t *y2)
{
    if (x1 != NULL) { *x1 = s_x1; }
    if (y1 != NULL) { *y1 = s_y1; }
    if (x2 != NULL) { *x2 = s_x2; }
    if (y2 != NULL) { *y2 = s_y2; }
}

void epaper_refresh_clear(void)
{
    s_dirty = false;
    s_x1    = 0;
    s_y1    = 0;
    s_x2    = 0;
    s_y2    = 0;
    s_count++;
}

uint32_t epaper_refresh_get_count(void)
{
    return s_count;
}
