/*
 * Simulator BLE — fake state machine for Bluetooth settings testing.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "thistle/ble_manager.h"
#include "esp_err.h"
#include <stdio.h>
#include <string.h>

static ble_state_t s_state = BLE_STATE_OFF;
static ble_rx_cb_t s_rx_cb = NULL;
static void *s_rx_cb_data = NULL;
static int s_adv_count = 0; /* track advertising calls to auto-connect */

esp_err_t ble_manager_init(const char *device_name)
{
    printf("[sim_ble] BLE initialized: '%s'\n", device_name ? device_name : "ThistleOS");
    return ESP_OK;
}

esp_err_t ble_manager_start_advertising(void)
{
    s_state = BLE_STATE_ADVERTISING;
    s_adv_count++;
    printf("[sim_ble] Advertising started (call #%d)\n", s_adv_count);

    /* Simulate auto-connection after 2nd advertising call */
    if (s_adv_count >= 2) {
        s_state = BLE_STATE_CONNECTED;
        printf("[sim_ble] Simulated device connected: iPhone (Sim)\n");
    }

    return ESP_OK;
}

esp_err_t ble_manager_stop_advertising(void)
{
    s_state = BLE_STATE_OFF;
    s_adv_count = 0;
    printf("[sim_ble] Advertising stopped\n");
    return ESP_OK;
}

esp_err_t ble_manager_disconnect(void)
{
    if (s_state == BLE_STATE_CONNECTED) {
        s_state = BLE_STATE_ADVERTISING;
        printf("[sim_ble] Disconnected, resuming advertising\n");
    }
    return ESP_OK;
}

esp_err_t ble_manager_send(const uint8_t *data, size_t len)
{
    if (s_state != BLE_STATE_CONNECTED) return ESP_ERR_INVALID_STATE;
    printf("[sim_ble] TX %zu bytes\n", len);
    return ESP_OK;
}

esp_err_t ble_manager_send_notification(const char *title, const char *body)
{
    if (s_state != BLE_STATE_CONNECTED) return ESP_ERR_INVALID_STATE;
    printf("[sim_ble] Notification: %s — %s\n", title ? title : "", body ? body : "");
    return ESP_OK;
}

esp_err_t ble_manager_register_rx_cb(ble_rx_cb_t cb, void *user_data)
{
    s_rx_cb = cb;
    s_rx_cb_data = user_data;
    return ESP_OK;
}

ble_state_t ble_manager_get_state(void)
{
    return s_state;
}

const char *ble_manager_get_peer_name(void)
{
    if (s_state == BLE_STATE_CONNECTED) return "iPhone (Sim)";
    return NULL;
}
