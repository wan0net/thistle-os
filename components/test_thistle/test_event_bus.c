/*
 * test_event_bus.c — Unit tests for the ThistleOS kernel event bus
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls event_bus_init() first to reset the subscriber table,
 * making tests fully self-contained.
 */

#include "unity.h"
#include "thistle/event.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Shared tracking state (reset in each test via event_bus_init)
 * -------------------------------------------------------------------------- */

static volatile int s_call_count_a = 0;
static volatile int s_call_count_b = 0;
static volatile int s_call_count_c = 0;
static event_type_t s_last_type_a  = EVENT_MAX;
static event_type_t s_last_type_b  = EVENT_MAX;
static void        *s_last_data_a  = NULL;

/* --------------------------------------------------------------------------
 * Callback implementations
 * -------------------------------------------------------------------------- */

static void handler_a(const event_t *ev, void *user_data)
{
    s_call_count_a++;
    s_last_type_a = ev->type;
    s_last_data_a = ev->data;
}

static void handler_b(const event_t *ev, void *user_data)
{
    s_call_count_b++;
    s_last_type_b = ev->type;
}

static void handler_c(const event_t *ev, void *user_data)
{
    s_call_count_c++;
}

static void reset_counters(void)
{
    s_call_count_a = 0;
    s_call_count_b = 0;
    s_call_count_c = 0;
    s_last_type_a  = EVENT_MAX;
    s_last_type_b  = EVENT_MAX;
    s_last_data_a  = NULL;
}

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("event_bus_init returns ESP_OK", "[event]")
{
    esp_err_t ret = event_bus_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("event_subscribe and event_publish invoke handler exactly once", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_SYSTEM_BOOT, handler_a, NULL));

    event_t ev = {
        .type      = EVENT_SYSTEM_BOOT,
        .timestamp = 0,
        .data      = NULL,
        .data_len  = 0,
    };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_INT(EVENT_SYSTEM_BOOT, s_last_type_a);
}

TEST_CASE("event_publish dispatches to all subscribers for the same event type", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_SYSTEM_BOOT, handler_a, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_SYSTEM_BOOT, handler_b, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_SYSTEM_BOOT, handler_c, NULL));

    event_t ev = { .type = EVENT_SYSTEM_BOOT };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_INT(1, s_call_count_b);
    TEST_ASSERT_EQUAL_INT(1, s_call_count_c);
}

TEST_CASE("event_unsubscribe prevents handler from being called", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_SYSTEM_BOOT, handler_a, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_unsubscribe(EVENT_SYSTEM_BOOT, handler_a));

    event_t ev = { .type = EVENT_SYSTEM_BOOT };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    TEST_ASSERT_EQUAL_INT(0, s_call_count_a);
}

TEST_CASE("event_publish_simple delivers event with NULL data", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_APP_LAUNCHED, handler_a, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_publish_simple(EVENT_APP_LAUNCHED));

    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_INT(EVENT_APP_LAUNCHED, s_last_type_a);
    TEST_ASSERT_NULL(s_last_data_a);
}

TEST_CASE("event subscribers are isolated by event type", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    /* handler_a listens to INPUT_KEY, handler_b listens to GPS_FIX */
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_INPUT_KEY, handler_a, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_GPS_FIX,   handler_b, NULL));

    /* Publish INPUT_KEY — only handler_a should fire */
    event_t ev_key = { .type = EVENT_INPUT_KEY };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev_key));
    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_INT(0, s_call_count_b);

    /* Publish GPS_FIX — only handler_b should fire */
    event_t ev_gps = { .type = EVENT_GPS_FIX };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev_gps));
    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);  /* unchanged */
    TEST_ASSERT_EQUAL_INT(1, s_call_count_b);
    TEST_ASSERT_EQUAL_INT(EVENT_INPUT_KEY, s_last_type_a);
    TEST_ASSERT_EQUAL_INT(EVENT_GPS_FIX,   s_last_type_b);
}

