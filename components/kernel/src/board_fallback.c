/*
 * Safe fallback board_init — called when no board.json is found on SD card.
 * Does NOT initialize any SPI/I2C/display hardware to avoid crashing on
 * unknown pin configurations. Just registers minimal HAL drivers so the
 * kernel can boot to a usable state.
 *
 * Declared __attribute__((weak)) so that an explicitly linked board component
 * (board_tdeck, board_tdeck_pro, etc.) can override it with a strong symbol.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "hal/board.h"
#include "esp_log.h"

static const char *TAG = "board_fallback";

__attribute__((weak))
esp_err_t board_init(void)
{
    ESP_LOGW(TAG, "No board.json found — running in safe mode");
    ESP_LOGW(TAG, "Insert SD card with config/boards/<board>.json and reboot");
    ESP_LOGW(TAG, "Or use Recovery mode to provision the device");

    hal_set_board_name("Unknown (safe mode)");

    /* Don't initialize ANY hardware — no SPI, no I2C, no display.
     * The kernel will boot with NULL HAL drivers. Apps should handle
     * NULL display/input gracefully. */

    return ESP_OK;
}
