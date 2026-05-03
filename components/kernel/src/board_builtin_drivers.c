// SPDX-License-Identifier: BSD-3-Clause
// Board builtin driver initialization — called from board_config.rs when
// ELF driver files are not present on SPIFFS/SD. Provides bus init helpers
// and maps driver IDs to compiled-in vtables.

#include <string.h>
#include <stdlib.h>
#include <stdint.h>
#include "esp_log.h"
#include "esp_err.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "hal/board.h"
#include "drv_epaper_gdeq031t10.h"
#include "drv_kbd_tca8418.h"
#include "drv_touch_cst328.h"

static const char *TAG = "board_builtin";

// ---------------------------------------------------------------------------
// Minimal JSON int helper — handles bare integers and quoted hex ("0x34").
// Searches for "key": <value> and parses the value.
// ---------------------------------------------------------------------------

static int json_int(const char *json, const char *key, int default_val)
{
    if (!json || !key) return default_val;
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return default_val;
    p += strlen(pattern);
    // Skip whitespace and colon
    while (*p == ' ' || *p == '\t' || *p == ':') p++;
    if (*p == '"') {
        // Quoted value — may be hex ("0x34") or decimal string
        p++;
        return (int)strtol(p, NULL, 0);
    }
    // Bare integer (possibly negative)
    return (int)strtol(p, NULL, 0);
}

// ---------------------------------------------------------------------------
// Bus initialisation helpers (called from board_config.rs via FFI)
// ---------------------------------------------------------------------------

esp_err_t board_bus_init_spi(int host, int mosi, int miso, int sclk, int max_transfer_bytes)
{
    spi_bus_config_t cfg = {
        .mosi_io_num     = mosi,
        .miso_io_num     = miso,
        .sclk_io_num     = sclk,
        .quadwp_io_num   = -1,
        .quadhd_io_num   = -1,
        .max_transfer_sz = max_transfer_bytes > 0 ? max_transfer_bytes : 4096,
    };
    esp_err_t ret = spi_bus_initialize((spi_host_device_t)host, &cfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK && ret != ESP_ERR_INVALID_STATE) {
        ESP_LOGE(TAG, "spi_bus_initialize host=%d failed: %s", host, esp_err_to_name(ret));
        return ret;
    }
    if (ret == ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "SPI host %d already initialized", host);
    }
    // Store host ID as the "handle" — ELF drivers call hal_bus_get_spi(idx)
    // and cast it back to spi_host_device_t.
    hal_bus_register_spi(host, (void *)(intptr_t)host);
    ESP_LOGI(TAG, "SPI host %d ready (MOSI=%d MISO=%d SCLK=%d)", host, mosi, miso, sclk);
    return ESP_OK;
}

esp_err_t board_bus_init_i2c(int port, int sda, int scl, int freq_hz)
{
    i2c_master_bus_config_t cfg = {
        .i2c_port              = (i2c_port_t)port,
        .sda_io_num            = (gpio_num_t)sda,
        .scl_io_num            = (gpio_num_t)scl,
        .clk_source            = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt     = 7,
        .flags.enable_internal_pullup = true,
    };
    (void)freq_hz; // freq_hz is set per-device in i2c_master_probe/add_device
    i2c_master_bus_handle_t handle;
    esp_err_t ret = i2c_new_master_bus(&cfg, &handle);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2c_new_master_bus port=%d failed: %s", port, esp_err_to_name(ret));
        return ret;
    }
    hal_bus_register_i2c(port, handle);
    ESP_LOGI(TAG, "I2C port %d ready (SDA=%d SCL=%d)", port, sda, scl);
    return ESP_OK;
}

// ---------------------------------------------------------------------------
// Builtin driver init — called when a .drv.elf is absent from SPIFFS/SD.
// id and hal_type come from board.json; config_json is the per-driver config
// sub-object.
// ---------------------------------------------------------------------------

