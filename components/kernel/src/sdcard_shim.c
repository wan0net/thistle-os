// SPDX-License-Identifier: BSD-3-Clause
// Thin C shim for SD card SPI mount — wraps ESP-IDF macros that can't be
// called from Rust (SDSPI_HOST_DEFAULT, SDSPI_DEVICE_CONFIG_DEFAULT).

#include "esp_vfs_fat.h"
#include "sdmmc_cmd.h"
#include "driver/sdspi_host.h"
#include "esp_log.h"

static const char *TAG = "sdcard_shim";

esp_err_t drv_sdcard_spi_mount_shim(
    const char *mount_point,
    int spi_host,
    int pin_cs,
    int max_files,
    sdmmc_card_t **card_out)
{
    esp_vfs_fat_sdmmc_mount_config_t mount_cfg = {
        .format_if_mount_failed = false,
        .max_files = max_files,
        .allocation_unit_size = 16 * 1024,
    };

    sdmmc_host_t host = SDSPI_HOST_DEFAULT();
    host.max_freq_khz = 4000;

    sdspi_device_config_t slot_cfg = SDSPI_DEVICE_CONFIG_DEFAULT();
    slot_cfg.gpio_cs = pin_cs;
    slot_cfg.host_id = spi_host;

    ESP_LOGI(TAG, "Mounting SD card at %s (CS=GPIO%d)", mount_point, pin_cs);
    return esp_vfs_fat_sdspi_mount(mount_point, &host, &slot_cfg, &mount_cfg, card_out);
}
