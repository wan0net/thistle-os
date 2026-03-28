// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Bosch BHI260AP IMU driver
#include "drv_imu_bhi260ap.h"
#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <string.h>

static const char *TAG = "bhi260ap";

// ---------------------------------------------------------------------------
// BHI260AP register map (BHY2 host interface)
// ---------------------------------------------------------------------------

#define BHI260_REG_CHIP_ID          0x01   // Expected: 0x89
#define BHI260_REG_REVISION_ID      0x02
#define BHI260_REG_BOOT_STATUS      0x0A
#define BHI260_REG_INT_STATUS       0x0E
#define BHI260_REG_RESET_REQ        0x1C

#define BHI260_REG_NW_FIFO_AVAIL    0x2A   // Non-wakeup FIFO available (16-bit LE)
#define BHI260_REG_NW_FIFO_DATA     0x34   // Non-wakeup FIFO data register

#define BHI260_REG_PARAM_WRITE_BUF  0x40   // Parameter write buffer (8 bytes)
#define BHI260_REG_PARAM_READ_BUF   0x48   // Parameter read buffer (8 bytes)
#define BHI260_REG_PARAM_REQ        0x50   // Parameter request (16-bit)
#define BHI260_REG_PARAM_ACK        0x52   // Parameter acknowledge (16-bit)

// Expected chip ID
#define BHI260_EXPECTED_CHIP_ID     0x89

// Boot status bits
#define BHI260_BOOT_HOST_IF_READY   (1 << 4)

// Virtual sensor IDs
#define BHI260_VSENSOR_ACCEL        1
#define BHI260_VSENSOR_GYRO         3

// I2C timeout in milliseconds
#define BHI260_I2C_TIMEOUT          50

// Default sample rate
#define BHI260_DEFAULT_SAMPLE_RATE  100.0f

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

static struct {
    imu_bhi260ap_config_t   config;
    i2c_master_dev_handle_t dev;
    hal_imu_cb_t            callback;
    void                   *cb_user_data;
    bool                    initialized;
    bool                    sensors_active;
    float                   current_sample_rate;
} s_state;

// ---------------------------------------------------------------------------
// I2C helpers
// ---------------------------------------------------------------------------

static esp_err_t bhi260_write_reg(uint8_t reg, const uint8_t *data, size_t len)
{
    if (!s_state.dev) {
        return ESP_ERR_INVALID_STATE;
    }

    // Build buffer: [reg, data...]
    if (len > 63) return ESP_ERR_INVALID_SIZE;
    uint8_t buf[64];
    buf[0] = reg;
    memcpy(&buf[1], data, len);
    return i2c_master_transmit(s_state.dev, buf, len + 1, BHI260_I2C_TIMEOUT);
}

static esp_err_t bhi260_write_reg8(uint8_t reg, uint8_t val)
{
    return bhi260_write_reg(reg, &val, 1);
}

static esp_err_t bhi260_read_reg(uint8_t reg, uint8_t *data, size_t len)
{
    if (!s_state.dev || !data) {
        return ESP_ERR_INVALID_STATE;
    }
    return i2c_master_transmit_receive(s_state.dev, &reg, 1, data, len, BHI260_I2C_TIMEOUT);
}

// ---------------------------------------------------------------------------
// Virtual sensor configuration via parameter page
// ---------------------------------------------------------------------------

static esp_err_t bhi260_set_virtual_sensor(uint8_t sensor_id, float sample_rate_hz)
{
    // Write config to parameter write buffer (0x40-0x47)
    // Format: [sample_rate:4LE float][latency:4LE uint32 = 0]
    uint8_t buf[8] = {0};
    memcpy(buf, &sample_rate_hz, 4);
    // latency = 0 (bytes 4-7 already zeroed)

    esp_err_t ret = bhi260_write_reg(BHI260_REG_PARAM_WRITE_BUF, buf, 8);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to write param buffer: 0x%x", ret);
        return ret;
    }

    // Request parameter write: page 5, index = sensor_id
    uint16_t req = (5 << 8) | sensor_id;
    uint8_t req_bytes[2] = { req & 0xFF, (req >> 8) & 0xFF };
    ret = bhi260_write_reg(BHI260_REG_PARAM_REQ, req_bytes, 2);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to write param request: 0x%x", ret);
        return ret;
    }

    // Wait for acknowledge
    uint8_t ack[2] = {0};
    for (int i = 0; i < 100; i++) {
        vTaskDelay(pdMS_TO_TICKS(1));
        ret = bhi260_read_reg(BHI260_REG_PARAM_ACK, ack, 2);
        if (ret != ESP_OK) {
            continue;
        }
        if (ack[0] == req_bytes[0] && ack[1] == req_bytes[1]) {
            return ESP_OK;
        }
    }

    ESP_LOGE(TAG, "Param write timeout for sensor %d", sensor_id);
    return ESP_ERR_TIMEOUT;
}

