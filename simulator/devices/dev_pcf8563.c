/*
 * PCF8563 RTC — virtual I2C device model.
 * 16 registers (0x00-0x0F). Time registers auto-populated from host clock.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <sys/time.h>

#define PCF8563_NUM_REGS 16

typedef struct {
    uint8_t regs[PCF8563_NUM_REGS];
    /* Offset in seconds between host clock and RTC time.
     * When the driver writes time registers, we compute this so subsequent
     * reads return the written time advancing in real-time. */
    int64_t offset_secs;
} pcf8563_model_t;

static uint8_t bin_to_bcd(uint8_t v) { return ((v / 10) << 4) | (v % 10); }

static void pcf8563_update_time(pcf8563_model_t *m)
{
    struct timeval tv;
    gettimeofday(&tv, NULL);
    time_t t = tv.tv_sec + m->offset_secs;
    struct tm tm;
    gmtime_r(&t, &tm);

    /* Reg 0x02: seconds (BCD) + VL bit 7 (0 = time valid) */
    m->regs[0x02] = bin_to_bcd((uint8_t)(tm.tm_sec % 60));
    /* Reg 0x03: minutes (BCD) */
    m->regs[0x03] = bin_to_bcd((uint8_t)tm.tm_min);
    /* Reg 0x04: hours (BCD, 24-hour) */
    m->regs[0x04] = bin_to_bcd((uint8_t)tm.tm_hour);
    /* Reg 0x05: days (BCD) */
    m->regs[0x05] = bin_to_bcd((uint8_t)tm.tm_mday);
    /* Reg 0x06: weekdays (0-6) */
    m->regs[0x06] = (uint8_t)tm.tm_wday;
    /* Reg 0x07: months (BCD) + century bit 7 (set for years >= 2000) */
    m->regs[0x07] = bin_to_bcd((uint8_t)(tm.tm_mon + 1));
    if (tm.tm_year >= 100) {
        m->regs[0x07] |= 0x80; /* Century bit */
    }
    /* Reg 0x08: years (BCD, 0-99) */
    m->regs[0x08] = bin_to_bcd((uint8_t)(tm.tm_year % 100));
}

static esp_err_t pcf8563_on_read(sim_i2c_device_t *dev,
                                  const uint8_t *tx, size_t tx_len,
                                  uint8_t *rx, size_t rx_len)
{
    pcf8563_model_t *m = (pcf8563_model_t *)dev->model;
    if (tx_len < 1) { memset(rx, 0, rx_len); return ESP_OK; }
    uint8_t reg = tx[0];

    /* If any of the requested registers overlap with time regs, refresh */
    if (reg <= 0x08 && (reg + rx_len) > 0x02) {
        /* Only update if oscillator is running (STOP bit = 0) */
        if (!(m->regs[0x00] & 0x20)) {
            pcf8563_update_time(m);
        }
    }

    /* Auto-increment read across registers */
    for (size_t i = 0; i < rx_len; i++) {
        uint8_t r = (uint8_t)((reg + i) % PCF8563_NUM_REGS);
        rx[i] = m->regs[r];
    }
    return ESP_OK;
}

static uint8_t bcd_to_bin(uint8_t bcd) { return (bcd >> 4) * 10 + (bcd & 0x0F); }

static esp_err_t pcf8563_on_write(sim_i2c_device_t *dev,
                                   const uint8_t *buf, size_t len)
{
    pcf8563_model_t *m = (pcf8563_model_t *)dev->model;
    if (len < 2) return ESP_OK;
    uint8_t reg = buf[0];

    /* Auto-increment write across registers */
    for (size_t i = 1; i < len; i++) {
        uint8_t r = (uint8_t)((reg + (i - 1)) % PCF8563_NUM_REGS);
        m->regs[r] = buf[i];
    }

    /* If time registers were written, compute offset from host clock */
    if (reg <= 0x08 && (reg + (len - 1)) > 0x02) {
        struct tm tm;
        memset(&tm, 0, sizeof(tm));
        tm.tm_sec  = bcd_to_bin(m->regs[0x02] & 0x7F);
        tm.tm_min  = bcd_to_bin(m->regs[0x03] & 0x7F);
        tm.tm_hour = bcd_to_bin(m->regs[0x04] & 0x3F);
        tm.tm_mday = bcd_to_bin(m->regs[0x05] & 0x3F);
        tm.tm_mon  = bcd_to_bin(m->regs[0x07] & 0x1F) - 1;
        int year   = bcd_to_bin(m->regs[0x08]);
        if (m->regs[0x07] & 0x80) year += 100; /* Century bit */
        tm.tm_year = year;

        time_t written_time = timegm(&tm);
        struct timeval tv;
        gettimeofday(&tv, NULL);
        m->offset_secs = (int64_t)written_time - (int64_t)tv.tv_sec;
    }

    return ESP_OK;
}

static const sim_i2c_device_ops_t pcf8563_ops = {
    .on_read  = pcf8563_on_read,
    .on_write = pcf8563_on_write,
};

static pcf8563_model_t pcf8563_model;

void dev_pcf8563_register(int bus_index, uint16_t addr)
{
    memset(&pcf8563_model, 0, sizeof(pcf8563_model));
    pcf8563_model.offset_secs = 0;
    /* Initialize with current host time */
    pcf8563_update_time(&pcf8563_model);

    sim_i2c_bus_add_model(bus_index, addr, &pcf8563_ops, &pcf8563_model);
    printf("[sim] PCF8563 RTC registered on I2C bus %d addr 0x%02X\n",
           bus_index, addr);
}
