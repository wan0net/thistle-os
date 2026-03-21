#include "hal/board.h"
#include "esp_log.h"
#include "esp_err.h"
#include <string.h>

static const char *TAG = "hal";

static hal_registry_t s_registry = {
    .display       = NULL,
    .inputs        = { NULL },
    .input_count   = 0,
    .radio         = NULL,
    .gps           = NULL,
    .audio         = NULL,
    .power         = NULL,
    .imu           = NULL,
    .storage       = { NULL },
    .storage_count = 0,
    .spi_bus       = { NULL },
    .spi_bus_count = 0,
    .i2c_bus       = { NULL },
    .i2c_bus_count = 0,
    .board_name    = NULL,
};

const hal_registry_t *hal_get_registry(void)
{
    return &s_registry;
}

esp_err_t hal_display_register(const hal_display_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.display = driver;
    s_registry.display_config = config;
    ESP_LOGI(TAG, "display driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_input_register(const hal_input_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_registry.input_count >= HAL_MAX_INPUT_DRIVERS) {
        ESP_LOGE(TAG, "input driver registration failed: max %d drivers already registered",
                 HAL_MAX_INPUT_DRIVERS);
        return ESP_ERR_NO_MEM;
    }
    uint8_t idx = s_registry.input_count;
    s_registry.inputs[idx] = driver;
    s_registry.input_configs[idx] = config;
    s_registry.input_count++;
    ESP_LOGI(TAG, "input driver registered: %s (slot %d)",
             driver->name ? driver->name : "(unnamed)", idx);
    return ESP_OK;
}

esp_err_t hal_radio_register(const hal_radio_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.radio = driver;
    s_registry.radio_config = config;
    ESP_LOGI(TAG, "radio driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_gps_register(const hal_gps_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.gps = driver;
    s_registry.gps_config = config;
    ESP_LOGI(TAG, "GPS driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_audio_register(const hal_audio_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.audio = driver;
    s_registry.audio_config = config;
    ESP_LOGI(TAG, "audio driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_power_register(const hal_power_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.power = driver;
    s_registry.power_config = config;
    ESP_LOGI(TAG, "power driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_imu_register(const hal_imu_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.imu = driver;
    s_registry.imu_config = config;
    ESP_LOGI(TAG, "IMU driver registered: %s", driver->name ? driver->name : "(unnamed)");
    return ESP_OK;
}

esp_err_t hal_storage_register(const hal_storage_driver_t *driver, const void *config)
{
    if (driver == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_registry.storage_count >= HAL_MAX_STORAGE_DRIVERS) {
        ESP_LOGE(TAG, "storage driver registration failed: max %d drivers already registered",
                 HAL_MAX_STORAGE_DRIVERS);
        return ESP_ERR_NO_MEM;
    }
    uint8_t idx = s_registry.storage_count;
    s_registry.storage[idx] = driver;
    s_registry.storage_configs[idx] = config;
    s_registry.storage_count++;
    ESP_LOGI(TAG, "storage driver registered: %s (slot %d)",
             driver->name ? driver->name : "(unnamed)", idx);
    return ESP_OK;
}

esp_err_t hal_set_board_name(const char *name)
{
    if (name == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_registry.board_name = name;
    ESP_LOGI(TAG, "board: %s", name);
    return ESP_OK;
}

esp_err_t hal_bus_register_spi(int host_id, void *bus_handle)
{
    if (!bus_handle) return ESP_ERR_INVALID_ARG;
    if (s_registry.spi_bus_count >= 2) {
        ESP_LOGE(TAG, "SPI bus registration failed: max 2 buses");
        return ESP_ERR_NO_MEM;
    }
    uint8_t idx = s_registry.spi_bus_count;
    s_registry.spi_bus[idx] = bus_handle;
    s_registry.spi_bus_count++;
    ESP_LOGI(TAG, "SPI bus %d registered (host %d)", idx, host_id);
    return ESP_OK;
}

esp_err_t hal_bus_register_i2c(int port, void *bus_handle)
{
    if (!bus_handle) return ESP_ERR_INVALID_ARG;
    if (s_registry.i2c_bus_count >= 2) {
        ESP_LOGE(TAG, "I2C bus registration failed: max 2 buses");
        return ESP_ERR_NO_MEM;
    }
    uint8_t idx = s_registry.i2c_bus_count;
    s_registry.i2c_bus[idx] = bus_handle;
    s_registry.i2c_bus_count++;
    ESP_LOGI(TAG, "I2C bus %d registered (port %d)", idx, port);
    return ESP_OK;
}

void *hal_bus_get_spi(int index)
{
    if (index < 0 || index >= s_registry.spi_bus_count) return NULL;
    return s_registry.spi_bus[index];
}

void *hal_bus_get_i2c(int index)
{
    if (index < 0 || index >= s_registry.i2c_bus_count) return NULL;
    return s_registry.i2c_bus[index];
}
