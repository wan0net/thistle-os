// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Simcom A7682E 4G modem driver (stub)
#include "drv_modem_a7682e.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "a7682e";

static a7682e_config_t s_config;
static bool            s_initialised = false;

// ---------------------------------------------------------------------------
// Public API implementations
// ---------------------------------------------------------------------------

esp_err_t drv_a7682e_init(const a7682e_config_t *config)
{
    // TODO: Install UART driver, configure baud rate, configure PWRKEY and
    //       RESET GPIOs as outputs.  Call drv_a7682e_power(true) and wait for
    //       modem to emit "RDY" unsolicited result code.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    memcpy(&s_config, config, sizeof(s_config));
    s_initialised = false;
    return ESP_ERR_NOT_SUPPORTED;
}

void drv_a7682e_deinit(void)
{
    // TODO: drv_a7682e_power(false), uninstall UART driver.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    s_initialised = false;
}

esp_err_t drv_a7682e_send_at(const char *cmd, char *buf, size_t buf_len, uint32_t timeout_ms)
{
    // TODO: Write "cmd\r\n" to UART, read response into buf until "OK\r\n" or
    //       "ERROR\r\n" or timeout.  Return ESP_ERR_TIMEOUT on timeout.
    ESP_LOGW(TAG, "%s: not implemented (cmd=%s)", __func__, cmd ? cmd : "(null)");
    if (buf && buf_len > 0) {
        buf[0] = '\0';
    }
    return ESP_ERR_NOT_SUPPORTED;
}

esp_err_t drv_a7682e_power(bool on)
{
    // TODO: Pulse PWRKEY for 1.5 s to toggle power state; wait for UART
    //       activity to confirm state change.
    ESP_LOGW(TAG, "%s: not implemented (on=%d)", __func__, (int)on);
    return ESP_ERR_NOT_SUPPORTED;
}

bool drv_a7682e_is_ready(void)
{
    // TODO: Send "AT\r\n", check for "OK" response within 300 ms.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return false;
}
