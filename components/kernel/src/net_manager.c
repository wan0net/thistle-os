/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Network manager implementation
 *
 * Transport-agnostic internet connectivity layer.
 * Any transport (WiFi, 4G, BLE tether, sim host) registers a
 * hal_net_driver_t vtable; apps call net_is_connected() and friends.
 *
 * The built-in WiFi transport wrapper is included here so that
 * wifi_manager.c needs no modification.
 */

#include "thistle/net_manager.h"
#include "thistle/wifi_manager.h"
#include "esp_log.h"
#include <string.h>

static const char *TAG = "net_mgr";

#define MAX_NET_TRANSPORTS 4

static const hal_net_driver_t *s_transports[MAX_NET_TRANSPORTS];
static int  s_transport_count = 0;
static bool s_initialized     = false;

/* ------------------------------------------------------------------ */
/* Core manager                                                         */
/* ------------------------------------------------------------------ */

esp_err_t net_manager_init(void)
{
    memset(s_transports, 0, sizeof(s_transports));
    s_transport_count = 0;
    s_initialized     = true;
    ESP_LOGI(TAG, "Network manager initialized");
    return ESP_OK;
}

esp_err_t net_manager_register(const hal_net_driver_t *driver)
{
    if (!driver || !s_initialized) return ESP_ERR_INVALID_STATE;
    if (s_transport_count >= MAX_NET_TRANSPORTS) return ESP_ERR_NO_MEM;
    s_transports[s_transport_count++] = driver;
    ESP_LOGI(TAG, "Registered transport: %s (type %d)", driver->name, (int)driver->type);
    return ESP_OK;
}

bool net_is_connected(void)
{
    for (int i = 0; i < s_transport_count; i++) {
        if (s_transports[i]->is_connected && s_transports[i]->is_connected()) {
            return true;
        }
    }
    return false;
}

const hal_net_driver_t *net_get_active(void)
{
    for (int i = 0; i < s_transport_count; i++) {
        if (s_transports[i]->is_connected && s_transports[i]->is_connected()) {
            return s_transports[i];
        }
    }
    return NULL;
}

hal_net_state_t net_get_state(void)
{
    hal_net_state_t best = HAL_NET_STATE_DISCONNECTED;
    for (int i = 0; i < s_transport_count; i++) {
        if (!s_transports[i]->get_state) continue;
        hal_net_state_t st = s_transports[i]->get_state();
        if (st == HAL_NET_STATE_CONNECTED)  return HAL_NET_STATE_CONNECTED;
        if (st == HAL_NET_STATE_CONNECTING) best = HAL_NET_STATE_CONNECTING;
    }
    return best;
}

const char *net_get_ip(void)
{
    const hal_net_driver_t *active = net_get_active();
    if (active && active->get_ip) return active->get_ip();
    return NULL;
}

int8_t net_get_rssi(void)
{
    const hal_net_driver_t *active = net_get_active();
    if (active && active->get_rssi) return active->get_rssi();
    return 0;
}

const char *net_get_transport_name(void)
{
    const hal_net_driver_t *active = net_get_active();
    if (active) return active->name;
    return "None";
}

esp_err_t net_connect_best(uint32_t timeout_ms)
{
    for (int i = 0; i < s_transport_count; i++) {
        if (s_transports[i]->is_connected && s_transports[i]->is_connected()) {
            return ESP_OK; /* already connected */
        }
        if (s_transports[i]->connect) {
            esp_err_t ret = s_transports[i]->connect(NULL, NULL, timeout_ms);
            if (ret == ESP_OK) {
                ESP_LOGI(TAG, "Connected via %s", s_transports[i]->name);
                return ESP_OK;
            }
        }
    }
    return ESP_FAIL;
}

esp_err_t net_ntp_sync(void)
{
    if (!net_is_connected()) return ESP_ERR_INVALID_STATE;
    /* wifi_manager_ntp_sync works over any IP transport */
    return wifi_manager_ntp_sync();
}

int net_list_transports(const hal_net_driver_t **out, int max)
{
    int count = 0;
    for (int i = 0; i < s_transport_count && count < max; i++) {
        out[count++] = s_transports[i];
    }
    return count;
}

/* ------------------------------------------------------------------ */
/* Built-in WiFi transport wrapper                                      */
/*                                                                      */
/* Wraps wifi_manager without modifying it.  Auto-registered by         */
/* net_manager_register_wifi() called from kernel_init.                 */
/* ------------------------------------------------------------------ */

static hal_net_state_t wifi_net_get_state(void)
{
    wifi_state_t ws = wifi_manager_get_state();
    if (ws == WIFI_STATE_CONNECTED)  return HAL_NET_STATE_CONNECTED;
    if (ws == WIFI_STATE_CONNECTING) return HAL_NET_STATE_CONNECTING;
    return HAL_NET_STATE_DISCONNECTED;
}

static bool wifi_net_is_connected(void)
{
    return wifi_manager_get_state() == WIFI_STATE_CONNECTED;
}

static const hal_net_driver_t s_wifi_net_driver = {
    .type         = HAL_NET_WIFI,
    .name         = "WiFi",
    .init         = wifi_manager_init,
    .connect      = wifi_manager_connect,
    .disconnect   = wifi_manager_disconnect,
    .get_state    = wifi_net_get_state,
    .get_ip       = wifi_manager_get_ip,
    .get_rssi     = wifi_manager_get_rssi,
    .is_connected = wifi_net_is_connected,
};

esp_err_t net_manager_register_wifi(void)
{
    return net_manager_register(&s_wifi_net_driver);
}
