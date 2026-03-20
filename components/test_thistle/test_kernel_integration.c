/*
 * test_kernel_integration.c — Integration tests: event bus + app manager + IPC
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * These tests verify that multiple kernel components interact correctly.
 * Each test calls the relevant init functions to reset all subsystem state
 * before exercising cross-component behaviour.
 */

#include "unity.h"
#include "thistle/event.h"
#include "thistle/app_manager.h"
#include "thistle/ipc.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Mock app state — shared across all tests in this file
 * -------------------------------------------------------------------------- */

static volatile int s_on_create_calls  = 0;
static volatile int s_on_start_calls   = 0;
static volatile int s_on_pause_calls   = 0;
static volatile int s_on_resume_calls  = 0;
static volatile int s_on_destroy_calls = 0;

static void reset_app_mock_state(void)
{
    s_on_create_calls  = 0;
    s_on_start_calls   = 0;
    s_on_pause_calls   = 0;
    s_on_resume_calls  = 0;
    s_on_destroy_calls = 0;
}

static esp_err_t mock_on_create(void)  { s_on_create_calls++;  return ESP_OK; }
static void      mock_on_start(void)   { s_on_start_calls++;            }
static void      mock_on_pause(void)   { s_on_pause_calls++;            }
static void      mock_on_resume(void)  { s_on_resume_calls++;           }
static void      mock_on_destroy(void) { s_on_destroy_calls++;          }

static const app_manifest_t s_manifest_integ_a = {
    .id               = "com.integ.app_a",
    .name             = "Integ App A",
    .version          = "1.0.0",
    .allow_background = true,
    .min_memory_kb    = 0,
};

static const app_entry_t s_integ_app_a = {
    .on_create  = mock_on_create,
    .on_start   = mock_on_start,
    .on_pause   = mock_on_pause,
    .on_resume  = mock_on_resume,
    .on_destroy = mock_on_destroy,
    .manifest   = &s_manifest_integ_a,
};

static const app_manifest_t s_manifest_integ_b = {
    .id               = "com.integ.app_b",
    .name             = "Integ App B",
    .version          = "1.0.0",
    .allow_background = true,
    .min_memory_kb    = 0,
};

static const app_entry_t __attribute__((unused)) s_integ_app_b = {
    .on_create  = mock_on_create,
    .on_start   = mock_on_start,
    .on_pause   = mock_on_pause,
    .on_resume  = mock_on_resume,
    .on_destroy = mock_on_destroy,
    .manifest   = &s_manifest_integ_b,
};

/* --------------------------------------------------------------------------
 * Event tracking state
 * -------------------------------------------------------------------------- */

static volatile int s_launched_event_count = 0;

static void on_app_launched(const event_t *ev, void *user_data)
{
    s_launched_event_count++;
}

/* --------------------------------------------------------------------------
 * IPC tracking state for inter-app test
 * -------------------------------------------------------------------------- */

static volatile int    s_ipc_recv_calls = 0;
static volatile uint32_t s_ipc_recv_type = 0;
static uint8_t           s_ipc_recv_data[IPC_MSG_MAX_DATA];
static size_t            s_ipc_recv_data_len = 0;

static void ipc_app_b_handler(const ipc_message_t *msg, void *user_data)
{
    s_ipc_recv_calls++;
    s_ipc_recv_type     = msg->msg_type;
    s_ipc_recv_data_len = msg->data_len;
    if (msg->data_len > 0 && msg->data_len <= IPC_MSG_MAX_DATA) {
        memcpy(s_ipc_recv_data, msg->data, msg->data_len);
    }
}

/* --------------------------------------------------------------------------
 * Helper
 * -------------------------------------------------------------------------- */

static void full_setup(void)
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_init());
    TEST_ASSERT_EQUAL(ESP_OK, ipc_init());
    reset_app_mock_state();
    s_launched_event_count = 0;
    s_ipc_recv_calls       = 0;
    s_ipc_recv_type        = 0;
    s_ipc_recv_data_len    = 0;
    memset(s_ipc_recv_data, 0, sizeof(s_ipc_recv_data));
}

/* --------------------------------------------------------------------------
 * Integration tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_event_triggers_on_app_launch: EVENT_APP_LAUNCHED fires when app is launched", "[integration]")
{
    full_setup();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_APP_LAUNCHED, on_app_launched, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_integ_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.integ.app_a"));

    TEST_ASSERT_EQUAL_INT(1, s_launched_event_count);
}

TEST_CASE("test_ipc_between_mock_apps: IPC message sent by app A received by handler for app B", "[integration]")
{
    full_setup();

    /* Register a handler that simulates app B receiving messages of type 55 */
    TEST_ASSERT_EQUAL(ESP_OK, ipc_register_handler(55, ipc_app_b_handler, NULL));

    /* Simulate app A sending a message to app B */
    ipc_message_t tx;
    memset(&tx, 0, sizeof(tx));
    tx.src_app  = 1;   /* app A */
    tx.dst_app  = 2;   /* app B */
    tx.msg_type = 55;
    tx.data_len = 5;
    tx.data[0] = 'h';
    tx.data[1] = 'e';
    tx.data[2] = 'l';
    tx.data[3] = 'l';
    tx.data[4] = 'o';

    TEST_ASSERT_EQUAL(ESP_OK, ipc_send(&tx));

    /* Handler must have been invoked synchronously */
    TEST_ASSERT_EQUAL_INT(1, s_ipc_recv_calls);
    TEST_ASSERT_EQUAL_UINT32(55, s_ipc_recv_type);
    TEST_ASSERT_EQUAL_size_t(5, s_ipc_recv_data_len);
    TEST_ASSERT_EQUAL_UINT8('h', s_ipc_recv_data[0]);
    TEST_ASSERT_EQUAL_UINT8('o', s_ipc_recv_data[4]);
}

TEST_CASE("test_app_lifecycle_full_cycle: register, launch, suspend, kill transitions are correct", "[integration]")
{
    full_setup();

    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_integ_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.integ.app_a"));

    app_handle_t fg = app_manager_get_foreground();
    TEST_ASSERT_NOT_EQUAL(APP_HANDLE_INVALID, fg);
    TEST_ASSERT_EQUAL_INT(APP_STATE_RUNNING, app_manager_get_state(fg));

    /* Suspend */
    reset_app_mock_state();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_suspend(fg));
    TEST_ASSERT_EQUAL_INT(1, s_on_pause_calls);
    TEST_ASSERT_EQUAL_INT(APP_STATE_SUSPENDED, app_manager_get_state(fg));

    /* Kill */
    reset_app_mock_state();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_kill(fg));
    TEST_ASSERT_EQUAL_INT(1, s_on_destroy_calls);
    TEST_ASSERT_EQUAL_INT(APP_STATE_UNLOADED, app_manager_get_state(fg));
}
