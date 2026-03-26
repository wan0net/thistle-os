// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — LTR-553ALS light/proximity sensor driver
#include "drv_light_ltr553.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "ltr553";

// LTR-553ALS I2C register map
#define LTR553_REG_ALS_CONTR   0x80
#define LTR553_REG_PS_CONTR    0x81
#define LTR553_REG_PS_LED      0x82
#define LTR553_REG_PS_MEAS_RATE 0x84
#define LTR553_REG_ALS_MEAS_RATE 0x85
#define LTR553_REG_PART_ID     0x86   // Expected: 0x92
#define LTR553_REG_MANUFAC_ID  0x87   // Expected: 0x05
#define LTR553_REG_ALS_DATA_CH1_L 0x88
#define LTR553_REG_ALS_DATA_CH1_H 0x89
#define LTR553_REG_ALS_DATA_CH0_L 0x8A
#define LTR553_REG_ALS_DATA_CH0_H 0x8B
#define LTR553_REG_ALS_STATUS  0x8C
#define LTR553_REG_PS_DATA_L   0x8D
#define LTR553_REG_PS_DATA_H   0x8E

// Configuration register values
#define LTR553_ALS_CONTR_ACTIVE   0x01   // Active mode, 1x gain
#define LTR553_PS_CONTR_ACTIVE    0x03   // Active mode
#define LTR553_PS_LED_CONFIG      0x7B   // 100mA, 100% duty, 60kHz
#define LTR553_ALS_MEAS_RATE      0x03   // 100ms integration, 500ms rate
#define LTR553_PS_MEAS_RATE       0x00   // 50ms rate

// Expected chip IDs
#define LTR553_EXPECTED_PART_ID  0x92
#define LTR553_EXPECTED_MANUFAC  0x05

// I2C timeout in milliseconds
#define LTR553_I2C_TIMEOUT 50

static light_ltr553_config_t  s_config;
static i2c_master_dev_handle_t s_dev = NULL;

// ---------------------------------------------------------------------------
// Helper functions for I2C register access
// ---------------------------------------------------------------------------

static esp_err_t ltr553_write_reg(uint8_t reg, uint8_t val)
{
    if (!s_dev) {
        return ESP_ERR_INVALID_STATE;
    }
    uint8_t buf[2] = { reg, val };
    return i2c_master_transmit(s_dev, buf, 2, LTR553_I2C_TIMEOUT);
}

static esp_err_t ltr553_read_reg(uint8_t reg, uint8_t *val)
{
    if (!s_dev || !val) {
        return ESP_ERR_INVALID_STATE;
    }
    return i2c_master_transmit_receive(s_dev, &reg, 1, val, 1, LTR553_I2C_TIMEOUT);
}

static esp_err_t ltr553_read_regs(uint8_t reg, uint8_t *buf, size_t len)
{
    if (!s_dev || !buf) {
        return ESP_ERR_INVALID_STATE;
    }
    return i2c_master_transmit_receive(s_dev, &reg, 1, buf, len, LTR553_I2C_TIMEOUT);
}

// ---------------------------------------------------------------------------
// ALS lux calculation with integer arithmetic
// ---------------------------------------------------------------------------

static uint16_t ltr553_calculate_lux(uint16_t ch0, uint16_t ch1)
{
    // Avoid division by zero
    if ((ch0 + ch1) == 0) {
        return 0;
    }

    // Calculate ratio * 1000 for fixed-point arithmetic
    uint32_t ratio_1000 = (ch1 * 1000) / (ch0 + ch1);

    uint32_t lux_1000;

    // Coefficients multiplied by 1000 for integer math
    if (ratio_1000 < 450) {
        // lux = 1.7743 * ch0 + 1.1059 * ch1
        lux_1000 = (1774 * ch0 + 1106 * ch1) / 1000;
    } else if (ratio_1000 < 640) {
        // lux = 4.2785 * ch0 - 1.9548 * ch1
        lux_1000 = (4279 * ch0 - 1955 * ch1) / 1000;
    } else if (ratio_1000 < 850) {
        // lux = 0.5926 * ch0 + 0.1185 * ch1
        lux_1000 = (593 * ch0 + 119 * ch1) / 1000;
    } else {
        // lux = 0
        lux_1000 = 0;
    }

    // Clamp to uint16_t max
    if (lux_1000 > 65535) {
        return 65535;
    }
    return (uint16_t)lux_1000;
}

// ---------------------------------------------------------------------------
// Public API implementations
// ---------------------------------------------------------------------------

