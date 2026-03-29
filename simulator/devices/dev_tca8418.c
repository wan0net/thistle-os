/*
 * TCA8418 keyboard matrix controller — virtual I2C device model.
 * Implements key event FIFO with injection from SDL events.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include <string.h>
#include <stdio.h>

#define TCA8418_NUM_REGS 256
#define TCA8418_FIFO_SIZE 10

/* Register addresses */
#define REG_KEY_LCK_EC  0x02  /* bits 3:0 = event count (read-only) */
#define REG_INT_STAT    0x03  /* bit 0 = K_INT (key event pending) */
#define REG_KEY_EVENT_A 0x04  /* next event from FIFO (read pops) */

typedef struct {
    uint8_t regs[TCA8418_NUM_REGS];
    /* Key event FIFO: bit 7 = press(1)/release(0), bits 6:0 = keycode */
    uint8_t fifo[TCA8418_FIFO_SIZE];
    uint8_t fifo_head;  /* next write position */
    uint8_t fifo_tail;  /* next read position */
    uint8_t fifo_count; /* number of events in FIFO */
} tca8418_model_t;

static tca8418_model_t tca8418_model;

static void tca8418_update_status(tca8418_model_t *m)
{
    /* KEY_LCK_EC bits 3:0 = event count */
    m->regs[REG_KEY_LCK_EC] = (m->regs[REG_KEY_LCK_EC] & 0xF0)
                               | (m->fifo_count & 0x0F);
    /* INT_STAT bit 0 = K_INT */
    if (m->fifo_count > 0) {
        m->regs[REG_INT_STAT] |= 0x01;
    } else {
        m->regs[REG_INT_STAT] &= (uint8_t)~0x01;
    }
}

static uint8_t tca8418_fifo_pop(tca8418_model_t *m)
{
    if (m->fifo_count == 0) return 0;
    uint8_t event = m->fifo[m->fifo_tail];
    m->fifo_tail = (uint8_t)((m->fifo_tail + 1) % TCA8418_FIFO_SIZE);
    m->fifo_count--;
    tca8418_update_status(m);
    return event;
}

static esp_err_t tca8418_on_read(sim_i2c_device_t *dev,
                                  const uint8_t *tx, size_t tx_len,
                                  uint8_t *rx, size_t rx_len)
{
    tca8418_model_t *m = (tca8418_model_t *)dev->model;
    if (tx_len < 1) { memset(rx, 0, rx_len); return ESP_OK; }
    uint8_t reg = tx[0];

    for (size_t i = 0; i < rx_len; i++) {
        uint8_t r = (uint8_t)(reg + i);
        if (r == REG_KEY_EVENT_A) {
            /* Reading KEY_EVENT_A pops from FIFO */
            rx[i] = tca8418_fifo_pop(m);
        } else {
            rx[i] = m->regs[r];
        }
    }
    return ESP_OK;
}

static esp_err_t tca8418_on_write(sim_i2c_device_t *dev,
                                   const uint8_t *buf, size_t len)
{
    tca8418_model_t *m = (tca8418_model_t *)dev->model;
    if (len < 2) return ESP_OK;
    uint8_t reg = buf[0];

    for (size_t i = 1; i < len; i++) {
        uint8_t r = (uint8_t)(reg + (i - 1));
        /* Skip read-only: KEY_LCK_EC event count bits, KEY_EVENT_A */
        if (r == REG_KEY_EVENT_A) continue;
        if (r == REG_INT_STAT) {
            /* Writing 1 to K_INT clears it (write-1-to-clear) */
            m->regs[r] &= ~(buf[i] & 0x01);
            continue;
        }
        m->regs[r] = buf[i];
    }
    return ESP_OK;
}

static const sim_i2c_device_ops_t tca8418_ops = {
    .on_read  = tca8418_on_read,
    .on_write = tca8418_on_write,
};

void dev_tca8418_register(int bus_index, uint16_t addr)
{
    memset(&tca8418_model, 0, sizeof(tca8418_model));
    tca8418_update_status(&tca8418_model);

    sim_i2c_bus_add_model(bus_index, addr, &tca8418_ops, &tca8418_model);
    printf("[sim] TCA8418 keyboard registered on I2C bus %d addr 0x%02X\n",
           bus_index, addr);
}

void dev_tca8418_inject_key(uint8_t keycode, bool press)
{
    tca8418_model_t *m = &tca8418_model;
    if (m->fifo_count >= TCA8418_FIFO_SIZE) {
        printf("[sim] TCA8418: FIFO full, dropping key event 0x%02X\n", keycode);
        return;
    }

    uint8_t event = (keycode & 0x7F);
    if (press) event |= 0x80;

    m->fifo[m->fifo_head] = event;
    m->fifo_head = (uint8_t)((m->fifo_head + 1) % TCA8418_FIFO_SIZE);
    m->fifo_count++;
    tca8418_update_status(m);
}
