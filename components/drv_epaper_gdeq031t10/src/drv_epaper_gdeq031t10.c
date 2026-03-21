// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — GDEQ031T10 3.1" 320x240 B/W e-paper driver
// Controller: UC8253 (compatible)
//
// SPI wiring assumed: MOSI, SCK wired to the SPI host; CS, DC, RST, BUSY are
// discrete GPIOs supplied through epaper_gdeq031t10_config_t.

#include "drv_epaper_gdeq031t10.h"

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/spi_master.h"
#include "driver/gpio.h"

#include <string.h>
#include <stdlib.h>

static const char *TAG = "epaper";

/* ── Panel geometry ─────────────────────────────────────────────────────── */
#define EPD_WIDTH       320
#define EPD_HEIGHT      240
#define EPD_FB_BYTES    (EPD_WIDTH * EPD_HEIGHT / 8)   /* 1-bit packed */

/* ── UC8253 command codes ────────────────────────────────────────────────── */
#define CMD_PANEL_SETTING           0x00
#define CMD_POWER_SETTING           0x01
#define CMD_POWER_OFF               0x02
#define CMD_POWER_ON                0x04
#define CMD_BOOSTER_SOFT_START      0x06
#define CMD_DEEP_SLEEP              0x07
#define CMD_DATA_START_TRANSMISSION 0x10
#define CMD_DATA_STOP               0x11
#define CMD_DISPLAY_REFRESH         0x12
#define CMD_PARTIAL_DATA_START      0x14
#define CMD_PARTIAL_DISPLAY_REFRESH 0x15
#define CMD_PARTIAL_DISPLAY_END     0x92
#define CMD_LUT_FULL                0x20
#define CMD_LUT_PARTIAL             0x21
#define CMD_PLL_CONTROL             0x30
#define CMD_TEMPERATURE_SENSOR      0x40
#define CMD_VCOM_DATA_INTERVAL      0x50
#define CMD_TCON_SETTING            0x60
#define CMD_RESOLUTION_SETTING      0x61
#define CMD_GSST_SETTING            0x65
#define CMD_REVISION                0x70
#define CMD_GET_STATUS              0x71
#define CMD_AUTO_MEASUREMENT_VCOM   0x80
#define CMD_READ_VCOM_VALUE         0x81
#define CMD_VCM_DC_SETTING          0x82
#define CMD_PARTIAL_WINDOW          0x90

/* ── LUT tables ─────────────────────────────────────────────────────────── */
/* Full-refresh LUT for GDEQ031T10 / UC8253 (44 bytes) */
static const uint8_t LUT_FULL_UPDATE[] = {
    0x80, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x60, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x80, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x60, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x03, 0x00, 0x00, 0x02,
    0x09, 0x09, 0x00, 0x00, 0x02,
    0x03, 0x03, 0x00, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x15, 0x41, 0xA8, 0x32, 0x30, 0x0A,
};

/* Partial-refresh LUT (faster, some ghosting) */
static const uint8_t LUT_PARTIAL_UPDATE[] = {
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x0A, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x15, 0x41, 0xA8, 0x32, 0x30, 0x0A,
};

/* ── Driver state ────────────────────────────────────────────────────────── */
static struct {
    spi_device_handle_t        spi;
    epaper_gdeq031t10_config_t cfg;
    hal_display_refresh_mode_t refresh_mode;
    uint8_t                   *fb;          /* 1-bit packed framebuffer */
    bool                       initialised;
} s_epd;

/* ── Low-level helpers ───────────────────────────────────────────────────── */

static void epaper_hw_reset(void)
{
    if (s_epd.cfg.pin_rst < 0) {
        /* RST not connected — skip hardware reset, just wait */
        vTaskDelay(pdMS_TO_TICKS(20));
        return;
    }
    gpio_set_level(s_epd.cfg.pin_rst, 0);
    vTaskDelay(pdMS_TO_TICKS(10));
    gpio_set_level(s_epd.cfg.pin_rst, 1);
    vTaskDelay(pdMS_TO_TICKS(10));
}

