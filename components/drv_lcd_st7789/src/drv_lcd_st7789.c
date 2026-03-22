// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ST7789 LCD display driver (esp_lcd wrapper)
//
// Wraps ESP-IDF's built-in esp_lcd component (esp_lcd_new_panel_st7789) behind
// the ThistleOS HAL display vtable.  SPI wiring: MOSI, SCK on the shared SPI
// host; CS, DC, RST are passed via lcd_st7789_config_t.  Backlight is driven
// via LEDC PWM on pin_bl.

#include "drv_lcd_st7789.h"

#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_vendor.h"
#include "esp_lcd_panel_ops.h"
#include "driver/ledc.h"
#include "esp_log.h"
#include "esp_err.h"

#include <string.h>

static const char *TAG = "st7789";

/* ── Panel geometry ─────────────────────────────────────────────────────── */
#define LCD_WIDTH  320
#define LCD_HEIGHT 240

/* ── LEDC backlight ──────────────────────────────────────────────────────── */
#define BL_LEDC_TIMER      LEDC_TIMER_0
#define BL_LEDC_MODE       LEDC_LOW_SPEED_MODE
#define BL_LEDC_CHANNEL    LEDC_CHANNEL_0
#define BL_LEDC_FREQ_HZ    5000
#define BL_LEDC_DUTY_RES   LEDC_TIMER_8_BIT   /* 0–255 duty range */
#define BL_LEDC_MAX_DUTY   255

/* ── Driver state ────────────────────────────────────────────────────────── */
static struct {
    lcd_st7789_config_t         cfg;
    esp_lcd_panel_io_handle_t   io;
    esp_lcd_panel_handle_t      panel;
    bool                        initialised;
    uint8_t                     brightness; /* last non-zero brightness percent */
} s_lcd;

/* ── Backlight (LEDC) ────────────────────────────────────────────────────── */

static esp_err_t bl_init(gpio_num_t pin)
{
    if (pin == GPIO_NUM_NC) return ESP_OK;

    ledc_timer_config_t timer_cfg = {
        .speed_mode      = BL_LEDC_MODE,
        .timer_num       = BL_LEDC_TIMER,
        .duty_resolution = BL_LEDC_DUTY_RES,
        .freq_hz         = BL_LEDC_FREQ_HZ,
        .clk_cfg         = LEDC_AUTO_CLK,
    };
    esp_err_t ret = ledc_timer_config(&timer_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "LEDC timer config failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ledc_channel_config_t ch_cfg = {
        .speed_mode = BL_LEDC_MODE,
        .channel    = BL_LEDC_CHANNEL,
        .timer_sel  = BL_LEDC_TIMER,
        .intr_type  = LEDC_INTR_DISABLE,
        .gpio_num   = pin,
        .duty       = 0,
        .hpoint     = 0,
    };
    ret = ledc_channel_config(&ch_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "LEDC channel config failed: %s", esp_err_to_name(ret));
    }
    return ret;
}

static esp_err_t bl_set_duty(uint8_t percent)
{
    if (s_lcd.cfg.pin_bl == GPIO_NUM_NC) return ESP_OK;

    uint32_t duty = (uint32_t)percent * BL_LEDC_MAX_DUTY / 100;
    esp_err_t ret = ledc_set_duty(BL_LEDC_MODE, BL_LEDC_CHANNEL, duty);
    if (ret != ESP_OK) return ret;
    return ledc_update_duty(BL_LEDC_MODE, BL_LEDC_CHANNEL);
}

/* ── Init ────────────────────────────────────────────────────────────────── */

static esp_err_t st7789_init(const void *config)
{
    if (!config) {
        ESP_LOGE(TAG, "init: NULL config");
        return ESP_ERR_INVALID_ARG;
    }
    if (s_lcd.initialised) {
        ESP_LOGW(TAG, "already initialised");
        return ESP_OK;
    }

    memcpy(&s_lcd.cfg, config, sizeof(lcd_st7789_config_t));

    /* ── Backlight init (off until display is ready) ── */
    esp_err_t ret = bl_init(s_lcd.cfg.pin_bl);
    if (ret != ESP_OK) return ret;

    /* ── Create SPI panel IO handle ── */
    esp_lcd_panel_io_spi_config_t io_config = {
        .dc_gpio_num       = s_lcd.cfg.pin_dc,
        .cs_gpio_num       = s_lcd.cfg.pin_cs,
        .pclk_hz           = s_lcd.cfg.spi_clock_hz > 0
                                 ? s_lcd.cfg.spi_clock_hz
                                 : 40000000,
        .lcd_cmd_bits      = 8,
        .lcd_param_bits    = 8,
        .spi_mode          = 0,
        .trans_queue_depth = 10,
    };

    ret = esp_lcd_new_panel_io_spi(
        (esp_lcd_spi_bus_handle_t)s_lcd.cfg.spi_host,
        &io_config, &s_lcd.io);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "SPI panel IO create failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* ── Create ST7789 panel handle ── */
    esp_lcd_panel_dev_config_t panel_config = {
        .reset_gpio_num = s_lcd.cfg.pin_rst,
        .rgb_ele_order  = LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = 16,
    };

    ret = esp_lcd_new_panel_st7789(s_lcd.io, &panel_config, &s_lcd.panel);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ST7789 panel create failed: %s", esp_err_to_name(ret));
        esp_lcd_panel_io_del(s_lcd.io);
        s_lcd.io = NULL;
        return ret;
    }

    /* ── Initialize panel ── */
    esp_lcd_panel_reset(s_lcd.panel);
    esp_lcd_panel_init(s_lcd.panel);

    /* Most ST7789 TFT panels require colour inversion for correct colours */
    esp_lcd_panel_invert_color(s_lcd.panel, true);

    esp_lcd_panel_disp_on_off(s_lcd.panel, true);

    /* Turn backlight on at full brightness */
    ret = bl_set_duty(100);
    if (ret != ESP_OK) {
        esp_lcd_panel_del(s_lcd.panel);
        esp_lcd_panel_io_del(s_lcd.io);
        s_lcd.panel = NULL;
        s_lcd.io    = NULL;
        return ret;
    }

    s_lcd.initialised = true;
    s_lcd.brightness  = 100;
    ESP_LOGI(TAG, "ST7789 initialized via esp_lcd (%dx%d)", LCD_WIDTH, LCD_HEIGHT);
    return ESP_OK;
}

