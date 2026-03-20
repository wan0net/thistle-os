#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

typedef void (*hal_radio_rx_cb_t)(const uint8_t *data, size_t len, int rssi, void *user_data);

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*set_frequency)(uint32_t freq_hz);
    esp_err_t (*set_tx_power)(int8_t dbm);
    esp_err_t (*set_bandwidth)(uint32_t bw_hz);
    esp_err_t (*set_spreading_factor)(uint8_t sf);
    esp_err_t (*send)(const uint8_t *data, size_t len);
    esp_err_t (*start_receive)(hal_radio_rx_cb_t cb, void *user_data);
    esp_err_t (*stop_receive)(void);
    int (*get_rssi)(void);
    esp_err_t (*sleep)(bool enter);
    const char *name;
} hal_radio_driver_t;
