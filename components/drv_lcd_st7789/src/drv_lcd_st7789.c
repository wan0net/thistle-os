// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ST7789 LCD display driver
//
// SPI wiring: MOSI, SCK on the shared SPI host; CS, DC are discrete GPIOs
// supplied through lcd_st7789_config_t.  RST may be GPIO_NUM_NC (no pin).
// Backlight is driven via LEDC PWM on pin_bl.

#include "drv_lcd_st7789.h"

#include "esp_log.h"
#include "esp_err.h"
#include "driver/spi_master.h"
#include "driver/gpio.h"
#include "driver/ledc.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>
#include <stdlib.h>

static const char *TAG = "st7789";

/* ── Panel geometry ─────────────────────────────────────────────────────── */
#define LCD_WIDTH  320
#define LCD_HEIGHT 240

/* ── ST7789 command codes ────────────────────────────────────────────────── */
#define CMD_SWRESET  0x01   /* Software reset */
#define CMD_SLPOUT   0x11   /* Sleep out */
#define CMD_SLPIN    0x10   /* Sleep in */
#define CMD_COLMOD   0x3A   /* Interface pixel format */
#define CMD_MADCTL   0x36   /* Memory data access control (rotation) */
#define CMD_CASET    0x2A   /* Column address set */
#define CMD_RASET    0x2B   /* Row address set */
#define CMD_RAMWR    0x2C   /* Memory write */
#define CMD_DISPON   0x29   /* Display on */
#define CMD_DISPOFF  0x28   /* Display off */
#define CMD_INVON    0x21   /* Display inversion on (many ST7789 panels need this) */
#define CMD_NORON    0x13   /* Normal display mode on */

/* COLMOD value: 16-bit RGB565 */
#define COLMOD_16BIT 0x55

/* MADCTL: row/column exchange + RGB order — adjust for panel orientation */
#define MADCTL_LANDSCAPE 0x70   /* MY=0, MX=1, MV=1, ML=0, BGR=1, MH=0 — 320x240 */

/* ── LEDC backlight ──────────────────────────────────────────────────────── */
#define BL_LEDC_TIMER      LEDC_TIMER_0
#define BL_LEDC_MODE       LEDC_LOW_SPEED_MODE
#define BL_LEDC_CHANNEL    LEDC_CHANNEL_0
#define BL_LEDC_FREQ_HZ    5000
#define BL_LEDC_DUTY_RES   LEDC_TIMER_8_BIT   /* 0–255 duty range */
#define BL_LEDC_MAX_DUTY   255

/* ── Driver state ────────────────────────────────────────────────────────── */
static struct {
    spi_device_handle_t spi;
    lcd_st7789_config_t cfg;
    bool                initialised;
    bool                bl_enabled;
} s_lcd;

/* ── Low-level SPI helpers ───────────────────────────────────────────────── */

/* Send a single command byte (DC=0). */
static esp_err_t lcd_send_cmd(uint8_t cmd)
{
    gpio_set_level(s_lcd.cfg.pin_dc, 0);
    spi_transaction_t t = {
        .length    = 8,
        .tx_buffer = &cmd,
    };
    return spi_device_polling_transmit(s_lcd.spi, &t);
}

/* Send data bytes (DC=1). */
static esp_err_t lcd_send_data(const uint8_t *data, size_t len)
{
    if (len == 0) return ESP_OK;
    gpio_set_level(s_lcd.cfg.pin_dc, 1);
    spi_transaction_t t = {
        .length    = len * 8,
        .tx_buffer = data,
    };
    return spi_device_polling_transmit(s_lcd.spi, &t);
}

static esp_err_t lcd_send_data_byte(uint8_t val)
{
    return lcd_send_data(&val, 1);
}

/* ── Hardware reset ──────────────────────────────────────────────────────── */

static void lcd_hw_reset(void)
{
    if (s_lcd.cfg.pin_rst == GPIO_NUM_NC) {
        /* No hardware reset pin — issue software reset via command instead */
        lcd_send_cmd(CMD_SWRESET);
        vTaskDelay(pdMS_TO_TICKS(150));
        return;
    }
    gpio_set_level(s_lcd.cfg.pin_rst, 0);
    vTaskDelay(pdMS_TO_TICKS(10));
    gpio_set_level(s_lcd.cfg.pin_rst, 1);
    vTaskDelay(pdMS_TO_TICKS(120));
}

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

/* ── Address window helper ───────────────────────────────────────────────── */