/* ── Deinit ──────────────────────────────────────────────────────────────── */

static void st7789_deinit(void)
{
    if (!s_lcd.initialised) return;

    bl_set_duty(0);

    esp_lcd_panel_disp_on_off(s_lcd.panel, false);
    esp_lcd_panel_del(s_lcd.panel);
    esp_lcd_panel_io_del(s_lcd.io);

    s_lcd.panel       = NULL;
    s_lcd.io          = NULL;
    s_lcd.initialised = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* ── Flush ───────────────────────────────────────────────────────────────── */
/*
 * flush() receives a pixel-aligned area and RGB565 color_data.
 * esp_lcd_panel_draw_bitmap() takes exclusive end coordinates (one past the
 * last pixel), so we pass area->x2 + 1 and area->y2 + 1.
 */
static esp_err_t st7789_flush(const hal_area_t *area, const uint8_t *color_data)
{
    if (!s_lcd.initialised)       return ESP_ERR_INVALID_STATE;
    if (!area || !color_data)     return ESP_ERR_INVALID_ARG;

    return esp_lcd_panel_draw_bitmap(s_lcd.panel,
                                     area->x1, area->y1,
                                     area->x2 + 1, area->y2 + 1,
                                     color_data);
}

/* ── set_brightness ──────────────────────────────────────────────────────── */

static esp_err_t st7789_set_brightness(uint8_t percent)
{
    if (!s_lcd.initialised) return ESP_ERR_INVALID_STATE;
    if (percent > 100) percent = 100;
    if (s_lcd.cfg.pin_bl == GPIO_NUM_NC) return ESP_ERR_NOT_SUPPORTED;

    esp_err_t ret = bl_set_duty(percent);
    if (ret == ESP_OK && percent > 0) {
        s_lcd.brightness = percent;
    }
    return ret;
}

/* ── sleep ───────────────────────────────────────────────────────────────── */

static esp_err_t st7789_sleep(bool enter)
{
    if (!s_lcd.initialised) return ESP_ERR_INVALID_STATE;

    if (enter) {
        esp_lcd_panel_disp_on_off(s_lcd.panel, false);
        if (s_lcd.cfg.pin_bl != GPIO_NUM_NC) bl_set_duty(0);
    } else {
        esp_lcd_panel_disp_on_off(s_lcd.panel, true);
        if (s_lcd.cfg.pin_bl != GPIO_NUM_NC) bl_set_duty(s_lcd.brightness);
    }
    return ESP_OK;
}

/* ── set_refresh_mode ────────────────────────────────────────────────────── */
/*
 * LCD has no LUT-based refresh modes.  All modes are accepted without error.
 */
static esp_err_t st7789_set_refresh_mode(hal_display_refresh_mode_t mode)
{
    (void)mode;
    return ESP_OK;
}

/* ── Vtable + get ────────────────────────────────────────────────────────── */

static const hal_display_driver_t st7789_driver = {
    .init             = st7789_init,
    .deinit           = st7789_deinit,
    .flush            = st7789_flush,
    .refresh          = NULL,   /* LCD: no deferred refresh needed */
    .set_brightness   = st7789_set_brightness,
    .sleep            = st7789_sleep,
    .set_refresh_mode = st7789_set_refresh_mode,
    .width            = LCD_WIDTH,
    .height           = LCD_HEIGHT,
    .type             = HAL_DISPLAY_TYPE_LCD,
    .name             = "ST7789 (esp_lcd)",
};

const hal_display_driver_t *drv_lcd_st7789_get(void)
{
    return &st7789_driver;
}
