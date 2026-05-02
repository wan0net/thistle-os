// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — GDEQ031T10 3.1" 240x320 B/W e-paper driver
// Controller: UC8253 (confirmed against GxEPD2_310_GDEQ031T10).
//
// Wiring on LilyGo T-Deck Pro: MOSI=33, SCK=36, CS=34, DC=35, BUSY=37,
// RST=-1 (not connected — chip is reset by toggling 1V8_EN on GPIO38
// before this driver runs, plus the soft-reset command below).
//
// BUSY polarity: LOW = busy, HIGH = ready. Wait while BUSY reads 0.

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

/* ── Panel geometry ───────────────────────────────────────────────────── */
#define EPD_WIDTH       240
#define EPD_HEIGHT      320
#define EPD_FB_BYTES    (EPD_WIDTH * EPD_HEIGHT / 8)   /* 9600 bytes */

/* ── UC8253 commands (matching GxEPD2_310_GDEQ031T10) ────────────────── */
#define CMD_PSR             0x00  /* Panel Setting */
#define CMD_PWR             0x01  /* Power Setting */
#define CMD_POF             0x02  /* Power OFF */
#define CMD_PON             0x04  /* Power ON */
#define CMD_DEEP_SLEEP      0x07
#define CMD_DTM1            0x10  /* Data Transmission 1 (old/B/W frame) */
#define CMD_DSP             0x11  /* Data Stop */
#define CMD_DRF             0x12  /* Display Refresh */
#define CMD_DTM2            0x13  /* Data Transmission 2 (new/B/W frame) */
#define CMD_LUT_VCOM        0x20
#define CMD_PLL             0x30
#define CMD_TSC             0x40  /* Temperature Sensor */
#define CMD_CDI             0x50  /* VCOM and Data Interval */
#define CMD_TCON            0x60
#define CMD_TRES            0x61  /* Resolution Setting */
#define CMD_REVISION        0x70
#define CMD_FLG             0x71  /* Status / FLAG */
#define CMD_PARTIAL_WINDOW  0x90
#define CMD_PARTIAL_IN      0x91
#define CMD_PARTIAL_OUT     0x92
#define CMD_CCSET           0xE0  /* Cascade Setting */
#define CMD_TSSET           0xE5  /* Force Temperature */

/* ── Driver state ──────────────────────────────────────────────────── */
static struct {
    spi_device_handle_t        spi;
    epaper_gdeq031t10_config_t cfg;
    hal_display_refresh_mode_t refresh_mode;
    uint8_t                   *fb;          /* current frame to send via 0x13 */
    uint8_t                   *fb_old;      /* previous frame to send via 0x10 */
    bool                       initialised;
    bool                       init_seq_done;     /* PSR sent since last refresh */
    bool                       power_on;
    bool                       first_refresh_done;
} s_epd;

/* ── Low-level helpers ──────────────────────────────────────────────── */

static esp_err_t epaper_send_cmd(uint8_t cmd)
{
    gpio_set_level(s_epd.cfg.pin_cs, 0);
    gpio_set_level(s_epd.cfg.pin_dc, 0);
    spi_transaction_t t = { .length = 8, .tx_buffer = &cmd };
    esp_err_t ret = spi_device_polling_transmit(s_epd.spi, &t);
    gpio_set_level(s_epd.cfg.pin_cs, 1);
    if (ret != ESP_OK) ESP_LOGE(TAG, "cmd 0x%02X spi err: %s", cmd, esp_err_to_name(ret));
    return ret;
}

static esp_err_t epaper_send_data(const uint8_t *data, size_t len)
{
    if (len == 0) return ESP_OK;
    gpio_set_level(s_epd.cfg.pin_cs, 0);
    gpio_set_level(s_epd.cfg.pin_dc, 1);
    esp_err_t ret = ESP_OK;
    size_t sent = 0;
    while (sent < len && ret == ESP_OK) {
        size_t chunk = len - sent;
        if (chunk > 4096) chunk = 4096;
        spi_transaction_t t = { .length = chunk * 8, .tx_buffer = data + sent };
        ret = spi_device_polling_transmit(s_epd.spi, &t);
        sent += chunk;
    }
    gpio_set_level(s_epd.cfg.pin_cs, 1);
    return ret;
}

static esp_err_t epaper_send_data_byte(uint8_t val)
{
    return epaper_send_data(&val, 1);
}

/* Wait while BUSY reads LOW (chip is busy). Returns when BUSY goes HIGH or
 * after timeout_ms — never blocks forever. */
