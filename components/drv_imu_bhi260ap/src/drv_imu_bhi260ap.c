// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Bosch BHI260AP IMU driver (stub)
#include "drv_imu_bhi260ap.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "bhi260ap";

static imu_bhi260ap_config_t s_config;
static hal_imu_cb_t           s_callback  = NULL;
static void                  *s_user_data = NULL;

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t bhi260ap_init(const void *config)
{
    // TODO: Add I2C device to bus, check chip ID register (0x2B = BHI260AP),
    //       upload firmware blob if required, configure virtual sensor list,
    //       install ISR on pin_int.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    if (config) memcpy(&s_config, config, sizeof(s_config));
    return ESP_ERR_NOT_SUPPORTED;
}

static void bhi260ap_deinit(void)
{
    // TODO: Remove ISR, remove I2C device, clear state.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
}

static esp_err_t bhi260ap_get_data(hal_imu_data_t *data)
{
    // TODO: Read FIFO or status registers; parse accel, gyro, mag virtual
    //       sensor output packets.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t bhi260ap_register_callback(hal_imu_cb_t cb, void *user_data)
{
    s_callback  = cb;
    s_user_data = user_data;
    return ESP_OK;
}

static esp_err_t bhi260ap_set_sample_rate(uint16_t hz)
{
    // TODO: Configure virtual sensor sample rate via BHY2 host interface.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t bhi260ap_sleep(bool enter)
{
    // TODO: Issue sleep/wakeup command via host interface.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    return ESP_ERR_NOT_SUPPORTED;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_imu_driver_t s_vtable = {
    .init              = bhi260ap_init,
    .deinit            = bhi260ap_deinit,
    .get_data          = bhi260ap_get_data,
    .register_callback = bhi260ap_register_callback,
    .set_sample_rate   = bhi260ap_set_sample_rate,
    .sleep             = bhi260ap_sleep,
    .name              = "BHI260AP",
};

const hal_imu_driver_t *drv_imu_bhi260ap_get(void)
{
    return &s_vtable;
}
