#pragma once
#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    char ssid[33];
    int8_t rssi;
    bool is_open;
} recovery_wifi_scan_result_t;

esp_err_t recovery_wifi_init(void);
esp_err_t recovery_wifi_scan(recovery_wifi_scan_result_t *results, int max, int *count);
esp_err_t recovery_wifi_connect(const char *ssid, const char *password, uint32_t timeout_ms);
bool recovery_wifi_is_connected(void);