esp_err_t board_builtin_driver_init(const char *id, const char *hal_type,
                                    const char *config_json)
{
    if (!id || !hal_type || !config_json) return ESP_ERR_INVALID_ARG;
    ESP_LOGI(TAG, "builtin: %s (hal=%s)", id, hal_type);

    // ── E-paper display: GDEQ031T10 ─────────────────────────────────────────
    if (strcmp(id, "com.thistle.drv.epaper-gdeq031t10") == 0) {
        static epaper_gdeq031t10_config_t cfg;
        int spi_bus_idx = json_int(config_json, "spi_bus", 0);
        // hal_bus_get_spi stores the host ID as the pointer value
        cfg.spi_host    = (spi_host_device_t)(intptr_t)hal_bus_get_spi(spi_bus_idx);
        cfg.pin_cs      = (gpio_num_t)json_int(config_json, "cs",           -1);
        cfg.pin_dc      = (gpio_num_t)json_int(config_json, "dc",           -1);
        cfg.pin_rst     = (gpio_num_t)json_int(config_json, "rst",          -1);
        cfg.pin_busy    = (gpio_num_t)json_int(config_json, "busy",         -1);
        cfg.spi_clock_hz =            json_int(config_json, "spi_clock_hz", 2000000);
        ESP_LOGI(TAG, "epaper: host=%d cs=%d dc=%d rst=%d busy=%d clk=%d",
                 cfg.spi_host, cfg.pin_cs, cfg.pin_dc, cfg.pin_rst,
                 cfg.pin_busy, cfg.spi_clock_hz);
        return hal_display_register(drv_epaper_gdeq031t10_get(), &cfg);
    }

    // ── Keyboard: TCA8418 ────────────────────────────────────────────────────
    if (strcmp(id, "com.thistle.drv.kbd-tca8418") == 0) {
        static kbd_tca8418_config_t cfg;
        int i2c_bus_idx = json_int(config_json, "i2c_bus", 0);
        cfg.i2c_bus  = (i2c_master_bus_handle_t)hal_bus_get_i2c(i2c_bus_idx);
        cfg.i2c_addr = (uint8_t)json_int(config_json, "i2c_addr", 0x34);
        cfg.pin_int  = (gpio_num_t)json_int(config_json, "pin_int", -1);
        ESP_LOGI(TAG, "kbd-tca8418: i2c_bus=%p addr=0x%02x int=%d",
                 (void *)cfg.i2c_bus, cfg.i2c_addr, cfg.pin_int);
        return hal_input_register(drv_kbd_tca8418_get(), &cfg);
    }

    // ── Touchscreen: CST328 ──────────────────────────────────────────────────
    if (strcmp(id, "com.thistle.drv.touch-cst328") == 0) {
        static touch_cst328_config_t cfg;
        int i2c_bus_idx = json_int(config_json, "i2c_bus", 0);
        cfg.i2c_bus  = (i2c_master_bus_handle_t)hal_bus_get_i2c(i2c_bus_idx);
        cfg.i2c_addr = (uint8_t)json_int(config_json, "i2c_addr", 0x1A);
        cfg.pin_int  = (gpio_num_t)json_int(config_json, "pin_int", -1);
        cfg.pin_rst  = (gpio_num_t)json_int(config_json, "pin_rst", -1);
        cfg.max_x    = (uint16_t)json_int(config_json, "max_x", 240);
        cfg.max_y    = (uint16_t)json_int(config_json, "max_y", 320);
        ESP_LOGI(TAG, "touch-cst328: i2c_bus=%p addr=0x%02x int=%d rst=%d",
                 (void *)cfg.i2c_bus, cfg.i2c_addr, cfg.pin_int, cfg.pin_rst);
        return hal_input_register(drv_touch_cst328_get(), &cfg);
    }

    ESP_LOGW(TAG, "no builtin for driver id=%s — skipping", id);
    return ESP_OK;
}

// ---------------------------------------------------------------------------
// GPIO pre-init helper — called from board_config.rs to configure power-enable
// and other board-level GPIOs before driver initialization.
// ---------------------------------------------------------------------------

esp_err_t board_gpio_set_output(int pin, int level, int delay_ms)
{
    if (pin < 0) return ESP_OK;
    gpio_reset_pin((gpio_num_t)pin);
    esp_err_t ret = gpio_set_direction((gpio_num_t)pin, GPIO_MODE_OUTPUT);
    if (ret != ESP_OK) return ret;
    ret = gpio_set_level((gpio_num_t)pin, (uint32_t)level);
    ESP_LOGI(TAG, "gpio_output: pin=%d level=%d delay=%d ret=%s",
             pin, level, delay_ms, esp_err_to_name(ret));
    if (delay_ms > 0) {
        vTaskDelay(pdMS_TO_TICKS(delay_ms));
        // After 1V8_EN is toggled, check the e-paper BUSY pin (GPIO 37)
        // to verify the display is wired. When 1V8 is OFF, BUSY should
        // float HIGH (pull-up); when ON, display drives BUSY LOW (idle).
        if (pin == 38) {
            /* UC8253 BUSY: LOW = busy, HIGH = ready. With 1V8 off, the
             * unpowered chip can't drive BUSY so it floats; with 1V8 on,
             * the chip should release BUSY HIGH once POR is complete. */
            gpio_config_t busy_conf = {
                .pin_bit_mask = (1ULL << 37),
                .mode         = GPIO_MODE_INPUT,
                .pull_up_en   = GPIO_PULLUP_DISABLE,
                .pull_down_en = GPIO_PULLDOWN_DISABLE,
                .intr_type    = GPIO_INTR_DISABLE,
            };
            gpio_config(&busy_conf);
            int busy = gpio_get_level((gpio_num_t)37);
            ESP_LOGI(TAG, "1V8_EN=%d -> BUSY(GPIO37)=%d (1=ready, 0=busy/unpowered)",
                     level, busy);
        }
    }
    return ret;
}
