/*
 * Virtual I2C bus — replaces ESP-IDF i2c_master_* with device model routing.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "sim_i2c_bus.h"
#include "driver/i2c_master.h"
#include <stdio.h>
#include <string.h>

typedef struct {
    sim_i2c_device_t devices[SIM_I2C_MAX_DEVICES];
    int device_count;
} sim_i2c_bus_t;

static sim_i2c_bus_t s_buses[SIM_I2C_MAX_BUSES];
static bool s_initialized = false;

void sim_i2c_bus_init(void)
{
    memset(s_buses, 0, sizeof(s_buses));
    s_initialized = true;
}

void *sim_i2c_bus_get(int index)
{
    if (index < 0 || index >= SIM_I2C_MAX_BUSES) return NULL;
    return &s_buses[index];
}

esp_err_t sim_i2c_bus_add_model(int bus_index, uint16_t address,
                                const sim_i2c_device_ops_t *ops, void *model)
{
    if (bus_index < 0 || bus_index >= SIM_I2C_MAX_BUSES) return ESP_ERR_INVALID_ARG;
    sim_i2c_bus_t *bus = &s_buses[bus_index];
    if (bus->device_count >= SIM_I2C_MAX_DEVICES) return ESP_ERR_NO_MEM;

    sim_i2c_device_t *dev = &bus->devices[bus->device_count++];
    dev->address = address;
    dev->ops = ops;
    dev->model = model;
    {
        char _msg[64];
        snprintf(_msg, sizeof(_msg), "registered device at 0x%02X", address);
        printf("[sim_i2c] bus %d: %s\n", bus_index, _msg);
        extern void sim_assert_check_line(const char *line);
        sim_assert_check_line(_msg);
    }
    return ESP_OK;
}

/* --- ESP-IDF API implementations --- */

esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *cfg,
                             i2c_master_bus_handle_t *handle)
{
    if (!cfg || !handle) return ESP_ERR_INVALID_ARG;
    int port = cfg->i2c_port;
    if (port < 0 || port >= SIM_I2C_MAX_BUSES) port = 0;
    *handle = (void *)&s_buses[port];
    return ESP_OK;
}

esp_err_t i2c_master_bus_add_device(i2c_master_bus_handle_t bus,
                                     const i2c_device_config_t *cfg,
                                     i2c_master_dev_handle_t *dev)
{
    if (!dev) return ESP_ERR_INVALID_ARG;

    /* If bus is a real sim_i2c_bus_t pointer, look up device by address */
    if (bus && s_initialized) {
        sim_i2c_bus_t *b = (sim_i2c_bus_t *)bus;
        /* Validate it's one of our buses */
        for (int bi = 0; bi < SIM_I2C_MAX_BUSES; bi++) {
            if (b == &s_buses[bi]) {
                uint16_t addr = cfg ? (uint16_t)cfg->device_address : 0;
                for (int i = 0; i < b->device_count; i++) {
                    if (b->devices[i].address == addr) {
                        *dev = (void *)&b->devices[i];
                        return ESP_OK;
                    }
                }
                break;
            }
        }
    }

    /* No model found — return sentinel (backward compat) */
    *dev = (void *)(uintptr_t)1;
    return ESP_OK;
}

esp_err_t i2c_master_bus_rm_device(i2c_master_dev_handle_t dev)
{
    (void)dev;
    return ESP_OK;
}

esp_err_t i2c_master_transmit_receive(i2c_master_dev_handle_t dev,
                                       const uint8_t *tx, size_t tx_len,
                                       uint8_t *rx, size_t rx_len,
                                       int timeout)
{
    (void)timeout;
    if (!rx || rx_len == 0) return ESP_OK;

    /* Sentinel handle — no model, zero-fill */
    if (dev == (void *)(uintptr_t)1) {
        memset(rx, 0, rx_len);
        return ESP_OK;
    }

    sim_i2c_device_t *d = (sim_i2c_device_t *)dev;
    if (d->ops && d->ops->on_read) {
        return d->ops->on_read(d, tx, tx_len, rx, rx_len);
    }
    memset(rx, 0, rx_len);
    return ESP_OK;
}

esp_err_t i2c_master_transmit(i2c_master_dev_handle_t dev,
                               const uint8_t *data, size_t len,
                               int timeout)
{
    (void)timeout;
    if (dev == (void *)(uintptr_t)1) return ESP_OK;

    sim_i2c_device_t *d = (sim_i2c_device_t *)dev;
    if (d->ops && d->ops->on_write) {
        return d->ops->on_write(d, data, len);
    }
    return ESP_OK;
}
