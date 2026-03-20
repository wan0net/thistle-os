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
