/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Simulator IMU HAL driver — returns static accelerometer/gyro/magnetometer data.
 */
#include "sim_imu.h"
#include <stddef.h>

static float s_accel_x = 0.0f;
static float s_accel_y = 0.0f;
static float s_accel_z = 9.81f;

static float s_gyro_x = 0.0f;
static float s_gyro_y = 0.0f;
static float s_gyro_z = 0.0f;

static hal_imu_cb_t s_cb      = NULL;
static void        *s_cb_user = NULL;

/* ---- vtable functions --------------------------------------------------- */

static esp_err_t sim_imu_init(const void *config)
{
    (void)config;
    return ESP_OK;
}

static void sim_imu_deinit(void)
{
}

static esp_err_t sim_imu_get_data(hal_imu_data_t *data)
{
    if (!data) return ESP_ERR_INVALID_ARG;

    data->accel_x = s_accel_x;
    data->accel_y = s_accel_y;
    data->accel_z = s_accel_z;
    data->gyro_x  = s_gyro_x;
    data->gyro_y  = s_gyro_y;
    data->gyro_z  = s_gyro_z;
    data->mag_x   = 0.0f;
    data->mag_y   = 0.0f;
    data->mag_z   = 0.0f;
    return ESP_OK;
}

static esp_err_t sim_imu_register_callback(hal_imu_cb_t cb, void *user_data)
{
    s_cb      = cb;
    s_cb_user = user_data;
    return ESP_OK;
}

static esp_err_t sim_imu_set_sample_rate(uint16_t hz)
{
    (void)hz;
    return ESP_OK;
}

static esp_err_t sim_imu_sleep(bool enter)
{
    (void)enter;
    return ESP_OK;
}

/* ---- driver instance ---------------------------------------------------- */

static const hal_imu_driver_t sim_imu_driver = {
    .init              = sim_imu_init,
    .deinit            = sim_imu_deinit,
    .get_data          = sim_imu_get_data,
    .register_callback = sim_imu_register_callback,
    .set_sample_rate   = sim_imu_set_sample_rate,
    .sleep             = sim_imu_sleep,
    .name              = "Simulator IMU",
};

const hal_imu_driver_t *sim_imu_get(void)
{
    return &sim_imu_driver;
}

void sim_imu_set_accel(float x, float y, float z)
{
    s_accel_x = x;
    s_accel_y = y;
    s_accel_z = z;
}

void sim_imu_set_gyro(float x, float y, float z)
{
    s_gyro_x = x;
    s_gyro_y = y;
    s_gyro_z = z;
}