/* Wait until BUSY goes low (display idle).  Returns ESP_ERR_TIMEOUT on failure. */
static esp_err_t epaper_wait_busy(uint32_t timeout_ms)
{
    uint32_t elapsed = 0;
    while (gpio_get_level(s_epd.cfg.pin_busy)) {
        vTaskDelay(pdMS_TO_TICKS(10));
        elapsed += 10;
        if (elapsed >= timeout_ms) {
            ESP_LOGE(TAG, "BUSY timeout after %u ms", (unsigned)timeout_ms);
            return ESP_ERR_TIMEOUT;
        }
    }
    return ESP_OK;
}

/* Transmit a single command byte (DC=0). */
static esp_err_t epaper_send_cmd(uint8_t cmd)
{
    gpio_set_level(s_epd.cfg.pin_dc, 0);   /* command mode */
    spi_transaction_t t = {
        .length    = 8,
        .tx_buffer = &cmd,
    };
    return spi_device_polling_transmit(s_epd.spi, &t);
}

/* Transmit data bytes (DC=1). */
static esp_err_t epaper_send_data(const uint8_t *data, size_t len)
{
    if (len == 0) return ESP_OK;
    gpio_set_level(s_epd.cfg.pin_dc, 1);   /* data mode */
    spi_transaction_t t = {
        .length    = len * 8,
        .tx_buffer = data,
    };
    return spi_device_polling_transmit(s_epd.spi, &t);
}

static esp_err_t epaper_send_data_byte(uint8_t val)
{
    return epaper_send_data(&val, 1);
}

/* Load one of the LUT tables into the controller. */
static esp_err_t epaper_load_lut(const uint8_t *lut, size_t len)
{
    esp_err_t ret;
    ret = epaper_send_cmd(CMD_LUT_FULL);
    if (ret != ESP_OK) return ret;
    return epaper_send_data(lut, len);
}

/* ── Init sequence ───────────────────────────────────────────────────────── */

