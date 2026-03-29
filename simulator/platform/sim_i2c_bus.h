/*
 * Virtual I2C bus for simulator — routes transactions to device models.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>

/* Device model callbacks */
typedef struct sim_i2c_device sim_i2c_device_t;

typedef struct {
    /* Called on i2c_master_transmit_receive (register read).
     * tx contains register address(es), fill rx with data. */
    esp_err_t (*on_read)(sim_i2c_device_t *dev,
                         const uint8_t *tx, size_t tx_len,
                         uint8_t *rx, size_t rx_len);
    /* Called on i2c_master_transmit (register write).
     * buf[0] is typically register address, buf[1..] is data. */
    esp_err_t (*on_write)(sim_i2c_device_t *dev,
                          const uint8_t *buf, size_t len);
} sim_i2c_device_ops_t;

struct sim_i2c_device {
    uint16_t address;
    const sim_i2c_device_ops_t *ops;
    void *model;   /* Device-specific state (e.g., register file) */
};

#define SIM_I2C_MAX_BUSES   2
#define SIM_I2C_MAX_DEVICES 8

/* Initialize the virtual bus system */
void sim_i2c_bus_init(void);

/* Get opaque bus handle for HAL registration */
void *sim_i2c_bus_get(int index);

/* Register a device model at a given address on a given bus */
esp_err_t sim_i2c_bus_add_model(int bus_index, uint16_t address,
                                const sim_i2c_device_ops_t *ops, void *model);
