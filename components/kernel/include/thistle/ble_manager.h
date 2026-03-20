#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

#define BLE_DEVICE_NAME_MAX 32

typedef enum {
    BLE_STATE_OFF,
    BLE_STATE_ADVERTISING,
    BLE_STATE_CONNECTED,
} ble_state_t;

/* Callback for data received from companion app */
typedef void (*ble_rx_cb_t)(const uint8_t *data, size_t len, void *user_data);

/* Initialize BLE subsystem */
esp_err_t ble_manager_init(const char *device_name);

/* Start advertising (discoverable) */
esp_err_t ble_manager_start_advertising(void);

/* Stop advertising */
esp_err_t ble_manager_stop_advertising(void);

/* Disconnect current connection */
esp_err_t ble_manager_disconnect(void);

/* Send data to connected companion app (via notify characteristic) */
esp_err_t ble_manager_send(const uint8_t *data, size_t len);

/* Send a text notification to companion app */
esp_err_t ble_manager_send_notification(const char *title, const char *body);

/* Register callback for incoming data */
esp_err_t ble_manager_register_rx_cb(ble_rx_cb_t cb, void *user_data);

/* Get current BLE state */
ble_state_t ble_manager_get_state(void);

/* Get connected device name (NULL if not connected) */
const char *ble_manager_get_peer_name(void);
