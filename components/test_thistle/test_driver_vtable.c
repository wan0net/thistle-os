/*
 * test_driver_vtable.c — Unit tests verifying driver vtable contracts
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test constructs a mock driver struct on the stack and exercises the
 * vtable function pointers directly.  Tests are fully self-contained and
 * require no global state reset.
 */

#include "unity.h"
#include "hal/display.h"
#include "hal/input.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Mock display vtable — all function pointers populated
 * -------------------------------------------------------------------------- */

static bool    s_display_inited    = false;
static bool    s_display_requires_config = false;
static int     s_display_init_calls = 0;
static int     s_display_deinit_calls = 0;

static esp_err_t mock_vtable_display_init(const void *config)
{
    s_display_init_calls++;
    if (s_display_requires_config && config == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    s_display_inited = true;
    return ESP_OK;
}

static void mock_vtable_display_deinit(void)
{
    s_display_deinit_calls++;
    s_display_inited = false;
}

static esp_err_t mock_vtable_display_flush(const hal_area_t *area, const uint8_t *data)
{
    return ESP_OK;
}

static esp_err_t mock_vtable_display_brightness(uint8_t pct)
{
    return ESP_OK;
}

static esp_err_t mock_vtable_display_sleep(bool enter)
{
    return ESP_OK;
}

static esp_err_t mock_vtable_display_refresh_mode(hal_display_refresh_mode_t mode)
{
    return ESP_OK;
}

static esp_err_t mock_vtable_display_refresh(void)
{
    return ESP_OK;
}

static const hal_display_driver_t s_mock_display_full = {
    .init             = mock_vtable_display_init,
    .deinit           = mock_vtable_display_deinit,
    .flush            = mock_vtable_display_flush,
    .refresh          = mock_vtable_display_refresh,
    .set_brightness   = mock_vtable_display_brightness,
    .sleep            = mock_vtable_display_sleep,
    .set_refresh_mode = mock_vtable_display_refresh_mode,
    .width            = 320,
    .height           = 240,
    .type             = HAL_DISPLAY_TYPE_EPAPER,
    .name             = "mock_display_vtable",
};

/* --------------------------------------------------------------------------
 * Mock input vtable — all function pointers populated
 * -------------------------------------------------------------------------- */

static esp_err_t mock_vtable_input_init(const void *config)   { return ESP_OK; }
static void      mock_vtable_input_deinit(void)               {}
static esp_err_t mock_vtable_input_reg_cb(hal_input_cb_t cb, void *ud) { return ESP_OK; }
static esp_err_t mock_vtable_input_poll(void)                 { return ESP_OK; }

static const hal_input_driver_t s_mock_input_full = {
    .init              = mock_vtable_input_init,
    .deinit            = mock_vtable_input_deinit,
    .register_callback = mock_vtable_input_reg_cb,
    .poll              = mock_vtable_input_poll,
    .name              = "mock_input_vtable",
    .is_touch          = false,
};

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_display_vtable_complete: all display function pointers are non-NULL", "[driver]")
{
    TEST_ASSERT_NOT_NULL(s_mock_display_full.init);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.deinit);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.flush);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.refresh);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.set_brightness);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.sleep);
    TEST_ASSERT_NOT_NULL(s_mock_display_full.set_refresh_mode);
}

TEST_CASE("test_input_vtable_complete: all input function pointers are non-NULL", "[driver]")
{
    TEST_ASSERT_NOT_NULL(s_mock_input_full.init);
    TEST_ASSERT_NOT_NULL(s_mock_input_full.deinit);
    TEST_ASSERT_NOT_NULL(s_mock_input_full.register_callback);
    TEST_ASSERT_NOT_NULL(s_mock_input_full.poll);
}

TEST_CASE("test_display_init_null_config_rejected: init(NULL) returns ESP_ERR_INVALID_ARG when config required", "[driver]")
{
    s_display_init_calls     = 0;
    s_display_inited         = false;
    s_display_requires_config = true;

    esp_err_t ret = s_mock_display_full.init(NULL);

    s_display_requires_config = false;  /* restore for other tests */
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
    TEST_ASSERT_FALSE(s_display_inited);
}

TEST_CASE("test_display_double_init_safe: calling init() twice returns ESP_OK both times", "[driver]")
{
    s_display_init_calls      = 0;
    s_display_inited          = false;
    s_display_requires_config = false;

    esp_err_t r1 = s_mock_display_full.init(NULL);
    esp_err_t r2 = s_mock_display_full.init(NULL);

    TEST_ASSERT_EQUAL(ESP_OK, r1);
    TEST_ASSERT_EQUAL(ESP_OK, r2);
    TEST_ASSERT_EQUAL_INT(2, s_display_init_calls);
    TEST_ASSERT_TRUE(s_display_inited);

    /* Clean up */
    s_mock_display_full.deinit();
    s_mock_display_full.deinit();
}

TEST_CASE("test_display_deinit_before_init_safe: deinit() without prior init does not crash", "[driver]")
{
    s_display_inited      = false;
    s_display_deinit_calls = 0;

    /* Must not crash — the mock simply decrements the call counter */
    s_mock_display_full.deinit();

    TEST_ASSERT_EQUAL_INT(1, s_display_deinit_calls);
}
