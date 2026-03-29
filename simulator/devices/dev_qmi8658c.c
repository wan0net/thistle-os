/*
 * QMI8658C 6-axis IMU — virtual I2C device model.
 * 256 registers. Accel/gyro data computed from injectable float values.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include <string.h>
#include <stdio.h>

#define QMI8658C_NUM_REGS 256

/* Register addresses */
#define REG_WHO_AM_I    0x00
#define REG_REVISION_ID 0x01
#define REG_CTRL1       0x02
#define REG_STATUS0     0x2E
#define REG_AX_L        0x35
#define REG_AX_H        0x36
#define REG_AY_L        0x37
#define REG_AY_H        0x38
#define REG_AZ_L        0x39
#define REG_AZ_H        0x3A
#define REG_GX_L        0x3B
#define REG_GX_H        0x3C
#define REG_GY_L        0x3D
#define REG_GY_H        0x3E
#define REG_GZ_L        0x3F
#define REG_GZ_H        0x40

/* Default: +/-8g range => scale = 8*9.80665/32768 */
#define ACCEL_SCALE (8.0f * 9.80665f / 32768.0f)
/* Default: +/-2048 dps range => scale = 2048/32768 */
#define GYRO_SCALE  (2048.0f / 32768.0f)

typedef struct {
    uint8_t regs[QMI8658C_NUM_REGS];
    float accel[3]; /* m/s^2 */
    float gyro[3];  /* deg/s */
} qmi8658c_model_t;

static qmi8658c_model_t qmi8658c_model;

static void qmi8658c_update_data_regs(qmi8658c_model_t *m)
{
    /* Convert float values to 16-bit raw */
    for (int i = 0; i < 3; i++) {
        int16_t raw_a = (int16_t)(m->accel[i] / ACCEL_SCALE);
        m->regs[REG_AX_L + i * 2]     = (uint8_t)(raw_a & 0xFF);
        m->regs[REG_AX_L + i * 2 + 1] = (uint8_t)((raw_a >> 8) & 0xFF);

        int16_t raw_g = (int16_t)(m->gyro[i] / GYRO_SCALE);
        m->regs[REG_GX_L + i * 2]     = (uint8_t)(raw_g & 0xFF);
        m->regs[REG_GX_L + i * 2 + 1] = (uint8_t)((raw_g >> 8) & 0xFF);
    }
}

static esp_err_t qmi8658c_on_read(sim_i2c_device_t *dev,
                                   const uint8_t *tx, size_t tx_len,
                                   uint8_t *rx, size_t rx_len)
{
    qmi8658c_model_t *m = (qmi8658c_model_t *)dev->model;
    if (tx_len < 1) { memset(rx, 0, rx_len); return ESP_OK; }
    uint8_t reg = tx[0];

    /* Refresh data registers before reading them */
    if (reg <= REG_GZ_H && (reg + rx_len) > REG_AX_L) {
        qmi8658c_update_data_regs(m);
    }

    /* Auto-increment read */
    for (size_t i = 0; i < rx_len; i++) {
        rx[i] = m->regs[(uint8_t)(reg + i)];
    }
    return ESP_OK;
}

static esp_err_t qmi8658c_on_write(sim_i2c_device_t *dev,
                                    const uint8_t *buf, size_t len)
{
    qmi8658c_model_t *m = (qmi8658c_model_t *)dev->model;
    if (len < 2) return ESP_OK;
    uint8_t reg = buf[0];

    /* Auto-increment write (skip read-only registers) */
    for (size_t i = 1; i < len; i++) {
        uint8_t r = (uint8_t)(reg + (i - 1));
        /* Skip read-only: WHO_AM_I, REVISION_ID, STATUS, data regs */
        if (r <= 0x01 || r == REG_STATUS0 || (r >= REG_AX_L && r <= REG_GZ_H)) {
            continue;
        }
        m->regs[r] = buf[i];
    }
    return ESP_OK;
}

static const sim_i2c_device_ops_t qmi8658c_ops = {
    .on_read  = qmi8658c_on_read,
    .on_write = qmi8658c_on_write,
};

void dev_qmi8658c_register(int bus_index, uint16_t addr)
{
    memset(&qmi8658c_model, 0, sizeof(qmi8658c_model));

    /* Fixed identification */
    qmi8658c_model.regs[REG_WHO_AM_I]    = 0x05;
    qmi8658c_model.regs[REG_REVISION_ID] = 0x7C;
    /* STATUS0: accel + gyro data available */
    qmi8658c_model.regs[REG_STATUS0]     = 0x03;

    /* Default: device at rest, Z-axis = 1g */
    qmi8658c_model.accel[0] = 0.0f;
    qmi8658c_model.accel[1] = 0.0f;
    qmi8658c_model.accel[2] = 9.80665f;
    qmi8658c_model.gyro[0]  = 0.0f;
    qmi8658c_model.gyro[1]  = 0.0f;
    qmi8658c_model.gyro[2]  = 0.0f;

    qmi8658c_update_data_regs(&qmi8658c_model);

    sim_i2c_bus_add_model(bus_index, addr, &qmi8658c_ops, &qmi8658c_model);
    printf("[sim] QMI8658C IMU registered on I2C bus %d addr 0x%02X\n",
           bus_index, addr);
}

void dev_qmi8658c_set_accel(float x, float y, float z)
{
    qmi8658c_model.accel[0] = x;
    qmi8658c_model.accel[1] = y;
    qmi8658c_model.accel[2] = z;
}

void dev_qmi8658c_set_gyro(float x, float y, float z)
{
    qmi8658c_model.gyro[0] = x;
    qmi8658c_model.gyro[1] = y;
    qmi8658c_model.gyro[2] = z;
}