static esp_err_t gdeq031t10_init(const void *config)
{
    if (!config) {
        ESP_LOGE(TAG, "init: NULL config");
        return ESP_ERR_INVALID_ARG;
    }
    if (s_epd.initialised) {
        ESP_LOGW(TAG, "already initialised");
        return ESP_OK;
    }

    memcpy(&s_epd.cfg, config, sizeof(epaper_gdeq031t10_config_t));
    s_epd.refresh_mode = HAL_DISPLAY_REFRESH_FULL;

    /* ── Allocate framebuffer ── */
    s_epd.fb = heap_caps_malloc(EPD_FB_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_8BIT);
    if (!s_epd.fb) {
        ESP_LOGE(TAG, "framebuffer alloc failed (%u bytes)", EPD_FB_BYTES);
        return ESP_ERR_NO_MEM;
    }
    memset(s_epd.fb, 0xFF, EPD_FB_BYTES);  /* white canvas */

    /* ── GPIO configuration ── */
    gpio_config_t io_conf = {
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
        .pin_bit_mask = (1ULL << s_epd.cfg.pin_cs)  |
                        (1ULL << s_epd.cfg.pin_dc)  |
                        ((s_epd.cfg.pin_rst >= 0) ? (1ULL << s_epd.cfg.pin_rst) : 0),
    };
    esp_err_t ret = gpio_config(&io_conf); if (ret != ESP_OK) goto fail;

    gpio_config_t busy_conf = {
        .mode         = GPIO_MODE_INPUT,
        .pull_up_en   = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
        .pin_bit_mask = (1ULL << s_epd.cfg.pin_busy),
    };
    ret = gpio_config(&busy_conf); if (ret != ESP_OK) goto fail;

    /* CS defaults high (SPI driver will drive it during transactions) */
    gpio_set_level(s_epd.cfg.pin_cs, 1);

    /* ── SPI device ── */
    spi_device_interface_config_t dev_cfg = {
        .clock_speed_hz = s_epd.cfg.spi_clock_hz > 0
                              ? s_epd.cfg.spi_clock_hz
                              : 4000000,
        .mode           = 0,
        .spics_io_num   = s_epd.cfg.pin_cs,
        .queue_size     = 1,
    };
    ret = spi_bus_add_device(s_epd.cfg.spi_host, &dev_cfg, &s_epd.spi);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_add_device failed: %s", esp_err_to_name(ret));
        free(s_epd.fb);
        s_epd.fb = NULL;
        return ret;
    }

    /* ── Reset ── */
    epaper_hw_reset();  /* Skips if RST=-1 */

    /* Software reset (0x12) — required when hardware RST is not connected */
    ret = epaper_send_cmd(0x12);  /* SW_RESET */
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "SW_RESET command failed: %s", esp_err_to_name(ret));
        goto fail;
    }
    vTaskDelay(pdMS_TO_TICKS(100));  /* Wait for SW reset to complete */

    /* Wait for controller to come out of reset */
    ret = epaper_wait_busy(5000);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "display did not become ready after reset");
        goto fail;
    }

    /* ── Power-on sequence ── */
    /* Power setting: VGH=20V, VGL=-20V, VDH=15V, VDL=-15V
     * Each step is checked individually — the power sequencing is sensitive
     * and sending further data after a failed command can damage the display. */
    ret = epaper_send_cmd(CMD_POWER_SETTING);          if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x03);  /* VDS_EN, VDG_EN */  if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x00);  /* VCOM_HV, VGHL_LV */ if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x2B);  /* VDH 15V */          if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x2B);  /* VDL -15V */         if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0xFF);  /* VDHR */             if (ret != ESP_OK) goto fail;

    /* Booster soft-start */
    ret = epaper_send_cmd(CMD_BOOSTER_SOFT_START);     if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x17);                 if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x17);                 if (ret != ESP_OK) goto fail;
    ret = epaper_send_data_byte(0x17);                 if (ret != ESP_OK) goto fail;

    /* Power on */
    ret = epaper_send_cmd(CMD_POWER_ON);
    if (ret != ESP_OK) goto fail;
    ret = epaper_wait_busy(3000);
    if (ret != ESP_OK) { ESP_LOGE(TAG, "power-on timeout"); goto fail; }

    /* Panel setting: KW mode, 320x240, scan up-right */
    ret  = epaper_send_cmd(CMD_PANEL_SETTING);
    ret |= epaper_send_data_byte(0xBF);  /* KW-BF, KWR-AF, BOPO-N, RST_N */
    ret |= epaper_send_data_byte(0x0B);
    if (ret != ESP_OK) goto fail;

    /* PLL: 50 Hz frame rate */
    ret  = epaper_send_cmd(CMD_PLL_CONTROL);
    ret |= epaper_send_data_byte(0x3C);
    if (ret != ESP_OK) goto fail;

    /* Resolution: 320 x 240 */
    ret  = epaper_send_cmd(CMD_RESOLUTION_SETTING);
    ret |= epaper_send_data_byte((EPD_WIDTH  >> 8) & 0xFF);
    ret |= epaper_send_data_byte( EPD_WIDTH        & 0xFF);
    ret |= epaper_send_data_byte((EPD_HEIGHT >> 8) & 0xFF);
    ret |= epaper_send_data_byte( EPD_HEIGHT       & 0xFF);
    if (ret != ESP_OK) goto fail;

    /* VCOM and data interval */
    ret  = epaper_send_cmd(CMD_VCOM_DATA_INTERVAL);
    ret |= epaper_send_data_byte(0x97);
    if (ret != ESP_OK) goto fail;

    /* Load full-update LUT */
    ret = epaper_load_lut(LUT_FULL_UPDATE, sizeof(LUT_FULL_UPDATE));
    if (ret != ESP_OK) goto fail;

    s_epd.initialised = true;
    ESP_LOGI(TAG, "GDEQ031T10 initialised (%dx%d)", EPD_WIDTH, EPD_HEIGHT);
    return ESP_OK;

fail:
    spi_bus_remove_device(s_epd.spi);
    free(s_epd.fb);
    s_epd.fb  = NULL;
    s_epd.spi = NULL;
    return ret != ESP_OK ? ret : ESP_FAIL;
}