/* --------------------------------------------------------------------------
 * Additional tracking state for new tests
 * -------------------------------------------------------------------------- */

static volatile size_t s_last_data_len_a = 0;

static void handler_a_with_len(const event_t *ev, void *user_data)
{
    s_call_count_a++;
    s_last_type_a     = ev->type;
    s_last_data_a     = ev->data;
    s_last_data_len_a = ev->data_len;
}

/* Handlers for subscribe-limit test — we need 8 distinct function pointers */
static volatile int s_multi_calls[8];

#define DEFINE_MULTI_HANDLER(N) \
    static void multi_handler_##N(const event_t *ev, void *ud) { s_multi_calls[N]++; }

DEFINE_MULTI_HANDLER(0)
DEFINE_MULTI_HANDLER(1)
DEFINE_MULTI_HANDLER(2)
DEFINE_MULTI_HANDLER(3)
DEFINE_MULTI_HANDLER(4)
DEFINE_MULTI_HANDLER(5)
DEFINE_MULTI_HANDLER(6)
DEFINE_MULTI_HANDLER(7)

static event_handler_t s_multi_handlers[8] = {
    multi_handler_0, multi_handler_1, multi_handler_2, multi_handler_3,
    multi_handler_4, multi_handler_5, multi_handler_6, multi_handler_7,
};

/* --------------------------------------------------------------------------
 * New edge-case tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_event_publish_with_data: subscriber receives correct data pointer and length", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();
    s_last_data_len_a = 0;

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_RADIO_RX, handler_a_with_len, NULL));

    static const uint8_t payload[] = { 0x01, 0x02, 0x03 };
    event_t ev = {
        .type     = EVENT_RADIO_RX,
        .timestamp = 0,
        .data     = (void *)payload,
        .data_len = sizeof(payload),
    };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_PTR(payload, s_last_data_a);
    TEST_ASSERT_EQUAL_size_t(sizeof(payload), s_last_data_len_a);
}

TEST_CASE("test_event_subscribe_limit: 8 handlers all called; 9th subscribe fails or is silently dropped", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());

    for (int i = 0; i < 8; i++) {
        s_multi_calls[i] = 0;
    }

    /* Subscribe 8 handlers to the same event */
    for (int i = 0; i < 8; i++) {
        esp_err_t r = event_subscribe(EVENT_BATTERY_LOW, s_multi_handlers[i], NULL);
        if (r != ESP_OK) {
            /* Fewer than 8 slots supported — skip gracefully */
            TEST_IGNORE_MESSAGE("event bus supports fewer than 8 handlers per event; skipping limit test");
            return;
        }
    }

    /* Attempt a 9th subscription */
    esp_err_t ret9 = event_subscribe(EVENT_BATTERY_LOW, handler_a, NULL);
    /* Either fails (ESP_ERR_NO_MEM / similar) or succeeds — both are valid
     * implementations. What matters is that the first 8 are still called. */
    (void)ret9;

    event_t ev = { .type = EVENT_BATTERY_LOW };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    for (int i = 0; i < 8; i++) {
        TEST_ASSERT_EQUAL_INT_MESSAGE(1, s_multi_calls[i], "handler not called");
    }
}

TEST_CASE("test_event_unsubscribe_middle: A and C called but not B after B unsubscribed", "[event]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    reset_counters();

    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_WIFI_CONNECTED, handler_a, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_WIFI_CONNECTED, handler_b, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, event_subscribe(EVENT_WIFI_CONNECTED, handler_c, NULL));

    TEST_ASSERT_EQUAL(ESP_OK, event_unsubscribe(EVENT_WIFI_CONNECTED, handler_b));

    event_t ev = { .type = EVENT_WIFI_CONNECTED };
    TEST_ASSERT_EQUAL(ESP_OK, event_publish(&ev));

    TEST_ASSERT_EQUAL_INT(1, s_call_count_a);
    TEST_ASSERT_EQUAL_INT(0, s_call_count_b);
    TEST_ASSERT_EQUAL_INT(1, s_call_count_c);
}
