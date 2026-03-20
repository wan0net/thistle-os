#pragma once

#include "esp_err.h"
#include "sdmmc_cmd.h"

typedef struct {
    int format_if_mount_failed;
    int max_files;
    int allocation_unit_size;
} esp_vfs_fat_sdmmc_mount_config_t;

typedef struct {
    int gpio_cs;
    int host_id;
} sdspi_device_config_t;

#define SDSPI_DEVICE_CONFIG_DEFAULT() (sdspi_device_config_t){0, 0}

static inline esp_err_t esp_vfs_fat_sdspi_mount(
    const char *mp,
    const sdmmc_host_t *h,
    const sdspi_device_config_t *s,
    const esp_vfs_fat_sdmmc_mount_config_t *c,
    sdmmc_card_t **card)
{
    (void)mp; (void)h; (void)s; (void)c; (void)card;
    return 0;
}

static inline esp_err_t esp_vfs_fat_sdcard_unmount(const char *mp, sdmmc_card_t *card) {
    (void)mp; (void)card;
    return 0;
}

static inline esp_err_t esp_vfs_fat_info(const char *mp, uint64_t *total, uint64_t *free_b) {
    (void)mp;
    *total  = 16ULL * 1024 * 1024 * 1024;
    *free_b = 14ULL * 1024 * 1024 * 1024;
    return 0;
}