/* ── Deinit ──────────────────────────────────────────────────────────────── */

static void gdeq031t10_deinit(void)
{
    if (!s_epd.initialised) return;

    /* Issue deep sleep before removing device */
    epaper_send_cmd(CMD_DEEP_SLEEP);
    epaper_send_data_byte(0xA5);   /* check code */

    spi_bus_remove_device(s_epd.spi);
    s_epd.spi = NULL;

    free(s_epd.fb);
    s_epd.fb = NULL;

    s_epd.initialised = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* ── Flush ───────────────────────────────────────────────────────────────── */
/*
 * flush() receives a pixel-aligned area and 1-bit-per-pixel color_data
 * (1 = black, 0 = white, matching e-paper convention).
 * We copy the strip into our local framebuffer and then push the relevant
 * region (or full frame) to the controller.
 */
static esp_err_t gdeq031t10_flush(const hal_area_t *area, const uint8_t *color_data)
{
    if (!s_epd.initialised) return ESP_ERR_INVALID_STATE;
    if (!area || !color_data)  return ESP_ERR_INVALID_ARG;

    uint16_t x1 = area->x1, y1 = area->y1;
    uint16_t x2 = area->x2, y2 = area->y2;

    if (x2 >= EPD_WIDTH)  x2 = EPD_WIDTH  - 1;
    if (y2 >= EPD_HEIGHT) y2 = EPD_HEIGHT - 1;

    /* Validate that the area is non-inverted after clamping */
    if (x1 > x2 || y1 > y2) {
        ESP_LOGE(TAG, "flush: invalid area (%u,%u)-(%u,%u) after clamping",
                 x1, y1, x2, y2);
        return ESP_ERR_INVALID_ARG;
    }

    /* Ensure byte-alignment on x axis (display works in full bytes per row) */
    uint16_t bx1 = x1 & ~7u;          /* round down to byte boundary */
    uint16_t bx2 = (x2 | 7u);         /* round up */
    if (bx2 >= EPD_WIDTH) bx2 = EPD_WIDTH - 1;

    /* Copy incoming data into framebuffer byte by byte */
    uint16_t src_w = x2 - x1 + 1;   /* source width in pixels */
    for (uint16_t row = y1; row <= y2; row++) {
        for (uint16_t col = x1; col <= x2; col++) {
            uint32_t src_bit_idx = (uint32_t)(row - y1) * src_w + (col - x1);
            uint8_t  src_byte    = color_data[src_bit_idx / 8];
            uint8_t  src_bit     = (src_byte >> (7 - (src_bit_idx & 7))) & 1;

            uint32_t dst_bit_idx = (uint32_t)row * EPD_WIDTH + col;
            uint32_t dst_byte    = dst_bit_idx / 8;
            uint8_t  dst_mask    = 0x80u >> (dst_bit_idx & 7);

            if (src_bit) {
                s_epd.fb[dst_byte] |=  dst_mask;   /* black */
            } else {
                s_epd.fb[dst_byte] &= ~dst_mask;   /* white */
            }
        }
    }

    esp_err_t ret = ESP_OK;

    if (s_epd.refresh_mode == HAL_DISPLAY_REFRESH_FULL) {
        /* Full refresh: send entire framebuffer */
        ret = epaper_load_lut(LUT_FULL_UPDATE, sizeof(LUT_FULL_UPDATE));
        if (ret != ESP_OK) return ret;

        ret  = epaper_send_cmd(CMD_DATA_START_TRANSMISSION);
        ret |= epaper_send_data(s_epd.fb, EPD_FB_BYTES);
        if (ret != ESP_OK) return ret;

        ret  = epaper_send_cmd(CMD_DATA_STOP);
        ret |= epaper_send_cmd(CMD_DISPLAY_REFRESH);
        if (ret != ESP_OK) return ret;

        ret = epaper_wait_busy(10000);

    } else {
        /* Partial / fast refresh: update only the dirty window */
        ret = epaper_load_lut(LUT_PARTIAL_UPDATE, sizeof(LUT_PARTIAL_UPDATE));
        if (ret != ESP_OK) return ret;

        /* Define partial window (byte-aligned x) */
        ret  = epaper_send_cmd(CMD_PARTIAL_WINDOW);
        ret |= epaper_send_data_byte((bx1 >> 8) & 0xFF);
        ret |= epaper_send_data_byte( bx1       & 0xFF);
        ret |= epaper_send_data_byte((bx2 >> 8) & 0xFF);
        ret |= epaper_send_data_byte( bx2       & 0xFF);
        ret |= epaper_send_data_byte((y1  >> 8) & 0xFF);
        ret |= epaper_send_data_byte( y1        & 0xFF);
        ret |= epaper_send_data_byte((y2  >> 8) & 0xFF);
        ret |= epaper_send_data_byte( y2        & 0xFF);
        ret |= epaper_send_data_byte(0x01);   /* gates scan inside partial window */
        if (ret != ESP_OK) return ret;

        /* Send partial data row by row */
        ret = epaper_send_cmd(CMD_PARTIAL_DATA_START);
        if (ret != ESP_OK) return ret;
        gpio_set_level(s_epd.cfg.pin_dc, 1);
        uint16_t row_bytes = (bx2 - bx1 + 1) / 8;
        for (uint16_t row = y1; row <= y2; row++) {
            uint32_t fb_byte = ((uint32_t)row * EPD_WIDTH + bx1) / 8;
            ret = epaper_send_data(&s_epd.fb[fb_byte], row_bytes);
            if (ret != ESP_OK) return ret;
        }

        ret  = epaper_send_cmd(CMD_PARTIAL_DISPLAY_REFRESH);
        ret |= epaper_wait_busy(5000);
        ret |= epaper_send_cmd(CMD_PARTIAL_DISPLAY_END);
    }

    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "flush failed: %s", esp_err_to_name(ret));
    }
    return ret;
}

