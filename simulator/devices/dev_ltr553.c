/*
 * LTR-553 ambient light / proximity sensor — virtual I2C device model.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include <string.h>
#include <stdio.h>

#define LTR553_NUM_REGS 256

/* Register addresses */
#define REG_ALS_CONTR   0x80  /* ALS control */
#define REG_PS_CONTR    0x81  /* PS control */
#define REG_PS_LED      0x82  /* PS LED config */
#define REG_PS_N_PULSES 0x83
#define REG_PS_MEAS     0x84
#define REG_ALS_MEAS    0x85
#define REG_PART_ID     0x86  /* Part ID: 0x92 */
#define REG_MANUFAC_ID  0x87  /* Manufacturer ID: 0x05 */
#define REG_ALS_DATA_CH1_L 0x88  /* ALS CH1 data low */
#define REG_ALS_DATA_CH1_H 0x89  /* ALS CH1 data high */
#define REG_ALS_DATA_CH0_L 0x8A  /* ALS CH0 data low */
#define REG_ALS_DATA_CH0_H 0x8B  /* ALS CH0 data high */
#define REG_ALS_STATUS  0x8C
#define REG_PS_DATA_L   0x8D  /* PS data low */
#define REG_PS_DATA_H   0x8E  /* PS data high (bits 2:0 only) */

typedef struct {
    uint8_t  regs[LTR553_NUM_REGS];
    uint16_t lux;       /* Configurable ambient light value */
    uint16_t proximity; /* Configurable proximity value */
} ltr553_model_t;

static ltr553_model_t ltr553_model;

static void ltr553_update_data_regs(ltr553_model_t *m)
{
    /*
     * ALS: split lux across CH0 (primary) and CH1 (IR).
     * Simple model: CH0 = lux, CH1 = lux / 2.
     */
    uint16_t ch0 = m->lux;
    uint16_t ch1 = m->lux / 2;
    m->regs[REG_ALS_DATA_CH0_L] = (uint8_t)(ch0 & 0xFF);
    m->regs[REG_ALS_DATA_CH0_H] = (uint8_t)((ch0 >> 8) & 0xFF);
    m->regs[REG_ALS_DATA_CH1_L] = (uint8_t)(ch1 & 0xFF);
    m->regs[REG_ALS_DATA_CH1_H] = (uint8_t)((ch1 >> 8) & 0xFF);

    /* ALS status: data valid (bit 2 = 0), new data (bit 3 = 1) */
    m->regs[REG_ALS_STATUS] = 0x08;

    /* PS data: 11-bit value */
    m->regs[REG_PS_DATA_L] = (uint8_t)(m->proximity & 0xFF);
    m->regs[REG_PS_DATA_H] = (uint8_t)((m->proximity >> 8) & 0x07);
}

static esp_err_t ltr553_on_read(sim_i2c_device_t *dev,
                                 const uint8_t *tx, size_t tx_len,
                                 uint8_t *rx, size_t rx_len)
{
    ltr553_model_t *m = (ltr553_model_t *)dev->model;
    if (tx_len < 1) { memset(rx, 0, rx_len); return ESP_OK; }
    uint8_t reg = tx[0];

    /* Refresh data registers if reading data area */
    if (reg >= REG_ALS_DATA_CH1_L && reg <= REG_PS_DATA_H) {
        ltr553_update_data_regs(m);
    }

    /* Auto-increment read */
    for (size_t i = 0; i < rx_len; i++) {
        rx[i] = m->regs[(uint8_t)(reg + i)];
    }
    return ESP_OK;
}

static esp_err_t ltr553_on_write(sim_i2c_device_t *dev,
                                  const uint8_t *buf, size_t len)
{
    ltr553_model_t *m = (ltr553_model_t *)dev->model;
    if (len < 2) return ESP_OK;
    uint8_t reg = buf[0];

    for (size_t i = 1; i < len; i++) {
        uint8_t r = (uint8_t)(reg + (i - 1));
        /* Skip read-only registers */
        if (r == REG_PART_ID || r == REG_MANUFAC_ID) continue;
        if (r >= REG_ALS_DATA_CH1_L && r <= REG_PS_DATA_H) continue;
        m->regs[r] = buf[i];
    }
    return ESP_OK;
}

static const sim_i2c_device_ops_t ltr553_ops = {
    .on_read  = ltr553_on_read,
    .on_write = ltr553_on_write,
};

void dev_ltr553_register(int bus_index, uint16_t addr)
{
    memset(&ltr553_model, 0, sizeof(ltr553_model));

    /* Fixed identification */
    ltr553_model.regs[REG_PART_ID]    = 0x92;
    ltr553_model.regs[REG_MANUFAC_ID] = 0x05;

    /* Default values */
    ltr553_model.lux       = 400;  /* Indoor ambient */
    ltr553_model.proximity = 0;    /* Nothing nearby */
    ltr553_update_data_regs(&ltr553_model);

    sim_i2c_bus_add_model(bus_index, addr, &ltr553_ops, &ltr553_model);
    printf("[sim] LTR-553 light/prox registered on I2C bus %d addr 0x%02X\n",
           bus_index, addr);
}

void dev_ltr553_set_lux(uint16_t lux)
{
    ltr553_model.lux = lux;
}

void dev_ltr553_set_proximity(uint16_t prox)
{
    ltr553_model.proximity = prox;
}