// ---------------------------------------------------------------------------
// Vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t bhi260ap_init(const void *config)
{
    if (!config) {
        ESP_LOGE(TAG, "Invalid config parameter");
        return ESP_ERR_INVALID_ARG;
    }

    const imu_bhi260ap_config_t *cfg = (const imu_bhi260ap_config_t *)config;
    if (!cfg->i2c_bus) {
        ESP_LOGE(TAG, "Invalid I2C bus handle");
        return ESP_ERR_INVALID_ARG;
    }

    // Store config
    memcpy(&s_state.config, cfg, sizeof(s_state.config));

    // Configure I2C device
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = cfg->i2c_addr,
        .scl_speed_hz = 400000,
    };

    esp_err_t ret = i2c_master_bus_add_device(cfg->i2c_bus, &dev_cfg, &s_state.dev);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to add I2C device: 0x%x", ret);
        s_state.dev = NULL;
        return ret;
    }

    // Read and verify chip ID
    uint8_t chip_id = 0;
    ret = bhi260_read_reg(BHI260_REG_CHIP_ID, &chip_id, 1);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read chip ID: 0x%x", ret);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ret;
    }

    if (chip_id != BHI260_EXPECTED_CHIP_ID) {
        ESP_LOGE(TAG, "Invalid chip ID: 0x%02x (expected 0x%02x)", chip_id, BHI260_EXPECTED_CHIP_ID);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ESP_ERR_NOT_FOUND;
    }

    ESP_LOGI(TAG, "Chip ID verified: 0x%02x", chip_id);

    // Soft reset
    ret = bhi260_write_reg8(BHI260_REG_RESET_REQ, 0x01);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to issue soft reset: 0x%x", ret);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ret;
    }

    vTaskDelay(pdMS_TO_TICKS(100));

    // Check boot status — bit 4 should be set (host interface ready)
    uint8_t boot_status = 0;
    for (int i = 0; i < 50; i++) {
        ret = bhi260_read_reg(BHI260_REG_BOOT_STATUS, &boot_status, 1);
        if (ret == ESP_OK && (boot_status & BHI260_BOOT_HOST_IF_READY)) {
            break;
        }
        vTaskDelay(pdMS_TO_TICKS(10));
    }

    if (!(boot_status & BHI260_BOOT_HOST_IF_READY)) {
        ESP_LOGE(TAG, "Host interface not ready (boot_status=0x%02x)", boot_status);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ESP_ERR_TIMEOUT;
    }

    ESP_LOGD(TAG, "Boot status: 0x%02x", boot_status);

    // Configure virtual sensors: accel + gyro at default rate
    float rate = BHI260_DEFAULT_SAMPLE_RATE;

    ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_ACCEL, rate);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to configure accelerometer: 0x%x", ret);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ret;
    }

    ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_GYRO, rate);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to configure gyroscope: 0x%x", ret);
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
        return ret;
    }

    s_state.initialized = true;
    s_state.sensors_active = true;
    s_state.current_sample_rate = rate;

    ESP_LOGI(TAG, "BHI260AP initialized (addr=0x%02x, rate=%.0fHz)", cfg->i2c_addr, rate);
    return ESP_OK;
}

static void bhi260ap_deinit(void)
{
    if (!s_state.initialized) {
        return;
    }

    // Disable virtual sensors (rate = 0)
    bhi260_set_virtual_sensor(BHI260_VSENSOR_ACCEL, 0.0f);
    bhi260_set_virtual_sensor(BHI260_VSENSOR_GYRO, 0.0f);

    // Remove I2C device
    if (s_state.dev) {
        i2c_master_bus_rm_device(s_state.dev);
        s_state.dev = NULL;
    }

    // Clear state
    memset(&s_state, 0, sizeof(s_state));

    ESP_LOGI(TAG, "BHI260AP deinitialized");
}

