/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS Simulator — Host Network transport
 *
 * Provides a hal_net_driver_t that is always "connected" via the host
 * OS network stack.  Register this in board_simulator.c so that apps
 * see a working net_is_connected() without requiring WiFi credentials
 * in the simulator.
 */

#include "hal/network.h"
#include "thistle/net_manager.h"
#include "esp_err.h"
#include <stdio.h>

/* ------------------------------------------------------------------ */
/* Driver functions — all trivial for host networking                   */
/* ------------------------------------------------------------------ */

static esp_err_t sim_net_init(void)
{
    printf("[sim_net] Host Network transport initialized\n");
    return ESP_OK;
}

static esp_err_t sim_net_connect(const char *target, const char *credential,
                                  uint32_t timeout_ms)
{
    (void)target;
    (void)credential;
    (void)timeout_ms;
    /* Host is always connected — nothing to do */
    return ESP_OK;
}

static esp_err_t sim_net_disconnect(void)
{
    /* No-op in simulator */
    return ESP_OK;
}

static hal_net_state_t sim_net_get_state(void)
{
    return HAL_NET_STATE_CONNECTED;
}

static const char *sim_net_get_ip(void)
{
    return "127.0.0.1";
}

static int8_t sim_net_get_rssi(void)
{
    return -30; /* excellent signal */
}

static bool sim_net_is_connected(void)
{
    return true;
}

/* ------------------------------------------------------------------ */
/* Driver descriptor                                                    */
/* ------------------------------------------------------------------ */

static const hal_net_driver_t s_sim_net_driver = {
    .type         = HAL_NET_HOST,
    .name         = "Host Network",
    .init         = sim_net_init,
    .connect      = sim_net_connect,
    .disconnect   = sim_net_disconnect,
    .get_state    = sim_net_get_state,
    .get_ip       = sim_net_get_ip,
    .get_rssi     = sim_net_get_rssi,
    .is_connected = sim_net_is_connected,
};

esp_err_t sim_network_register(void)
{
    return net_manager_register(&s_sim_net_driver);
}
