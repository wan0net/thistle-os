// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — LTR-553ALS light/proximity sensor driver (stub)
#include "drv_light_ltr553.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "ltr553";

// LTR-553ALS I2C register map (partial)
#define LTR553_REG_ALS_CONTR   0x80
#define LTR553_REG_PS_CONTR    0x81
#define LTR553_REG_PS_LED      0x82
#define LTR553_REG_PART_ID     0x86   // Expected: 0x92
#define LTR553_REG_ALS_DATA_CH1_L 0x88
#define LTR553_REG_PS_DATA_L   0x8D

static light_ltr553_config_t  s_config;
static i2c_master_dev_handle_t s_dev = NULL;

// ---------------------------------------------------------------------------
// Public API implementations
// ---------------------------------------------------------------------------

esp_err_t drv_ltr553_init(const light_ltr553_config_t *config)
{
    // TODO: Add I2C device to bus at config->i2c_addr, verify part ID
    //       register == 0x92, write ALS_CONTR active mode, PS_CONTR active
    //       mode, configure PS LED to 50 mA 100% duty.  If pin_int is valid,
    //       install interrupt ISR.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    memcpy(&s_config, config, sizeof(s_config));
    return ESP_ERR_NOT_SUPPORTED;
}

void drv_ltr553_deinit(void)
{
    // TODO: Remove I2C device, disable interrupts.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    s_dev = NULL;
}

esp_err_t drv_ltr553_read(ltr553_data_t *data)
{
    // TODO: Read ALS CH0/CH1 16-bit registers, apply gain/integration-time
    //       formula to convert to lux.  Read PS 11-bit register for proximity.
    ESP_LOGW(TAG, "%s: not implemented", __func__);
    if (data) {
        data->als_lux      = 0;
        data->ps_proximity = 0;
    }
    return ESP_ERR_NOT_SUPPORTED;
}
