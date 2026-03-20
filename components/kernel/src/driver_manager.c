#include "thistle/driver_manager.h"

#include "hal/board.h"

#include "esp_log.h"

static const char *TAG = "drv_mgr";

esp_err_t driver_manager_init(void)
{
    ESP_LOGI(TAG, "Calling board_init()");
    esp_err_t ret = board_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "board_init() failed: %s", esp_err_to_name(ret));
        return ret;
    }
    const hal_registry_t *reg = hal_get_registry();
    ESP_LOGI(TAG, "Board: %s", reg->board_name ? reg->board_name : "(unknown)");
    return ESP_OK;
}

esp_err_t driver_manager_start_all(void)
{
    const hal_registry_t *reg = hal_get_registry();
    esp_err_t ret;

    /* Display */
    if (reg->display != NULL && reg->display->init != NULL) {
        ESP_LOGI(TAG, "Initializing display driver: %s", reg->display->name ? reg->display->name : "?");
        ret = reg->display->init(reg->display_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "Display init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* Input drivers */
    for (int i = 0; i < reg->input_count; i++) {
        if (reg->inputs[i] != NULL && reg->inputs[i]->init != NULL) {
            ESP_LOGI(TAG, "Initializing input driver[%d]: %s", i,
                     reg->inputs[i]->name ? reg->inputs[i]->name : "?");
            ret = reg->inputs[i]->init(reg->input_configs[i]);
            if (ret != ESP_OK) {
                ESP_LOGE(TAG, "Input driver[%d] init failed: %s", i, esp_err_to_name(ret));
                return ret;
            }
        }
    }

    /* Radio */
    if (reg->radio != NULL && reg->radio->init != NULL) {
        ESP_LOGI(TAG, "Initializing radio driver: %s", reg->radio->name ? reg->radio->name : "?");
        ret = reg->radio->init(reg->radio_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "Radio init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* GPS */
    if (reg->gps != NULL && reg->gps->init != NULL) {
        ESP_LOGI(TAG, "Initializing GPS driver: %s", reg->gps->name ? reg->gps->name : "?");
        ret = reg->gps->init(reg->gps_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "GPS init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* Audio */
    if (reg->audio != NULL && reg->audio->init != NULL) {
        ESP_LOGI(TAG, "Initializing audio driver: %s", reg->audio->name ? reg->audio->name : "?");
        ret = reg->audio->init(reg->audio_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "Audio init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* Power */
    if (reg->power != NULL && reg->power->init != NULL) {
        ESP_LOGI(TAG, "Initializing power driver: %s", reg->power->name ? reg->power->name : "?");
        ret = reg->power->init(reg->power_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "Power init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* IMU */
    if (reg->imu != NULL && reg->imu->init != NULL) {
        ESP_LOGI(TAG, "Initializing IMU driver: %s", reg->imu->name ? reg->imu->name : "?");
        ret = reg->imu->init(reg->imu_config);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "IMU init failed: %s", esp_err_to_name(ret));
            return ret;
        }
    }

    /* Storage drivers */
    for (int i = 0; i < reg->storage_count; i++) {
        if (reg->storage[i] != NULL && reg->storage[i]->init != NULL) {
            ESP_LOGI(TAG, "Initializing storage driver[%d]: %s", i,
                     reg->storage[i]->name ? reg->storage[i]->name : "?");
            ret = reg->storage[i]->init(reg->storage_configs[i]);
            if (ret != ESP_OK) {
                ESP_LOGE(TAG, "Storage driver[%d] init failed: %s", i, esp_err_to_name(ret));
                return ret;
            }
        }
    }

    /* Mount storage devices */
    for (int i = 0; i < reg->storage_count; i++) {
        if (reg->storage[i] != NULL && reg->storage[i]->mount != NULL) {
            ESP_LOGI(TAG, "Mounting storage[%d]: %s", i,
                     reg->storage[i]->name ? reg->storage[i]->name : "?");
            ret = reg->storage[i]->mount(NULL);  /* use default mount point */
            if (ret != ESP_OK) {
                ESP_LOGW(TAG, "Storage mount failed (non-fatal): %s", esp_err_to_name(ret));
                /* Non-fatal — continue without SD */
            }
        }
    }

    ESP_LOGI(TAG, "All drivers started");
    return ESP_OK;
}

esp_err_t driver_manager_stop_all(void)
{
    const hal_registry_t *reg = hal_get_registry();

    /* Deinit in reverse order of init */

    /* Storage drivers */
    for (int i = (int)reg->storage_count - 1; i >= 0; i--) {
        if (reg->storage[i] != NULL && reg->storage[i]->deinit != NULL) {
            ESP_LOGI(TAG, "Deinitializing storage driver[%d]: %s", i,
                     reg->storage[i]->name ? reg->storage[i]->name : "?");
            reg->storage[i]->deinit();
        }
    }

    /* IMU */
    if (reg->imu != NULL && reg->imu->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing IMU");
        reg->imu->deinit();
    }

    /* Power */
    if (reg->power != NULL && reg->power->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing power driver");
        reg->power->deinit();
    }

    /* Audio */
    if (reg->audio != NULL && reg->audio->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing audio driver");
        reg->audio->deinit();
    }

    /* GPS */
    if (reg->gps != NULL && reg->gps->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing GPS");
        reg->gps->deinit();
    }

    /* Radio */
    if (reg->radio != NULL && reg->radio->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing radio");
        reg->radio->deinit();
    }

    /* Input drivers */
    for (int i = (int)reg->input_count - 1; i >= 0; i--) {
        if (reg->inputs[i] != NULL && reg->inputs[i]->deinit != NULL) {
            ESP_LOGI(TAG, "Deinitializing input driver[%d]", i);
            reg->inputs[i]->deinit();
        }
    }

    /* Display */
    if (reg->display != NULL && reg->display->deinit != NULL) {
        ESP_LOGI(TAG, "Deinitializing display");
        reg->display->deinit();
    }

    ESP_LOGI(TAG, "All drivers stopped");
    return ESP_OK;
}
