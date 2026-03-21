// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * board_config.c — JSON-driven board initialization
 *
 * Reads board.json from SPIFFS, initializes SPI/I2C buses, and loads
 * drivers dynamically. This replaces the compiled-in board_* components
 * for the immutable kernel architecture.
 *
 * Boot sequence:
 *   1. Mount SPIFFS (internal flash, always available)
 *   2. Read board.json
 *   3. Init SPI buses → store handles in HAL registry
 *   4. Init I2C buses → store handles in HAL registry
 *   5. For each driver: load .drv.elf, pass config JSON, driver registers with HAL
 */

#include "thistle/board_config.h"
#include "thistle/driver_loader.h"
#include "thistle/signing.h"
#include "hal/board.h"
#include "hal/sdcard_path.h"
#include "esp_log.h"
#include "esp_err.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef SIMULATOR_BUILD
#include "driver/spi_master.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include "esp_spiffs.h"
#endif

static const char *TAG = "board_cfg";

#define MAX_CONFIG_SIZE 8192
#define MAX_DRIVERS 12
#define MAX_DRIVER_CONFIG_SIZE 512

static char s_board_name[64] = "Unknown";

/* ── JSON helpers (same pattern as manifest.c) ──────────────────────── */

static bool json_str(const char *json, const char *key, char *buf, size_t buf_size)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return false;
    p += strlen(pattern);
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;
    if (*p != '"') return false;
    p++;
    size_t i = 0;
    while (*p && *p != '"' && i < buf_size - 1) {
        buf[i++] = *p++;
    }
    buf[i] = '\0';
    return true;
}

static bool json_int(const char *json, const char *key, int *out)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return false;
    p += strlen(pattern);
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;
    char *end;
    long val = strtol(p, &end, 0); /* base 0: supports 0x prefix for hex */
    if (end == p) return false;
    *out = (int)val;
    return true;
}

/* Find the Nth occurrence of a JSON object start '{' within an array context.
 * Returns pointer to the '{' or NULL. */
static const char *json_array_nth(const char *json, const char *array_key, int index)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", array_key);
    const char *p = strstr(json, pattern);
    if (!p) return NULL;
    p = strchr(p, '[');
    if (!p) return NULL;
    p++; /* skip '[' */

    int count = 0;
    int depth = 0;
    while (*p) {
        if (*p == '{') {
            if (depth == 0) {
                if (count == index) return p;
                count++;
            }
            depth++;
        } else if (*p == '}') {
            depth--;
        } else if (*p == ']' && depth == 0) {
            break;
        }
        p++;
    }
    return NULL;
}

/* Extract a sub-object as a string. Finds "key": { ... } and copies the
 * braces-enclosed content into buf. */
static bool json_object(const char *json, const char *key, char *buf, size_t buf_size)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return false;
    p += strlen(pattern);
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;
    if (*p != '{') return false;

    int depth = 0;
    const char *start = p;
    while (*p) {
        if (*p == '{') depth++;
        else if (*p == '}') {
            depth--;
            if (depth == 0) {
                size_t len = (size_t)(p - start + 1);
                if (len >= buf_size) len = buf_size - 1;
                memcpy(buf, start, len);
                buf[len] = '\0';
                return true;
            }
        }
        p++;
    }
    return false;
}

/* ── Bus initialization ─────────────────────────────────────────────── */

#ifndef SIMULATOR_BUILD
static esp_err_t init_spi_buses(const char *json)
{
    for (int i = 0; i < 2; i++) {
        const char *bus = json_array_nth(json, "spi", i);
        if (!bus) break;

        int host = 2, mosi = -1, miso = -1, sclk = -1, max_xfer = 4096;
        json_int(bus, "host", &host);
        json_int(bus, "mosi", &mosi);
        json_int(bus, "miso", &miso);
        json_int(bus, "sclk", &sclk);
        json_int(bus, "max_transfer_bytes", &max_xfer);

        spi_bus_config_t cfg = {
            .mosi_io_num = mosi,
            .miso_io_num = miso,
            .sclk_io_num = sclk,
            .quadwp_io_num = -1,
            .quadhd_io_num = -1,
            .max_transfer_sz = max_xfer,
        };

        esp_err_t ret = spi_bus_initialize(host, &cfg, SPI_DMA_CH_AUTO);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "SPI bus %d init failed: %s", i, esp_err_to_name(ret));
            return ret;
        }

        /* Store the host ID as the "handle" — SPI uses host IDs, not opaque handles */
        hal_bus_register_spi(host, (void *)(intptr_t)host);
        ESP_LOGI(TAG, "SPI bus %d: host=%d mosi=%d miso=%d sclk=%d", i, host, mosi, miso, sclk);
    }
    return ESP_OK;
}

static esp_err_t init_i2c_buses(const char *json)
{
    for (int i = 0; i < 2; i++) {
        const char *bus = json_array_nth(json, "i2c", i);
        if (!bus) break;

        int port = 0, sda = -1, scl = -1, freq = 400000;
        json_int(bus, "port", &port);
        json_int(bus, "sda", &sda);
        json_int(bus, "scl", &scl);
        json_int(bus, "freq_hz", &freq);

        i2c_master_bus_config_t cfg = {
            .i2c_port = port,
            .sda_io_num = sda,
            .scl_io_num = scl,
            .clk_source = I2C_CLK_SRC_DEFAULT,
            .glitch_ignore_cnt = 7,
            .flags.enable_internal_pullup = true,
        };

        i2c_master_bus_handle_t handle;
        esp_err_t ret = i2c_new_master_bus(&cfg, &handle);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "I2C bus %d init failed: %s", i, esp_err_to_name(ret));
            return ret;
        }

        hal_bus_register_i2c(port, (void *)handle);
        ESP_LOGI(TAG, "I2C bus %d: port=%d sda=%d scl=%d freq=%d", i, port, sda, scl, freq);
    }
    return ESP_OK;
}
#endif /* SIMULATOR_BUILD */