static esp_err_t lcd_set_window(uint16_t x1, uint16_t y1, uint16_t x2, uint16_t y2)
{
    esp_err_t ret;
    uint8_t buf[4];

    /* Column address set */
    ret = lcd_send_cmd(CMD_CASET);
    if (ret != ESP_OK) return ret;
    buf[0] = (x1 >> 8) & 0xFF;
    buf[1] =  x1       & 0xFF;
    buf[2] = (x2 >> 8) & 0xFF;
    buf[3] =  x2       & 0xFF;
    ret = lcd_send_data(buf, 4);
    if (ret != ESP_OK) return ret;

    /* Row address set */
    ret = lcd_send_cmd(CMD_RASET);
    if (ret != ESP_OK) return ret;
    buf[0] = (y1 >> 8) & 0xFF;
    buf[1] =  y1       & 0xFF;
    buf[2] = (y2 >> 8) & 0xFF;
    buf[3] =  y2       & 0xFF;
    ret = lcd_send_data(buf, 4);
    if (ret != ESP_OK) return ret;

    /* Memory write — caller sends pixels next */
    return lcd_send_cmd(CMD_RAMWR);
}

/* ── Init sequence ───────────────────────────────────────────────────────── */

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

    /* ── GPIO configuration ── */
    /* CS and DC are output */
    uint64_t out_mask = (1ULL << s_lcd.cfg.pin_cs) | (1ULL << s_lcd.cfg.pin_dc);
    if (s_lcd.cfg.pin_rst != GPIO_NUM_NC) {
        out_mask |= (1ULL << s_lcd.cfg.pin_rst);
    }
    gpio_config_t io_conf = {
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
        .pin_bit_mask = out_mask,
    };
    ESP_ERROR_CHECK(gpio_config(&io_conf));

    /* CS high (idle), DC low initially */
    gpio_set_level(s_lcd.cfg.pin_cs, 1);
    gpio_set_level(s_lcd.cfg.pin_dc, 0);
    if (s_lcd.cfg.pin_rst != GPIO_NUM_NC) {
        gpio_set_level(s_lcd.cfg.pin_rst, 1);
    }

    /* ── SPI device ── */
    spi_device_interface_config_t dev_cfg = {
        .clock_speed_hz = s_lcd.cfg.spi_clock_hz > 0
                              ? s_lcd.cfg.spi_clock_hz
                              : 40000000,
        .mode           = 0,
        .spics_io_num   = s_lcd.cfg.pin_cs,
        .queue_size     = 7,
    };
    esp_err_t ret = spi_bus_add_device(s_lcd.cfg.spi_host, &dev_cfg, &s_lcd.spi);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_add_device failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* ── Backlight init (off until display is ready) ── */
    ret = bl_init(s_lcd.cfg.pin_bl);
    if (ret != ESP_OK) goto fail;

    /* ── Hardware/software reset ── */
    lcd_hw_reset();

    /* ── Init command sequence ── */
    /* Sleep out — exit sleep mode */
    ret = lcd_send_cmd(CMD_SLPOUT);
    if (ret != ESP_OK) goto fail;
    vTaskDelay(pdMS_TO_TICKS(120));  /* datasheet: wait ≥120ms after SLPOUT */

    /* Pixel format: 16-bit RGB565 */
    ret  = lcd_send_cmd(CMD_COLMOD);
    ret |= lcd_send_data_byte(COLMOD_16BIT);
    if (ret != ESP_OK) goto fail;

    /* Memory access control: landscape orientation */
    ret  = lcd_send_cmd(CMD_MADCTL);
    ret |= lcd_send_data_byte(MADCTL_LANDSCAPE);
    if (ret != ESP_OK) goto fail;

    /* Normal display mode */
    ret = lcd_send_cmd(CMD_NORON);
    if (ret != ESP_OK) goto fail;
    vTaskDelay(pdMS_TO_TICKS(10));

    /* Inversion on — most ST7789 TFT panels need this for correct colours */
    ret = lcd_send_cmd(CMD_INVON);
    if (ret != ESP_OK) goto fail;

    /* Display on */
    ret = lcd_send_cmd(CMD_DISPON);
    if (ret != ESP_OK) goto fail;
    vTaskDelay(pdMS_TO_TICKS(20));

    /* Turn backlight on at full brightness */
    ret = bl_set_duty(100);
    if (ret != ESP_OK) goto fail;
    s_lcd.bl_enabled = true;

    s_lcd.initialised = true;
    ESP_LOGI(TAG, "ST7789 initialised (%dx%d)", LCD_WIDTH, LCD_HEIGHT);
    return ESP_OK;