static void epaper_wait_ready(uint32_t timeout_ms, const char *tag)
{
    uint32_t elapsed = 0;
    while (gpio_get_level(s_epd.cfg.pin_busy) == 0 && elapsed < timeout_ms) {
        vTaskDelay(pdMS_TO_TICKS(10));
        elapsed += 10;
    }
    if (gpio_get_level(s_epd.cfg.pin_busy) == 0) {
        ESP_LOGW(TAG, "%s: BUSY still LOW after %ums (continuing)", tag, (unsigned)timeout_ms);
    } else if (elapsed > 0) {
        ESP_LOGI(TAG, "%s: ready after %ums", tag, (unsigned)elapsed);
    }
}

static void epaper_hw_reset(void)
{
    if (s_epd.cfg.pin_rst < 0) {
        /* T-Deck Pro: RST not connected; rely on 1V8_EN power cycle (board init) */
        vTaskDelay(pdMS_TO_TICKS(20));
        return;
    }
    gpio_set_level(s_epd.cfg.pin_rst, 1);
    vTaskDelay(pdMS_TO_TICKS(10));
    gpio_set_level(s_epd.cfg.pin_rst, 0);
    vTaskDelay(pdMS_TO_TICKS(10));
    gpio_set_level(s_epd.cfg.pin_rst, 1);
    vTaskDelay(pdMS_TO_TICKS(10));
}

/* ── UC8253 init sequence (matches GxEPD2 _InitDisplay) ────────────── */

static esp_err_t epaper_init_display(void)
{
    esp_err_t ret;

    /* Soft reset via PSR (used because RST is not wired on T-Deck Pro) */
    ret  = epaper_send_cmd(CMD_PSR);
    ret |= epaper_send_data_byte(0x1E);   /* soft reset */
    ret |= epaper_send_data_byte(0x0D);
    if (ret != ESP_OK) return ret;
    vTaskDelay(pdMS_TO_TICKS(2));

    /* Panel setting: BWOTP / KW mode */
    ret  = epaper_send_cmd(CMD_PSR);
    ret |= epaper_send_data_byte(0x1F);
    ret |= epaper_send_data_byte(0x0D);
    if (ret != ESP_OK) return ret;

    s_epd.power_on = false;
    s_epd.init_seq_done = true;
    return ESP_OK;
}

static esp_err_t epaper_power_on(void)
{
    if (s_epd.power_on) return ESP_OK;
    esp_err_t ret = epaper_send_cmd(CMD_PON);
    if (ret != ESP_OK) return ret;
    epaper_wait_ready(200, "PON");
    s_epd.power_on = true;
    return ESP_OK;
}

static esp_err_t epaper_power_off(void)
{
    if (!s_epd.power_on) return ESP_OK;
    esp_err_t ret = epaper_send_cmd(CMD_POF);
    if (ret != ESP_OK) return ret;
    epaper_wait_ready(200, "POF");
    s_epd.power_on = false;
    return ESP_OK;
}

/* ── Driver vtable functions ──────────────────────────────────────── */

static esp_err_t gdeq031t10_init(const void *config)
{
    if (!config) return ESP_ERR_INVALID_ARG;
    if (s_epd.initialised) return ESP_OK;

    memcpy(&s_epd.cfg, config, sizeof(epaper_gdeq031t10_config_t));
    s_epd.refresh_mode = HAL_DISPLAY_REFRESH_FULL;

    /* Allocate framebuffers */
    s_epd.fb     = heap_caps_malloc(EPD_FB_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_8BIT);
    s_epd.fb_old = heap_caps_malloc(EPD_FB_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_8BIT);
    if (!s_epd.fb || !s_epd.fb_old) {
        ESP_LOGE(TAG, "FB alloc failed");
        free(s_epd.fb);     s_epd.fb = NULL;
        free(s_epd.fb_old); s_epd.fb_old = NULL;
        return ESP_ERR_NO_MEM;
    }
    memset(s_epd.fb,     0xFF, EPD_FB_BYTES);   /* white */
    memset(s_epd.fb_old, 0xFF, EPD_FB_BYTES);

    /* GPIO: CS, DC, (RST) as outputs */
    uint64_t out_mask = (1ULL << s_epd.cfg.pin_cs) | (1ULL << s_epd.cfg.pin_dc);
    if (s_epd.cfg.pin_rst >= 0) out_mask |= (1ULL << s_epd.cfg.pin_rst);
    gpio_config_t io_conf = {
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
        .pin_bit_mask = out_mask,
    };
    esp_err_t ret = gpio_config(&io_conf);
    if (ret != ESP_OK) goto fail;

    /* GPIO: BUSY as input. No internal pull — UC8253 drives it actively. */
    gpio_config_t busy_conf = {
        .mode         = GPIO_MODE_INPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
        .pin_bit_mask = (1ULL << s_epd.cfg.pin_busy),
    };
    ret = gpio_config(&busy_conf);
    if (ret != ESP_OK) goto fail;

    gpio_set_level(s_epd.cfg.pin_cs, 1);

    /* SPI device — manual CS so we can frame each command/data segment */
    spi_device_interface_config_t dev_cfg = {
        .clock_source   = SPI_CLK_SRC_DEFAULT,
        .clock_speed_hz = s_epd.cfg.spi_clock_hz > 0 ? s_epd.cfg.spi_clock_hz : 2000000,
        .mode           = 0,
        .spics_io_num   = -1,
        .queue_size     = 1,
    };
    ret = spi_bus_add_device(s_epd.cfg.spi_host, &dev_cfg, &s_epd.spi);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_add_device: %s", esp_err_to_name(ret));
        goto fail;
    }

    ESP_LOGI(TAG, "UC8253 init: CS=%d DC=%d BUSY=%d RST=%d host=%d clk=%d",
             (int)s_epd.cfg.pin_cs, (int)s_epd.cfg.pin_dc,
             (int)s_epd.cfg.pin_busy, (int)s_epd.cfg.pin_rst,
             (int)s_epd.cfg.spi_host, (int)s_epd.cfg.spi_clock_hz);

    epaper_hw_reset();
    ESP_LOGI(TAG, "BUSY at start = %d (1=ready 0=busy)",
             gpio_get_level(s_epd.cfg.pin_busy));

    ret = epaper_init_display();
    if (ret != ESP_OK) goto fail;

    s_epd.initialised        = true;
    s_epd.first_refresh_done = false;
    ESP_LOGI(TAG, "UC8253 init complete (%dx%d portrait)", EPD_WIDTH, EPD_HEIGHT);
    return ESP_OK;

