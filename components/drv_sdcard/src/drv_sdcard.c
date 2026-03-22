#include "drv_sdcard.h"
#include "hal/sdcard_path.h"
#include "esp_log.h"
#include "esp_vfs_fat.h"
#include "sdmmc_cmd.h"
#include "driver/sdspi_host.h"
#include <string.h>

static const char *TAG = "sdcard";

static struct {
    sdcard_config_t cfg;
    sdmmc_card_t *card;
    bool mounted;
    bool initialized;
} s_sd;

static esp_err_t sdcard_init(const void *config)
{
    if (s_sd.initialized) {
        ESP_LOGW(TAG, "already initialized");
        return ESP_OK;
    }
    if (!config) {
        ESP_LOGE(TAG, "NULL config");
        return ESP_ERR_INVALID_ARG;
    }

    memcpy(&s_sd.cfg, config, sizeof(sdcard_config_t));

    if (!s_sd.cfg.mount_point) {
        s_sd.cfg.mount_point = THISTLE_SDCARD;
    }
    if (s_sd.cfg.max_files <= 0) {
        s_sd.cfg.max_files = 5;
    }

    s_sd.initialized = true;
    s_sd.mounted = false;
    ESP_LOGI(TAG, "SD card driver initialized (CS=GPIO%d, mount=%s)",
             s_sd.cfg.pin_cs, s_sd.cfg.mount_point);
    return ESP_OK;
}

static void sdcard_deinit(void)
{
    if (s_sd.mounted) {
        /* Unmount first */
        esp_vfs_fat_sdcard_unmount(s_sd.cfg.mount_point, s_sd.card);
        s_sd.mounted = false;
    }
    s_sd.initialized = false;
    ESP_LOGI(TAG, "SD card deinitialized");
}

static esp_err_t sdcard_mount(const char *mount_point)
{
    if (!s_sd.initialized) return ESP_ERR_INVALID_STATE;
    if (s_sd.mounted) {
        ESP_LOGW(TAG, "already mounted");
        return ESP_OK;
    }

    const char *mp = mount_point ? mount_point : s_sd.cfg.mount_point;

    esp_vfs_fat_sdmmc_mount_config_t mount_cfg = {
        .format_if_mount_failed = false,
        .max_files = s_sd.cfg.max_files,
        .allocation_unit_size = 16 * 1024,
    };

    sdmmc_host_t host = SDSPI_HOST_DEFAULT();
    host.max_freq_khz = 4000;  /* 4 MHz — conservative for shared SPI bus */

    sdspi_device_config_t slot_cfg = SDSPI_DEVICE_CONFIG_DEFAULT();
    slot_cfg.gpio_cs = s_sd.cfg.pin_cs;
    slot_cfg.host_id = s_sd.cfg.spi_host;

    ESP_LOGI(TAG, "Mounting SD card at %s", mp);
    esp_err_t ret = esp_vfs_fat_sdspi_mount(mp, &host, &slot_cfg, &mount_cfg, &s_sd.card);

    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to mount SD card: %s", esp_err_to_name(ret));
        return ret;
    }

    s_sd.mounted = true;

    /* Log card info */
    sdmmc_card_print_info(stdout, s_sd.card);
    ESP_LOGI(TAG, "SD card mounted at %s", mp);
    return ESP_OK;
}

static esp_err_t sdcard_unmount(void)
{
    if (!s_sd.mounted) return ESP_OK;

    esp_err_t ret = esp_vfs_fat_sdcard_unmount(s_sd.cfg.mount_point, s_sd.card);
    if (ret == ESP_OK) {
        s_sd.mounted = false;
        ESP_LOGI(TAG, "SD card unmounted");
    }
    return ret;
}

static bool sdcard_is_mounted(void)
{
    return s_sd.mounted;
}

static uint64_t sdcard_get_total_bytes(void)
{
    if (!s_sd.mounted) return 0;
    uint64_t total = 0, free_bytes = 0;
    if (esp_vfs_fat_info(s_sd.cfg.mount_point, &total, &free_bytes) != ESP_OK) return 0;
    return total;
}

static uint64_t sdcard_get_free_bytes(void)
{
    if (!s_sd.mounted) return 0;
    uint64_t total = 0, free_bytes = 0;
    if (esp_vfs_fat_info(s_sd.cfg.mount_point, &total, &free_bytes) != ESP_OK) return 0;
    return free_bytes;
}

static const hal_storage_driver_t sdcard_driver = {
    .init = sdcard_init,
    .deinit = sdcard_deinit,
    .mount = sdcard_mount,
    .unmount = sdcard_unmount,
    .is_mounted = sdcard_is_mounted,
    .get_total_bytes = sdcard_get_total_bytes,
    .get_free_bytes = sdcard_get_free_bytes,
    .type = HAL_STORAGE_TYPE_SD,
    .name = "SD Card",
};

const hal_storage_driver_t *drv_sdcard_get(void)
{
    return &sdcard_driver;
}