fail:
    spi_bus_remove_device(s_lcd.spi);
    s_lcd.spi = NULL;
    return ret != ESP_OK ? ret : ESP_FAIL;
}

/* ── Deinit ──────────────────────────────────────────────────────────────── */

static void st7789_deinit(void)
{
    if (!s_lcd.initialised) return;

    bl_set_duty(0);

    lcd_send_cmd(CMD_DISPOFF);
    lcd_send_cmd(CMD_SLPIN);
    vTaskDelay(pdMS_TO_TICKS(5));

    spi_bus_remove_device(s_lcd.spi);
    s_lcd.spi = NULL;

    s_lcd.initialised = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* ── Flush ───────────────────────────────────────────────────────────────── */
/*
 * flush() receives a pixel-aligned area and RGB565 color_data (packed, big-endian
 * as expected by the ST7789).  Data goes directly to the controller via SPI DMA —
 * no intermediate framebuffer is needed.
 */
static esp_err_t st7789_flush(const hal_area_t *area, const uint8_t *color_data)
{
    if (!s_lcd.initialised) return ESP_ERR_INVALID_STATE;
    if (!area || !color_data)  return ESP_ERR_INVALID_ARG;

    uint16_t x1 = area->x1, y1 = area->y1;
    uint16_t x2 = area->x2, y2 = area->y2;

    if (x2 >= LCD_WIDTH)  x2 = LCD_WIDTH  - 1;
    if (y2 >= LCD_HEIGHT) y2 = LCD_HEIGHT - 1;

    /* Set address window */
    esp_err_t ret = lcd_set_window(x1, y1, x2, y2);
    if (ret != ESP_OK) return ret;

    /* Send pixel data (RGB565, 2 bytes per pixel) */
    size_t len = (size_t)(x2 - x1 + 1) * (y2 - y1 + 1) * 2;
    gpio_set_level(s_lcd.cfg.pin_dc, 1);  /* data mode */
    spi_transaction_t t = {
        .length    = len * 8,
        .tx_buffer = color_data,
    };
    ret = spi_device_transmit(s_lcd.spi, &t);  /* DMA-capable path */
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "flush SPI transmit failed: %s", esp_err_to_name(ret));
    }
    return ret;
}

/* ── set_brightness ──────────────────────────────────────────────────────── */

static esp_err_t st7789_set_brightness(uint8_t percent)
{
    if (!s_lcd.initialised) return ESP_ERR_INVALID_STATE;
    if (percent > 100) percent = 100;
    return bl_set_duty(percent);
}

/* ── sleep ───────────────────────────────────────────────────────────────── */

static esp_err_t st7789_sleep(bool enter)
{
    if (!s_lcd.initialised) return ESP_ERR_INVALID_STATE;

    if (enter) {
        bl_set_duty(0);
        esp_err_t ret = lcd_send_cmd(CMD_DISPOFF);
        if (ret != ESP_OK) return ret;
        ret = lcd_send_cmd(CMD_SLPIN);
        vTaskDelay(pdMS_TO_TICKS(5));
        return ret;
    } else {
        esp_err_t ret = lcd_send_cmd(CMD_SLPOUT);
        if (ret != ESP_OK) return ret;
        vTaskDelay(pdMS_TO_TICKS(120));
        ret = lcd_send_cmd(CMD_DISPON);
        if (ret != ESP_OK) return ret;
        vTaskDelay(pdMS_TO_TICKS(20));
        return bl_set_duty(100);
    }
}

/* ── set_refresh_mode ────────────────────────────────────────────────────── */
/*
 * LCD does not use LUT-based refresh modes like e-paper.  Full/partial are
 * equivalent — every flush writes pixels directly to GRAM.  Fast mode could
 * in future enable tearing-effect synchronisation via TE pin, but for now
 * all modes are accepted without error.
 */
static esp_err_t st7789_set_refresh_mode(hal_display_refresh_mode_t mode)
{
    (void)mode;
    return ESP_OK;
}

/* ── Vtable + get ────────────────────────────────────────────────────────── */

static const hal_display_driver_t s_vtable = {
    .init             = st7789_init,
    .deinit           = st7789_deinit,
    .flush            = st7789_flush,
    .set_brightness   = st7789_set_brightness,
    .sleep            = st7789_sleep,
    .set_refresh_mode = st7789_set_refresh_mode,
    .width            = LCD_WIDTH,
    .height           = LCD_HEIGHT,
    .type             = HAL_DISPLAY_TYPE_LCD,
    .name             = "ST7789",
};

const hal_display_driver_t *drv_lcd_st7789_get(void)
{
    return &s_vtable;
}