static esp_err_t bhi260ap_get_data(hal_imu_data_t *data)
{
    if (!s_state.initialized || !data) {
        return ESP_ERR_INVALID_STATE;
    }

    memset(data, 0, sizeof(*data));

    // Check non-wakeup FIFO available bytes
    uint8_t avail[2];
    esp_err_t ret = bhi260_read_reg(BHI260_REG_NW_FIFO_AVAIL, avail, 2);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read FIFO available: 0x%x", ret);
        return ret;
    }

    uint16_t fifo_len = avail[0] | (avail[1] << 8);
    if (fifo_len == 0) {
        return ESP_OK;  // No new data
    }

    // Read FIFO data (max 256 bytes at a time)
    uint8_t fifo[256];
    uint16_t to_read = fifo_len > sizeof(fifo) ? sizeof(fifo) : fifo_len;
    ret = bhi260_read_reg(BHI260_REG_NW_FIFO_DATA, fifo, to_read);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to read FIFO data: 0x%x", ret);
        return ret;
    }

    // Parse FIFO frames
    // BHY2 format: [sensor_id:1][payload...]
    // Accel (ID 1): 7 bytes = [x_lsb, x_msb, y_lsb, y_msb, z_lsb, z_msb, status]
    // Gyro  (ID 3): 7 bytes = same format
    size_t pos = 0;
    while (pos < to_read) {
        uint8_t sid = fifo[pos++];
        if (sid == 0 || pos >= to_read) {
            break;
        }

        if (sid == BHI260_VSENSOR_ACCEL && pos + 6 <= to_read) {
            // Accelerometer: 16-bit signed, scale = 1/4096 g -> m/s^2
            int16_t ax = (int16_t)(fifo[pos]     | (fifo[pos + 1] << 8));
            int16_t ay = (int16_t)(fifo[pos + 2] | (fifo[pos + 3] << 8));
            int16_t az = (int16_t)(fifo[pos + 4] | (fifo[pos + 5] << 8));
            data->accel_x = ax / 4096.0f * 9.80665f;
            data->accel_y = ay / 4096.0f * 9.80665f;
            data->accel_z = az / 4096.0f * 9.80665f;
            pos += 7;  // 6 data + 1 status
        } else if (sid == BHI260_VSENSOR_GYRO && pos + 6 <= to_read) {
            // Gyroscope: 16-bit signed, scale = 1/16.4 -> deg/s
            int16_t gx = (int16_t)(fifo[pos]     | (fifo[pos + 1] << 8));
            int16_t gy = (int16_t)(fifo[pos + 2] | (fifo[pos + 3] << 8));
            int16_t gz = (int16_t)(fifo[pos + 4] | (fifo[pos + 5] << 8));
            data->gyro_x = gx / 16.4f;
            data->gyro_y = gy / 16.4f;
            data->gyro_z = gz / 16.4f;
            pos += 7;
        } else {
            // Unknown sensor ID or insufficient data — stop parsing
            break;
        }
    }

    return ESP_OK;
}

static esp_err_t bhi260ap_register_callback(hal_imu_cb_t cb, void *user_data)
{
    s_state.callback = cb;
    s_state.cb_user_data = user_data;
    return ESP_OK;
}

static esp_err_t bhi260ap_set_sample_rate(uint16_t hz)
{
    if (!s_state.initialized) {
        return ESP_ERR_INVALID_STATE;
    }

    float rate = (float)hz;

    esp_err_t ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_ACCEL, rate);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set accel sample rate: 0x%x", ret);
        return ret;
    }

    ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_GYRO, rate);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set gyro sample rate: 0x%x", ret);
        return ret;
    }

    s_state.current_sample_rate = rate;
    s_state.sensors_active = (hz > 0);

    ESP_LOGI(TAG, "Sample rate set to %u Hz", hz);
    return ESP_OK;
}

static esp_err_t bhi260ap_sleep(bool enter)
{
    if (!s_state.initialized) {
        return ESP_ERR_INVALID_STATE;
    }

    if (enter) {
        // Disable virtual sensors (rate = 0)
        esp_err_t ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_ACCEL, 0.0f);
        if (ret != ESP_OK) return ret;
        ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_GYRO, 0.0f);
        if (ret != ESP_OK) return ret;
        s_state.sensors_active = false;
        ESP_LOGI(TAG, "Entered sleep mode");
    } else {
        // Re-enable with previous sample rate
        float rate = s_state.current_sample_rate;
        if (rate <= 0.0f) {
            rate = BHI260_DEFAULT_SAMPLE_RATE;
        }
        esp_err_t ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_ACCEL, rate);
        if (ret != ESP_OK) return ret;
        ret = bhi260_set_virtual_sensor(BHI260_VSENSOR_GYRO, rate);
        if (ret != ESP_OK) return ret;
        s_state.sensors_active = true;
        ESP_LOGI(TAG, "Exited sleep mode (rate=%.0fHz)", rate);
    }

    return ESP_OK;
}

// ---------------------------------------------------------------------------
// Vtable + get
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