/* ── Driver loading from board.json ─────────────────────────────────── */

static esp_err_t load_drivers_from_config(const char *json)
{
    driver_loader_init();

    for (int i = 0; i < MAX_DRIVERS; i++) {
        const char *drv = json_array_nth(json, "drivers", i);
        if (!drv) break;

        char id[64] = {0};
        char entry[64] = {0};
        char hal[16] = {0};
        json_str(drv, "id", id, sizeof(id));
        json_str(drv, "entry", entry, sizeof(entry));
        json_str(drv, "hal", hal, sizeof(hal));

        if (entry[0] == '\0') {
            ESP_LOGW(TAG, "Driver %d missing 'entry' field, skipping", i);
            continue;
        }

        /* Extract the config sub-object as a JSON string to pass to the driver */
        char config_json[MAX_DRIVER_CONFIG_SIZE] = "{}";
        json_object(drv, "config", config_json, sizeof(config_json));

        /* Try loading from SPIFFS first, then SD card */
        char path[256];
        bool found = false;

        /* SPIFFS: /spiffs/drivers/<entry> */
        snprintf(path, sizeof(path), "/spiffs/drivers/%s", entry);
        FILE *f = fopen(path, "rb");
        if (f) {
            fclose(f);
            found = true;
        }

        /* SD card: /sdcard/drivers/<entry> */
        if (!found) {
            snprintf(path, sizeof(path), THISTLE_SDCARD "/drivers/%s", entry);
            f = fopen(path, "rb");
            if (f) {
                fclose(f);
                found = true;
            }
        }

        if (!found) {
            ESP_LOGW(TAG, "Driver '%s' (%s) not found on SPIFFS or SD card", id, entry);
            continue;
        }

        ESP_LOGI(TAG, "Loading driver: %s [%s] from %s (config: %zu bytes)", id, hal, path, strlen(config_json));
        esp_err_t ret = driver_loader_load_with_config(path, config_json);
        if (ret == ESP_OK) {
            ESP_LOGI(TAG, "Driver '%s' loaded successfully", id);
        } else {
            ESP_LOGW(TAG, "Driver '%s' failed to load: %s", id, esp_err_to_name(ret));
        }
    }

    return ESP_OK;
}

/* ── SPIFFS mount ───────────────────────────────────────────────────── */

#ifndef SIMULATOR_BUILD
static esp_err_t mount_spiffs(void)
{
    esp_vfs_spiffs_conf_t conf = {
        .base_path = "/spiffs",
        .partition_label = "storage",
        .max_files = 10,
        .format_if_mount_failed = true,
    };
    esp_err_t ret = esp_vfs_spiffs_register(&conf);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "SPIFFS mount failed: %s", esp_err_to_name(ret));
        return ret;
    }
    size_t total = 0, used = 0;
    esp_spiffs_info("storage", &total, &used);
    ESP_LOGI(TAG, "SPIFFS mounted: %zu KB used / %zu KB total", used / 1024, total / 1024);
    return ESP_OK;
}
#endif

/* ── Public API ─────────────────────────────────────────────────────── */

esp_err_t board_config_init(const char *config_path)
{
    if (!config_path) {
        config_path = "/spiffs/config/board.json";
    }

#ifndef SIMULATOR_BUILD
    /* Mount SPIFFS first — board.json lives there */
    esp_err_t ret = mount_spiffs();
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "SPIFFS unavailable, falling back to compiled board_init()");
        return board_init();
    }
#endif

    /* Read board.json */
    FILE *f = fopen(config_path, "r");
    if (!f) {
        ESP_LOGW(TAG, "No board.json at %s, falling back to compiled board_init()", config_path);
#ifndef SIMULATOR_BUILD
        return board_init();
#else
        return ESP_ERR_NOT_FOUND;
#endif
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0 || size > MAX_CONFIG_SIZE) {
        fclose(f);
        ESP_LOGE(TAG, "board.json invalid size: %ld", size);
        return ESP_ERR_INVALID_SIZE;
    }

    char *json = malloc((size_t)size + 1);
    if (!json) {
        fclose(f);
        return ESP_ERR_NO_MEM;
    }
    fread(json, 1, (size_t)size, f);
    fclose(f);
    json[size] = '\0';

    /* Parse board name */
    char board_section[512];
    if (json_object(json, "board", board_section, sizeof(board_section))) {
        json_str(board_section, "name", s_board_name, sizeof(s_board_name));
    }
    hal_set_board_name(s_board_name);
    ESP_LOGI(TAG, "Board: %s (from %s)", s_board_name, config_path);

    /* Parse and init buses */
    char buses_section[1024];
    if (json_object(json, "buses", buses_section, sizeof(buses_section))) {
#ifndef SIMULATOR_BUILD
        esp_err_t ret = init_spi_buses(buses_section);
        if (ret != ESP_OK) {
            free(json);
            return ret;
        }
        ret = init_i2c_buses(buses_section);
        if (ret != ESP_OK) {
            free(json);
            return ret;
        }
#else
        ESP_LOGI(TAG, "Simulator: skipping bus init");
#endif
    }

    /* Load drivers */
    load_drivers_from_config(json);

    free(json);
    ESP_LOGI(TAG, "Board config init complete");
    return ESP_OK;
}

const char *board_config_get_name(void)
{
    return s_board_name;
}
