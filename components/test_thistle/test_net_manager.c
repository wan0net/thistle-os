/*
 * test_net_manager.c — Unit tests for the ThistleOS network manager
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls net_manager_init() to reset transport registration state.
 * These tests exercise only the net_manager abstraction layer via mock
 * transports; they do not touch WiFi hardware or the IDF WiFi stack.
 */

#include "unity.h"
#include "thistle/net_manager.h"
#include "hal/network.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Mock transport: always disconnected
 * -------------------------------------------------------------------------- */

static esp_err_t mock_disconnected_init(void) { return ESP_OK; }
static esp_err_t mock_disconnected_connect(const char *t, const char *c, uint32_t ms) { return ESP_ERR_TIMEOUT; }
static esp_err_t mock_disconnected_disconnect(void) { return ESP_OK; }
static hal_net_state_t mock_disconnected_get_state(void) { return HAL_NET_STATE_DISCONNECTED; }
static const char *mock_disconnected_get_ip(void) { return NULL; }
static int8_t mock_disconnected_get_rssi(void) { return 0; }
static bool mock_disconnected_is_connected(void) { return false; }

static const hal_net_driver_t s_mock_disconnected = {
    .type         = HAL_NET_WIFI,
    .name         = "MockWiFi",
    .init         = mock_disconnected_init,
    .connect      = mock_disconnected_connect,
    .disconnect   = mock_disconnected_disconnect,
    .get_state    = mock_disconnected_get_state,
    .get_ip       = mock_disconnected_get_ip,
    .get_rssi     = mock_disconnected_get_rssi,
    .is_connected = mock_disconnected_is_connected,
};

/* --------------------------------------------------------------------------
 * Mock transport: always connected, returns "10.0.0.1"
 * -------------------------------------------------------------------------- */

static esp_err_t mock_connected_init(void) { return ESP_OK; }
static esp_err_t mock_connected_connect(const char *t, const char *c, uint32_t ms) { return ESP_OK; }
static esp_err_t mock_connected_disconnect(void) { return ESP_OK; }
static hal_net_state_t mock_connected_get_state(void) { return HAL_NET_STATE_CONNECTED; }
static const char *mock_connected_get_ip(void) { return "10.0.0.1"; }
static int8_t mock_connected_get_rssi(void) { return -55; }
static bool mock_connected_is_connected(void) { return true; }

static const hal_net_driver_t s_mock_connected = {
    .type         = HAL_NET_HOST,
    .name         = "MockConnected",
    .init         = mock_connected_init,
    .connect      = mock_connected_connect,
    .disconnect   = mock_connected_disconnect,
    .get_state    = mock_connected_get_state,
    .get_ip       = mock_connected_get_ip,
    .get_rssi     = mock_connected_get_rssi,
    .is_connected = mock_connected_is_connected,
};

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_net_init: net_manager_init returns ESP_OK", "[net]")
{
    esp_err_t ret = net_manager_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_net_not_connected_initially: net_is_connected false before any transport", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_FALSE(net_is_connected());
}

TEST_CASE("test_net_register_transport: registered transport appears in net_list_transports", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_disconnected));

    const hal_net_driver_t *out[4];
    int count = net_list_transports(out, 4);
    TEST_ASSERT_GREATER_OR_EQUAL(1, count);

    /* The last registered transport must be findable in the list */
    bool found = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(out[i]->name, "MockWiFi") == 0) {
            found = true;
            break;
        }
    }
    TEST_ASSERT_TRUE_MESSAGE(found, "Registered transport not found in list");
}

TEST_CASE("test_net_mock_connected: connected mock makes net_is_connected return true", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));
    TEST_ASSERT_TRUE(net_is_connected());
}

TEST_CASE("test_net_get_ip_from_mock: net_get_ip returns mock transport's IP", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));

    const char *ip = net_get_ip();
    TEST_ASSERT_NOT_NULL(ip);
    TEST_ASSERT_EQUAL_STRING("10.0.0.1", ip);
}

TEST_CASE("test_net_get_transport_name: returns connected mock's name", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));

    const char *name = net_get_transport_name();
    TEST_ASSERT_NOT_NULL(name);
    TEST_ASSERT_EQUAL_STRING("MockConnected", name);
}

TEST_CASE("test_net_multiple_transports: disconnected then connected — net picks connected one", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_disconnected));
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));

    /* Overall connection state must be true (one of the two is connected) */
    TEST_ASSERT_TRUE(net_is_connected());

    /* The active transport must be the connected one */
    const hal_net_driver_t *active = net_get_active();
    TEST_ASSERT_NOT_NULL(active);
    TEST_ASSERT_EQUAL_STRING("MockConnected", active->name);
}

TEST_CASE("test_net_connect_best: mock with connect=ESP_OK succeeds", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));

    /* Already connected — net_connect_best must return ESP_OK immediately */
    esp_err_t ret = net_connect_best(100);
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_net_get_state_connected: state is CONNECTED after registering connected mock", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_connected));

    hal_net_state_t state = net_get_state();
    TEST_ASSERT_EQUAL(HAL_NET_STATE_CONNECTED, state);
}

TEST_CASE("test_net_get_state_disconnected: state is DISCONNECTED with no transports", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());

    hal_net_state_t state = net_get_state();
    TEST_ASSERT_EQUAL(HAL_NET_STATE_DISCONNECTED, state);
}

TEST_CASE("test_net_get_active_null_when_none: net_get_active returns NULL before any transport", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    const hal_net_driver_t *active = net_get_active();
    TEST_ASSERT_NULL(active);
}

TEST_CASE("test_net_get_ip_null_when_disconnected: net_get_ip returns NULL with no connected transport", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_register(&s_mock_disconnected));

    const char *ip = net_get_ip();
    TEST_ASSERT_NULL(ip);
}

TEST_CASE("test_net_list_transports_empty: returns 0 with no transports registered", "[net]")
{
    TEST_ASSERT_EQUAL(ESP_OK, net_manager_init());

    const hal_net_driver_t *out[4];
    int count = net_list_transports(out, 4);
    TEST_ASSERT_EQUAL_INT(0, count);
}
