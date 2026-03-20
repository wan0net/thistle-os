// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ST7789 LCD display driver (stub)
#include "drv_lcd_st7789.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "st7789";

static lcd_st7789_config_t s_config;

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t st7789_init(const void *config)
{
    // TODO: Configure SPI bus, add device, send init command sequence,
    //       enable backlight via LEDC PWM on pin_bl.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    memcpy(&s_config, config, sizeof(s_config));
    return ESP_ERR_NOT_SUPPORTED;
}

static void st7789_deinit(void)
{
    // TODO: Remove SPI device, deinitialise SPI bus, disable backlight.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
}

static esp_err_t st7789_flush(const hal_area_t *area, const uint8_t *color_data)
{
    // TODO: Set column/row address window (CASET/RASET), send RAMWR, DMA transfer.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t st7789_set_brightness(uint8_t percent)
{
    // TODO: Set LEDC duty cycle on pin_bl proportional to percent (0–100).
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t st7789_sleep(bool enter)
{
    // TODO: Send SLPIN (0x10) or SLPOUT (0x11) command.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t st7789_set_refresh_mode(hal_display_refresh_mode_t mode)
{
    // TODO: LCD does not differentiate partial vs full in the same way as
    //       e-paper; could configure frame-rate or tearing-effect pin here.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_display_driver_t s_vtable = {
    .init             = st7789_init,
    .deinit           = st7789_deinit,
    .flush            = st7789_flush,
    .set_brightness   = st7789_set_brightness,
    .sleep            = st7789_sleep,
    .set_refresh_mode = st7789_set_refresh_mode,
    .width            = 320,
    .height           = 240,
    .type             = HAL_DISPLAY_TYPE_LCD,
    .name             = "ST7789",
};

const hal_display_driver_t *drv_lcd_st7789_get(void)
{
    return &s_vtable;
}
