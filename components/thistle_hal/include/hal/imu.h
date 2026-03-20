#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    float accel_x, accel_y, accel_z;   // m/s^2
    float gyro_x, gyro_y, gyro_z;     // deg/s
    float mag_x, mag_y, mag_z;        // uT (if available)
} hal_imu_data_t;

typedef void (*hal_imu_cb_t)(const hal_imu_data_t *data, void *user_data);

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*get_data)(hal_imu_data_t *data);
    esp_err_t (*register_callback)(hal_imu_cb_t cb, void *user_data);
    esp_err_t (*set_sample_rate)(uint16_t hz);
    esp_err_t (*sleep)(bool enter);
    const char *name;
} hal_imu_driver_t;
