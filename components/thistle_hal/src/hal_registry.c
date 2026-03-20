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
