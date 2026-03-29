/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026 ThistleOS Contributors
 */
#include "sim_radio.h"
#include <stdio.h>
#include <string.h>
#include <stddef.h>

#define SIM_RADIO_MAX_PACKET 255

static uint8_t s_loopback_buf[SIM_RADIO_MAX_PACKET];
static hal_radio_rx_cb_t s_rx_cb;
static void *s_rx_user_data;
static uint32_t s_freq_hz;
static int8_t s_tx_power;
static uint32_t s_bandwidth;
static uint8_t s_sf;

static esp_err_t sim_radio_init(const void *config)
{
    (void)config;
    s_rx_cb = NULL;
    s_rx_user_data = NULL;
    s_freq_hz = 0;
    s_tx_power = 0;
    s_bandwidth = 0;
    s_sf = 0;
    printf("[sim_radio] init\n");
    return ESP_OK;
}

static void sim_radio_deinit(void)
{
    s_rx_cb = NULL;
    s_rx_user_data = NULL;
    printf("[sim_radio] deinit\n");
}

static esp_err_t sim_radio_set_frequency(uint32_t freq_hz)
{
    s_freq_hz = freq_hz;
    printf("[sim_radio] set_frequency %u Hz\n", (unsigned)freq_hz);
    return ESP_OK;
}

static esp_err_t sim_radio_set_tx_power(int8_t dbm)
{
    s_tx_power = dbm;
    printf("[sim_radio] set_tx_power %d dBm\n", (int)dbm);
    return ESP_OK;
}

static esp_err_t sim_radio_set_bandwidth(uint32_t bw_hz)
{
    s_bandwidth = bw_hz;
    printf("[sim_radio] set_bandwidth %u Hz\n", (unsigned)bw_hz);
    return ESP_OK;
}

static esp_err_t sim_radio_set_spreading_factor(uint8_t sf)
{
    s_sf = sf;
    printf("[sim_radio] set_spreading_factor %u\n", (unsigned)sf);
    return ESP_OK;
}

static esp_err_t sim_radio_send(const uint8_t *data, size_t len)
{
    if (len > SIM_RADIO_MAX_PACKET) {
        printf("[sim_radio] send rejected: %zu bytes exceeds max %d\n",
               len, SIM_RADIO_MAX_PACKET);
        return ESP_ERR_INVALID_SIZE;
    }

    printf("[sim_radio] send %zu bytes\n", len);
    memcpy(s_loopback_buf, data, len);

    if (s_rx_cb) {
        printf("[sim_radio] loopback -> rx callback (%zu bytes, rssi=-30)\n", len);
        s_rx_cb(s_loopback_buf, len, -30, s_rx_user_data);
    }

    return ESP_OK;
}

static esp_err_t sim_radio_start_receive(hal_radio_rx_cb_t cb, void *user_data)
{
    s_rx_cb = cb;
    s_rx_user_data = user_data;
    printf("[sim_radio] start_receive\n");
    return ESP_OK;
}

static esp_err_t sim_radio_stop_receive(void)
{
    s_rx_cb = NULL;
    s_rx_user_data = NULL;
    printf("[sim_radio] stop_receive\n");
    return ESP_OK;
}

static int sim_radio_get_rssi(void)
{
    return -60;
}

static esp_err_t sim_radio_sleep(bool enter)
{
    printf("[sim_radio] sleep %s\n", enter ? "enter" : "exit");
    return ESP_OK;
}

static const hal_radio_driver_t sim_radio_driver = {
    .init               = sim_radio_init,
    .deinit             = sim_radio_deinit,
    .set_frequency      = sim_radio_set_frequency,
    .set_tx_power       = sim_radio_set_tx_power,
    .set_bandwidth      = sim_radio_set_bandwidth,
    .set_spreading_factor = sim_radio_set_spreading_factor,
    .send               = sim_radio_send,
    .start_receive      = sim_radio_start_receive,
    .stop_receive       = sim_radio_stop_receive,
    .get_rssi           = sim_radio_get_rssi,
    .sleep              = sim_radio_sleep,
    .name               = "Simulator Radio (loopback)",
};

const hal_radio_driver_t *sim_radio_get(void)
{
    return &sim_radio_driver;
}