esp_err_t drv_ltr553_init(const light_ltr553_config_t *config)
{
    if (!config || !config->i2c_bus) {
        ESP_LOGE(TAG, "Invalid config parameter");
        return ESP_ERR_INVALID_ARG;
    }

    // Store config
    memcpy(&s_config, config, sizeof(s_config));

    // Configure I2C device structure
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = config->i2c_addr,
        .scl_speed_hz = 400000,  // 400 kHz
    };

    // Add device to I2C bus
    esp_err_t ret = i2c_master_bus_add_device(config->i2c_bus, &dev_cfg, &s_dev);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to add I2C device: 0x%x", ret);
        s_dev = NULL;
        return ret;
    }

    // Verify part ID register (0x86) == 0x92
    uint8_t part_id;
    ret = ltr553_read_reg(LTR553_REG_PART_ID, &part_id);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read PART_ID register: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    if (part_id != LTR553_EXPECTED_PART_ID) {
        ESP_LOGE(TAG, "Invalid PART_ID: 0x%02x (expected 0x%02x)", part_id, LTR553_EXPECTED_PART_ID);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ESP_ERR_NOT_FOUND;
    }

    // Enable ALS: write 0x01 to ALS_CONTR (0x80) - active mode, 1x gain
    ret = ltr553_write_reg(LTR553_REG_ALS_CONTR, LTR553_ALS_CONTR_ACTIVE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to enable ALS: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    // Enable PS: write 0x03 to PS_CONTR (0x81) - active mode
    ret = ltr553_write_reg(LTR553_REG_PS_CONTR, LTR553_PS_CONTR_ACTIVE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to enable PS: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    // Configure PS LED: write 0x7B to PS_LED (0x82) - 100mA, 100% duty, 60kHz
    ret = ltr553_write_reg(LTR553_REG_PS_LED, LTR553_PS_LED_CONFIG);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to configure PS LED: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    // Set ALS measurement rate: write 0x03 to ALS_MEAS_RATE (0x85)
    ret = ltr553_write_reg(LTR553_REG_ALS_MEAS_RATE, LTR553_ALS_MEAS_RATE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set ALS measurement rate: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    // Set PS measurement rate: write 0x00 to PS_MEAS_RATE (0x84)
    ret = ltr553_write_reg(LTR553_REG_PS_MEAS_RATE, LTR553_PS_MEAS_RATE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set PS measurement rate: 0x%x", ret);
        i2c_master_bus_rm_device(s_dev);
        s_dev = NULL;
        return ret;
    }

    ESP_LOGI(TAG, "LTR-553ALS initialized successfully (addr=0x%02x, part_id=0x%02x)",
             config->i2c_addr, part_id);
    return ESP_OK;
}

void drv_ltr553_deinit(void)
{
    if (!s_dev) {
        return;
    }

    // Write 0x00 to ALS_CONTR to put sensor in standby
    ltr553_write_reg(LTR553_REG_ALS_CONTR, 0x00);

    // Write 0x00 to PS_CONTR to put sensor in standby
    ltr553_write_reg(LTR553_REG_PS_CONTR, 0x00);

    // Remove I2C device
    i2c_master_bus_rm_device(s_dev);
    s_dev = NULL;

    ESP_LOGI(TAG, "LTR-553ALS deinitialized");
}

esp_err_t drv_ltr553_read(ltr553_data_t *data)
{
    if (!data) {
        return ESP_ERR_INVALID_ARG;
    }

    if (!s_dev) {
        ESP_LOGE(TAG, "Sensor not initialized");
        return ESP_ERR_INVALID_STATE;
    }

    esp_err_t ret;
    uint8_t buf[4];

    // Read 4 bytes starting at ALS_DATA_CH1_L (0x88): ch1_l, ch1_h, ch0_l, ch0_h
    ret = ltr553_read_regs(LTR553_REG_ALS_DATA_CH1_L, buf, 4);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read ALS data: 0x%x", ret);
        return ret;
    }

    // Combine into ch0 and ch1 (16-bit each, little-endian)
    uint16_t ch1 = (buf[1] << 8) | buf[0];
    uint16_t ch0 = (buf[3] << 8) | buf[2];

    // Calculate lux from channels
    data->als_lux = ltr553_calculate_lux(ch0, ch1);

    // Read 2 bytes at PS_DATA_L (0x8D): ps_l, ps_h
    uint8_t ps_buf[2];
    ret = ltr553_read_regs(LTR553_REG_PS_DATA_L, ps_buf, 2);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read PS data: 0x%x", ret);
        return ret;
    }

    // Combine into 11-bit proximity value: (ps_h & 0x07) << 8 | ps_l
    data->ps_proximity = ((ps_buf[1] & 0x07) << 8) | ps_buf[0];

    return ESP_OK;
}