fail:
    if (s_epd.spi) { spi_bus_remove_device(s_epd.spi); s_epd.spi = NULL; }
    free(s_epd.fb);     s_epd.fb = NULL;
    free(s_epd.fb_old); s_epd.fb_old = NULL;
    return ret != ESP_OK ? ret : ESP_FAIL;
}

static void gdeq031t10_deinit(void)
{
    if (!s_epd.initialised) return;
    epaper_power_off();
    epaper_send_cmd(CMD_DEEP_SLEEP);
    epaper_send_data_byte(0xA5);
    if (s_epd.spi) { spi_bus_remove_device(s_epd.spi); s_epd.spi = NULL; }
    free(s_epd.fb);     s_epd.fb = NULL;
    free(s_epd.fb_old); s_epd.fb_old = NULL;
    s_epd.initialised = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* Copy a WM-supplied 1-bit packed sub-rectangle into the panel-sized
 * framebuffer. WM bits are MSB-first within each byte; rows are packed
 * (w + 7) / 8 bytes per row. */
static esp_err_t gdeq031t10_flush(const hal_area_t *area, const uint8_t *color_data)
{
    if (!s_epd.initialised) return ESP_ERR_INVALID_STATE;
    if (!area || !color_data) return ESP_ERR_INVALID_ARG;

    uint16_t x1 = area->x1, y1 = area->y1;
    uint16_t x2 = area->x2, y2 = area->y2;
    if (x2 >= EPD_WIDTH)  x2 = EPD_WIDTH  - 1;
    if (y2 >= EPD_HEIGHT) y2 = EPD_HEIGHT - 1;
    if (x1 > x2 || y1 > y2) return ESP_ERR_INVALID_ARG;

    /* Fast path: full-width row-aligned blit. WM almost always sends the
     * whole screen with x1=0, x2=239 — direct memcpy is ~50× faster than
     * the per-pixel path. */
    if (x1 == 0 && x2 == EPD_WIDTH - 1) {
        size_t row_bytes = EPD_WIDTH / 8;
        memcpy(s_epd.fb + (size_t)y1 * row_bytes,
               color_data,
               (size_t)(y2 - y1 + 1) * row_bytes);
        return ESP_OK;
    }

    /* Slow path: arbitrary rectangle, bit-by-bit. */
    uint16_t src_w = x2 - x1 + 1;
    for (uint16_t row = y1; row <= y2; row++) {
        for (uint16_t col = x1; col <= x2; col++) {
            uint32_t src_bit_idx = (uint32_t)(row - y1) * src_w + (col - x1);
            uint8_t  src_bit     = (color_data[src_bit_idx >> 3] >> (7 - (src_bit_idx & 7))) & 1;
            uint32_t dst_bit_idx = (uint32_t)row * EPD_WIDTH + col;
            uint8_t  dst_mask    = 0x80u >> (dst_bit_idx & 7);
            if (src_bit) s_epd.fb[dst_bit_idx >> 3] |=  dst_mask;
            else         s_epd.fb[dst_bit_idx >> 3] &= ~dst_mask;
        }
    }
    return ESP_OK;
}

static esp_err_t gdeq031t10_refresh(void)
{
    if (!s_epd.initialised) return ESP_ERR_INVALID_STATE;

    /* Skip refresh entirely if nothing has changed since last commit.
     * E-paper panels should be static — this preserves both the image and
     * the panel's lifespan. */
    if (s_epd.first_refresh_done &&
        memcmp(s_epd.fb, s_epd.fb_old, EPD_FB_BYTES) == 0) {
        ESP_LOGD(TAG, "refresh: framebuffer unchanged — skipping");
        return ESP_OK;
    }

    bool fast = s_epd.first_refresh_done &&
                s_epd.refresh_mode != HAL_DISPLAY_REFRESH_FULL;

    esp_err_t ret;

    /* Re-issue init if we cleared init_seq_done after the last refresh */
    if (!s_epd.init_seq_done) {
        ret = epaper_init_display();
        if (ret != ESP_OK) return ret;
    }

    /* Send previous frame via 0x10 (used by partial/fast diff) */
    ret  = epaper_send_cmd(CMD_DTM1);
    if (ret != ESP_OK) return ret;
    ret = epaper_send_data(s_epd.fb_old, EPD_FB_BYTES);
    if (ret != ESP_OK) return ret;

    /* Send current frame via 0x13 */
    ret = epaper_send_cmd(CMD_DTM2);
    if (ret != ESP_OK) return ret;
    ret = epaper_send_data(s_epd.fb, EPD_FB_BYTES);
    if (ret != ESP_OK) return ret;

    /* Fast/partial mode: cascade + force temperature (matches GxEPD2 _Update_Part) */
    if (fast) {
        ret  = epaper_send_cmd(CMD_CCSET);
        ret |= epaper_send_data_byte(0x02);   /* TSFIX */
        ret |= epaper_send_cmd(CMD_TSSET);
        ret |= epaper_send_data_byte(0x79);   /* 121°C — fast LUT */
        if (ret != ESP_OK) return ret;
    }

    /* VCOM/data interval — 0x97 for full refresh, 0xD7 for fast */
    ret  = epaper_send_cmd(CMD_CDI);
    ret |= epaper_send_data_byte(fast ? 0xD7 : 0x97);
    if (ret != ESP_OK) return ret;

    /* Power on, refresh, wait, power off */
    ret = epaper_power_on();
    if (ret != ESP_OK) return ret;

    ret = epaper_send_cmd(CMD_DRF);
    if (ret != ESP_OK) return ret;
    /* Full refresh ~1.1s, partial ~0.7s per GxEPD2 timing. Allow generous
     * margin (8s) since the BUSY signal may be flaky on this board. */
    epaper_wait_ready(fast ? 3000 : 8000, "DRF");

    epaper_power_off();

    /* Save current frame as previous; auto-switch to fast for future updates */
    memcpy(s_epd.fb_old, s_epd.fb, EPD_FB_BYTES);
    s_epd.first_refresh_done = true;
    s_epd.init_seq_done      = false;   /* GxEPD2 forces re-init each cycle */
    if (s_epd.refresh_mode == HAL_DISPLAY_REFRESH_FULL && !fast) {
        /* Stay in FULL only on the first paint; later refreshes use fast.
         * Caller can force a full clean refresh via set_refresh_mode(FULL). */
        s_epd.refresh_mode = HAL_DISPLAY_REFRESH_FAST;
    }

    ESP_LOGI(TAG, "refresh: %s done", fast ? "fast" : "full");
    return ESP_OK;
}

static esp_err_t gdeq031t10_set_brightness(uint8_t percent)
{
    (void)percent;
    return ESP_ERR_NOT_SUPPORTED;   /* e-paper has no backlight */
}

static esp_err_t gdeq031t10_sleep(bool enter)
{
    if (!s_epd.initialised) return ESP_ERR_INVALID_STATE;
    if (enter) {
        epaper_power_off();
        epaper_send_cmd(CMD_DEEP_SLEEP);
        epaper_send_data_byte(0xA5);
        s_epd.init_seq_done = false;
    } else {
        epaper_hw_reset();
        epaper_init_display();
    }
    return ESP_OK;
}

static esp_err_t gdeq031t10_set_refresh_mode(hal_display_refresh_mode_t mode)
{
    s_epd.refresh_mode = mode;
    return ESP_OK;
}

/* ── Vtable ──────────────────────────────────────────────────────── */

static const hal_display_driver_t gdeq031t10_driver = {
    .init             = gdeq031t10_init,
    .deinit           = gdeq031t10_deinit,
    .flush            = gdeq031t10_flush,
    .refresh          = gdeq031t10_refresh,
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
