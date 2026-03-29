/*
 * CST328 capacitive touch controller — virtual I2C device model.
 * 2-byte register addressing. Supports single-touch injection.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include <string.h>
#include <stdio.h>

typedef struct {
    uint16_t touch_x;
    uint16_t touch_y;
    bool     touch_down;
} cst328_model_t;

static cst328_model_t cst328_model;

/*
 * CST328 uses 2-byte register addresses.
 * The driver sends 2 bytes in the write phase (reg_hi, reg_lo),
 * then reads back data.
 *
 * Key registers:
 *   0xD000 — touch count (1 byte): 0 or 1
 *   0xD001 — touch point 1 (7 bytes): X_H, X_L, Y_H, Y_L, pressure, area, id
 *   0xD100 — chip ID area (optional, returns 0)
 */

static esp_err_t cst328_on_read(sim_i2c_device_t *dev,
                                 const uint8_t *tx, size_t tx_len,
                                 uint8_t *rx, size_t rx_len)
{
    cst328_model_t *m = (cst328_model_t *)dev->model;

    if (tx_len < 2) { memset(rx, 0, rx_len); return ESP_OK; }

    uint16_t reg = (uint16_t)((tx[0] << 8) | tx[1]);

    if (reg == 0xD000) {
        /* Touch count register */
        rx[0] = m->touch_down ? 1 : 0;
        /* Zero-fill any extra bytes */
        for (size_t i = 1; i < rx_len; i++) rx[i] = 0;
    } else if (reg == 0xD001) {
        /* Touch point 1 data: X_H, X_L, Y_H, Y_L, pressure, area, id */
        uint8_t data[7] = {0};
        if (m->touch_down) {
            data[0] = (uint8_t)((m->touch_x >> 8) & 0x0F);
            data[1] = (uint8_t)(m->touch_x & 0xFF);
            data[2] = (uint8_t)((m->touch_y >> 8) & 0x0F);
            data[3] = (uint8_t)(m->touch_y & 0xFF);
            data[4] = 200;  /* pressure */
            data[5] = 50;   /* area */
            data[6] = 0;    /* id */
        }
        size_t copy = rx_len < 7 ? rx_len : 7;
        memcpy(rx, data, copy);
        for (size_t i = copy; i < rx_len; i++) rx[i] = 0;
    } else {
        /* Unknown register — return zeros */
        memset(rx, 0, rx_len);
    }

    return ESP_OK;
}

static esp_err_t cst328_on_write(sim_i2c_device_t *dev,
                                  const uint8_t *buf, size_t len)
{
    /* CST328 has very few writable registers (reset, mode).
     * For the simulator we accept but ignore writes. */
    (void)dev;
    (void)buf;
    (void)len;
    return ESP_OK;
}

static const sim_i2c_device_ops_t cst328_ops = {
    .on_read  = cst328_on_read,
    .on_write = cst328_on_write,
};

void dev_cst328_register(int bus_index, uint16_t addr)
{
    memset(&cst328_model, 0, sizeof(cst328_model));

    sim_i2c_bus_add_model(bus_index, addr, &cst328_ops, &cst328_model);
    printf("[sim] CST328 touch registered on I2C bus %d addr 0x%02X\n",
           bus_index, addr);
}

void dev_cst328_inject_touch(uint16_t x, uint16_t y, bool down)
{
    cst328_model.touch_x    = x;
    cst328_model.touch_y    = y;
    cst328_model.touch_down = down;
}
