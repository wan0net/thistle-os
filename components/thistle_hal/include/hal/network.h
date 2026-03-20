/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Network HAL interface
 *
 * Defines the vtable that any network transport (WiFi, 4G/LTE, BLE tether,
 * simulator host network) must implement to plug into the net_manager.
 */
#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/* Network transport types */
typedef enum {
    HAL_NET_WIFI,
    HAL_NET_CELLULAR,   /* 4G/LTE via PPP */
    HAL_NET_BLE,        /* BLE tethering */
    HAL_NET_ETHERNET,   /* Future */
    HAL_NET_HOST,       /* Simulator — uses host OS networking */
} hal_net_transport_t;

/* Network state */
typedef enum {
    HAL_NET_STATE_DISCONNECTED,
    HAL_NET_STATE_CONNECTING,
    HAL_NET_STATE_CONNECTED,
} hal_net_state_t;

/* Network driver vtable */
typedef struct {
    hal_net_transport_t type;
    const char *name;                              /* "WiFi", "4G LTE", "Host" */
    esp_err_t (*init)(void);
    esp_err_t (*connect)(const char *target, const char *credential, uint32_t timeout_ms);
    esp_err_t (*disconnect)(void);
    hal_net_state_t (*get_state)(void);
    const char *(*get_ip)(void);
    int8_t (*get_rssi)(void);                      /* signal strength, 0 if N/A */
    bool (*is_connected)(void);
} hal_net_driver_t;
