#pragma once
#include "esp_err.h"
#include <stdbool.h>

/* Check if /sdcard/update/thistle_os.bin exists */
bool recovery_ota_check_sd(void);

/* Flash firmware from SD card to ota_1 */
esp_err_t recovery_ota_apply_sd(void);

/* Download firmware from app store catalog URL and flash to ota_1 */
esp_err_t recovery_ota_download_and_flash(const char *catalog_url);
