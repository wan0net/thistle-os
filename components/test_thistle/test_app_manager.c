/*
 * test_app_manager.c — Unit tests for the ThistleOS app manager
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls app_manager_init() (and event_bus_init()) to reset all
 * state, making tests fully self-contained.
 *
 * The app manager calls event_publish() internally (for EVENT_APP_LAUNCHED and
 * EVENT_APP_STOPPED), so the event bus must be initialised first.
 */

#include "unity.h"
#include "thistle/app_manager.h"
#include "thistle/event.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Mock app callback tracking
 * -------------------------------------------------------------------------- */

static volatile int s_on_create_calls  = 0;
static volatile int s_on_start_calls   = 0;
static volatile int s_on_pause_calls   = 0;
static volatile int s_on_resume_calls  = 0;
static volatile int s_on_destroy_calls = 0;

static void reset_mock_state(void)
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

/* --------------------------------------------------------------------------
 * Mock app definitions
 * -------------------------------------------------------------------------- */

static const app_manifest_t s_manifest_a = {
    .id               = "com.test.app_a",
    .name             = "Test App A",
    .version          = "1.0.0",
    .allow_background = false,
    .min_memory_kb    = 0,
};

static const app_entry_t s_app_a = {
    .on_create  = mock_on_create,
    .on_start   = mock_on_start,
    .on_pause   = mock_on_pause,
    .on_resume  = mock_on_resume,
    .on_destroy = mock_on_destroy,
    .manifest   = &s_manifest_a,
};

static const app_manifest_t s_manifest_b = {
    .id               = "com.test.app_b",
    .name             = "Test App B",
    .version          = "1.0.0",
    .allow_background = true,
    .min_memory_kb    = 0,
};

static const app_entry_t __attribute__((unused)) s_app_b = {
    .on_create  = mock_on_create,
    .on_start   = mock_on_start,
    .on_pause   = mock_on_pause,
    .on_resume  = mock_on_resume,
    .on_destroy = mock_on_destroy,
    .manifest   = &s_manifest_b,
};

/* --------------------------------------------------------------------------
 * Helper: init both the event bus and app manager cleanly
 * -------------------------------------------------------------------------- */

static void setup(void)
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_init());
    reset_mock_state();
}

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("app_manager_init returns ESP_OK", "[app]")
{
    TEST_ASSERT_EQUAL(ESP_OK, event_bus_init());
    esp_err_t ret = app_manager_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("app_manager_register accepts a valid app_entry_t", "[app]")
{
    setup();
    esp_err_t ret = app_manager_register(&s_app_a);
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("app_manager_launch invokes on_create then on_start", "[app]")
{
    setup();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));

    esp_err_t ret = app_manager_launch("com.test.app_a");
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    TEST_ASSERT_EQUAL_INT(1, s_on_create_calls);
    TEST_ASSERT_EQUAL_INT(1, s_on_start_calls);
}

TEST_CASE("app_manager_get_foreground returns valid handle after launch", "[app]")
{
    setup();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.test.app_a"));

    app_handle_t fg = app_manager_get_foreground();
    TEST_ASSERT_NOT_EQUAL(APP_HANDLE_INVALID, fg);
}

TEST_CASE("app_manager_suspend calls on_pause and sets state to SUSPENDED", "[app]")
{
    setup();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.test.app_a"));

    app_handle_t fg = app_manager_get_foreground();
    TEST_ASSERT_NOT_EQUAL(APP_HANDLE_INVALID, fg);

    reset_mock_state();  /* clear counts from launch */
    esp_err_t ret = app_manager_suspend(fg);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    TEST_ASSERT_EQUAL_INT(1, s_on_pause_calls);
    TEST_ASSERT_EQUAL_INT(APP_STATE_SUSPENDED, app_manager_get_state(fg));
}

TEST_CASE("app_manager_kill calls on_destroy and sets state to UNLOADED", "[app]")
{
    setup();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.test.app_a"));

    app_handle_t fg = app_manager_get_foreground();
    TEST_ASSERT_NOT_EQUAL(APP_HANDLE_INVALID, fg);

    reset_mock_state();  /* clear counts from launch */
    esp_err_t ret = app_manager_kill(fg);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    TEST_ASSERT_EQUAL_INT(1, s_on_destroy_calls);
    TEST_ASSERT_EQUAL_INT(APP_STATE_UNLOADED, app_manager_get_state(fg));
}

/* --------------------------------------------------------------------------
 * Additional edge-case tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_app_launch_nonexistent: launching unknown app ID returns error", "[app]")
{
    setup();

    esp_err_t ret = app_manager_launch("com.no.such.app");
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_app_double_register: registering the same app twice returns error", "[app]")
{
    setup();

    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));

    /* Second registration of the same entry must be rejected */
    esp_err_t ret = app_manager_register(&s_app_a);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_app_kill_calls_destroy: on_destroy invoked after kill", "[app]")
{
    setup();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_register(&s_app_a));
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_launch("com.test.app_a"));

    app_handle_t fg = app_manager_get_foreground();
    TEST_ASSERT_NOT_EQUAL(APP_HANDLE_INVALID, fg);

    reset_mock_state();
    TEST_ASSERT_EQUAL(ESP_OK, app_manager_kill(fg));

    TEST_ASSERT_EQUAL_INT(1, s_on_destroy_calls);
    TEST_ASSERT_EQUAL_INT(APP_STATE_UNLOADED, app_manager_get_state(fg));
}

TEST_CASE("test_app_manager_get_free_memory", "[app]")
{
    size_t free = app_manager_get_free_memory();
    TEST_ASSERT_GREATER_THAN(0, free);
}
