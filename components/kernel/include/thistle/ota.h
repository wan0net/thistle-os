#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/* OTA update source */
typedef enum {
    OTA_SOURCE_SD_CARD,     /* Update from /sdcard/update/thistle_os.bin */
    OTA_SOURCE_HTTP,        /* Update from HTTP URL */
} ota_source_t;

/* OTA progress callback */
typedef void (*ota_progress_cb_t)(uint32_t bytes_written, uint32_t total_bytes, void *user_data);

/* Initialize OTA subsystem */
esp_err_t ota_init(void);

/* Check if an OTA update file exists on SD card */
bool ota_sd_update_available(void);

/* Apply firmware update from SD card (/sdcard/update/thistle_os.bin)
 * This writes to the OTA partition and reboots on success. */
esp_err_t ota_apply_from_sd(ota_progress_cb_t progress_cb, void *user_data);

/* Apply firmware update from HTTP URL
 * Downloads and writes to OTA partition, reboots on success. */
esp_err_t ota_apply_from_http(const char *url, ota_progress_cb_t progress_cb, void *user_data);

/* Get the currently running firmware version string */
const char *ota_get_current_version(void);

/* Get the currently running partition label */
const char *ota_get_running_partition(void);

/* Mark the current firmware as valid (prevents rollback) */
esp_err_t ota_mark_valid(void);

/* Rollback to previous firmware */
esp_err_t ota_rollback(void);