/* ── set_brightness ──────────────────────────────────────────────────────── */

static esp_err_t gdeq031t10_set_brightness(uint8_t percent)
{
    (void)percent;
    /* E-paper has no backlight */
    return ESP_ERR_NOT_SUPPORTED;
}

/* ── sleep ───────────────────────────────────────────────────────────────── */

static esp_err_t gdeq031t10_sleep(bool enter)
{
    if (!s_epd.initialised) return ESP_ERR_INVALID_STATE;

    if (enter) {
        esp_err_t ret = epaper_send_cmd(CMD_POWER_OFF);
        if (ret != ESP_OK) return ret;
        ret = epaper_wait_busy(3000);
        if (ret != ESP_OK) return ret;

        ret  = epaper_send_cmd(CMD_DEEP_SLEEP);
        ret |= epaper_send_data_byte(0xA5);
        return ret;
    } else {
        /* Wake: hardware reset + re-issue power-on */
        epaper_hw_reset();
        esp_err_t ret = epaper_wait_busy(3000);
        if (ret != ESP_OK) return ret;
        ret = epaper_send_cmd(CMD_POWER_ON);
        if (ret != ESP_OK) return ret;
        return epaper_wait_busy(3000);
    }
}

/* ── set_refresh_mode ────────────────────────────────────────────────────── */

static esp_err_t gdeq031t10_set_refresh_mode(hal_display_refresh_mode_t mode)
{
    s_epd.refresh_mode = mode;
    return ESP_OK;
}

/* ── Vtable ──────────────────────────────────────────────────────────────── */

static const hal_display_driver_t gdeq031t10_driver = {
    .init             = gdeq031t10_init,
    .deinit           = gdeq031t10_deinit,
    .flush            = gdeq031t10_flush,
    .set_brightness   = gdeq031t10_set_brightness,
    .sleep            = gdeq031t10_sleep,
    .set_refresh_mode = gdeq031t10_set_refresh_mode,
    .width            = EPD_WIDTH,
    .height           = EPD_HEIGHT,
    .type             = HAL_DISPLAY_TYPE_EPAPER,
    .name             = "GDEQ031T10",
};

const hal_display_driver_t *drv_epaper_gdeq031t10_get(void)
{
    return &gdeq031t10_driver;
}
