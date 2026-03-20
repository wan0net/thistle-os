/*
 * test_ipc.c — Unit tests for the ThistleOS kernel IPC subsystem
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls ipc_init() to create a fresh queue and clear handler state.
 * ipc_init() allocates a FreeRTOS queue, so tests must run inside a FreeRTOS
 * task context (which is guaranteed by the ESP-IDF Unity runner).
 */

#include "unity.h"
#include "thistle/ipc.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Handler tracking state
 * -------------------------------------------------------------------------- */

static volatile int     s_handler_calls = 0;
static volatile uint32_t s_handler_msg_type = 0;
static uint8_t          s_handler_data[IPC_MSG_MAX_DATA];
static size_t           s_handler_data_len = 0;

static void reset_handler_state(void)
{
    s_handler_calls    = 0;
    s_handler_msg_type = 0;
    s_handler_data_len = 0;
    memset(s_handler_data, 0, sizeof(s_handler_data));
}

static void capture_handler(const ipc_message_t *msg, void *user_data)
{
    s_handler_calls++;
    s_handler_msg_type = msg->msg_type;
    s_handler_data_len = msg->data_len;
    if (msg->data_len > 0 && msg->data_len <= IPC_MSG_MAX_DATA) {
        memcpy(s_handler_data, msg->data, msg->data_len);
    }
}

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("ipc_init returns ESP_OK", "[ipc]")
{
    esp_err_t ret = ipc_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("ipc_send and ipc_recv round-trip preserves all fields", "[ipc]")
{
    TEST_ASSERT_EQUAL(ESP_OK, ipc_init());

    ipc_message_t tx = {
        .src_app   = 1,
        .dst_app   = 2,
        .msg_type  = 99,
        .data_len  = 4,
        .timestamp = 12345,
    };
    tx.data[0] = 0xAA;
    tx.data[1] = 0xBB;
    tx.data[2] = 0xCC;
    tx.data[3] = 0xDD;

    TEST_ASSERT_EQUAL(ESP_OK, ipc_send(&tx));

    ipc_message_t rx;
    memset(&rx, 0, sizeof(rx));
    TEST_ASSERT_EQUAL(ESP_OK, ipc_recv(&rx, 50));

    TEST_ASSERT_EQUAL_UINT32(1,     rx.src_app);
    TEST_ASSERT_EQUAL_UINT32(2,     rx.dst_app);
    TEST_ASSERT_EQUAL_UINT32(99,    rx.msg_type);
    TEST_ASSERT_EQUAL_UINT32(12345, rx.timestamp);
    TEST_ASSERT_EQUAL_size_t(4,     rx.data_len);
    TEST_ASSERT_EQUAL_UINT8(0xAA, rx.data[0]);
    TEST_ASSERT_EQUAL_UINT8(0xBB, rx.data[1]);
    TEST_ASSERT_EQUAL_UINT8(0xCC, rx.data[2]);
    TEST_ASSERT_EQUAL_UINT8(0xDD, rx.data[3]);
}

TEST_CASE("ipc_recv with no messages returns ESP_ERR_TIMEOUT", "[ipc]")
{
    TEST_ASSERT_EQUAL(ESP_OK, ipc_init());

    ipc_message_t rx;
    memset(&rx, 0, sizeof(rx));

    /* Use a short timeout — 50 ms is sufficient to confirm the queue is empty */
    esp_err_t ret = ipc_recv(&rx, 50);
    TEST_ASSERT_EQUAL(ESP_ERR_TIMEOUT, ret);
}

TEST_CASE("ipc_register_handler is called synchronously on ipc_send", "[ipc]")
{
    TEST_ASSERT_EQUAL(ESP_OK, ipc_init());
    reset_handler_state();

    TEST_ASSERT_EQUAL(ESP_OK, ipc_register_handler(42, capture_handler, NULL));

    ipc_message_t tx = {
        .src_app  = 0,
        .dst_app  = 0,
        .msg_type = 42,
        .data_len = 3,
    };
    tx.data[0] = 'H';
    tx.data[1] = 'i';
    tx.data[2] = '!';

    TEST_ASSERT_EQUAL(ESP_OK, ipc_send(&tx));

    TEST_ASSERT_EQUAL_INT(1,  s_handler_calls);
    TEST_ASSERT_EQUAL_UINT32(42, s_handler_msg_type);
    TEST_ASSERT_EQUAL_size_t(3,  s_handler_data_len);
    TEST_ASSERT_EQUAL_UINT8('H', s_handler_data[0]);
    TEST_ASSERT_EQUAL_UINT8('i', s_handler_data[1]);
    TEST_ASSERT_EQUAL_UINT8('!', s_handler_data[2]);
}

TEST_CASE("ipc_send and ipc_recv maintain FIFO ordering for multiple messages", "[ipc]")
{
    TEST_ASSERT_EQUAL(ESP_OK, ipc_init());

    const int MSG_COUNT = 5;

    for (int i = 0; i < MSG_COUNT; i++) {
        ipc_message_t tx = {
            .src_app  = 0,
            .dst_app  = 0,
            .msg_type = (uint32_t)i,
            .data_len = 1,
        };
        tx.data[0] = (uint8_t)(i * 10);
        TEST_ASSERT_EQUAL(ESP_OK, ipc_send(&tx));
    }

    for (int i = 0; i < MSG_COUNT; i++) {
        ipc_message_t rx;
        memset(&rx, 0, sizeof(rx));
        TEST_ASSERT_EQUAL(ESP_OK, ipc_recv(&rx, 50));
        TEST_ASSERT_EQUAL_UINT32((uint32_t)i, rx.msg_type);
        TEST_ASSERT_EQUAL_UINT8((uint8_t)(i * 10), rx.data[0]);
    }
}
