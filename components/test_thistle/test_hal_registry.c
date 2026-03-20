/*
 * test_hal_registry.c — Unit tests for the ThistleOS HAL registry
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * NOTE: The HAL registry uses a static global that is NOT reset between tests.
 * Tests that read state after registration must account for prior test runs.
 * The input overflow test relies on the cumulative count; order matters within
 * that single test case.
 */

#include "unity.h"
#include "hal/board.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Mock display driver
 * -------------------------------------------------------------------------- */

static esp_err_t mock_display_init(const void *config) { return ESP_OK; }
static void      mock_display_deinit(void) {}
static esp_err_t mock_display_flush(const hal_area_t *area, const uint8_t *data) { return ESP_OK; }
static esp_err_t mock_display_brightness(uint8_t pct) { return ESP_OK; }
static esp_err_t mock_display_sleep(bool enter) { return ESP_OK; }
static esp_err_t mock_display_refresh_mode(hal_display_refresh_mode_t mode) { return ESP_OK; }

static const hal_display_driver_t mock_display = {
    .init             = mock_display_init,
    .deinit           = mock_display_deinit,
    .flush            = mock_display_flush,
    .set_brightness   = mock_display_brightness,
    .sleep            = mock_display_sleep,
    .set_refresh_mode = mock_display_refresh_mode,
    .width            = 320,
    .height           = 240,
    .type             = HAL_DISPLAY_TYPE_EPAPER,
    .name             = "mock_display",
};

/* Config sentinel for pointer-identity tests */
static const uint32_t mock_display_cfg = 0xDEADBEEF;

/* --------------------------------------------------------------------------
 * Mock input drivers
 * -------------------------------------------------------------------------- */

static esp_err_t mock_input_init(const void *c) { return ESP_OK; }
static void      mock_input_deinit(void) {}
static esp_err_t mock_input_reg_cb(hal_input_cb_t cb, void *ud) { return ESP_OK; }
static esp_err_t mock_input_poll(void) { return ESP_OK; }

static const hal_input_driver_t mock_input_1 = {
    .init              = mock_input_init,
    .deinit            = mock_input_deinit,
    .register_callback = mock_input_reg_cb,
    .poll              = mock_input_poll,
    .name              = "mock_kbd",
    .is_touch          = false,
};

static const hal_input_driver_t mock_input_2 = {
    .init              = mock_input_init,
    .deinit            = mock_input_deinit,
    .register_callback = mock_input_reg_cb,
    .poll              = mock_input_poll,
    .name              = "mock_touch",
    .is_touch          = true,
};

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("hal_display_register stores driver and is retrievable", "[hal]")
{
    esp_err_t ret = hal_display_register(&mock_display, NULL);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_NOT_NULL(reg);
    TEST_ASSERT_NOT_NULL(reg->display);
    TEST_ASSERT_EQUAL_STRING("mock_display", reg->display->name);
    TEST_ASSERT_EQUAL_UINT16(320, reg->display->width);
    TEST_ASSERT_EQUAL_UINT16(240, reg->display->height);
}

TEST_CASE("hal_display_register rejects NULL driver", "[hal]")
{
    esp_err_t ret = hal_display_register(NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("hal_input_register stores multiple drivers in order", "[hal]")
{
    /*
     * The registry is a static global. We register two inputs here and verify
     * their slots. Prior tests may have already registered inputs; we capture
     * the count before and verify the two new slots are added correctly.
     */
    const hal_registry_t *reg = hal_get_registry();
    uint8_t prior_count = reg->input_count;

    /* Only run if there is room for both */
    TEST_ASSERT_TRUE_MESSAGE(prior_count + 2 <= HAL_MAX_INPUT_DRIVERS,
                             "Not enough free input slots for this test — run tests in isolation");

    TEST_ASSERT_EQUAL(ESP_OK, hal_input_register(&mock_input_1, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, hal_input_register(&mock_input_2, NULL));

    TEST_ASSERT_EQUAL_UINT8(prior_count + 2, reg->input_count);
    TEST_ASSERT_EQUAL_STRING("mock_kbd",   reg->inputs[prior_count]->name);
    TEST_ASSERT_EQUAL_STRING("mock_touch", reg->inputs[prior_count + 1]->name);
    TEST_ASSERT_FALSE(reg->inputs[prior_count]->is_touch);
    TEST_ASSERT_TRUE(reg->inputs[prior_count + 1]->is_touch);
}

TEST_CASE("hal_input_register returns ESP_ERR_NO_MEM when slots exhausted", "[hal]")
{
    const hal_registry_t *reg = hal_get_registry();

    /* Fill remaining slots to reach HAL_MAX_INPUT_DRIVERS */
    static const hal_input_driver_t filler = {
        .init              = mock_input_init,
        .deinit            = mock_input_deinit,
        .register_callback = mock_input_reg_cb,
        .poll              = mock_input_poll,
        .name              = "filler",
        .is_touch          = false,
    };

    while (reg->input_count < HAL_MAX_INPUT_DRIVERS) {
        esp_err_t r = hal_input_register(&filler, NULL);
        TEST_ASSERT_EQUAL(ESP_OK, r);
    }

    /* Now one more must fail */
    esp_err_t ret = hal_input_register(&filler, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_NO_MEM, ret);
}

TEST_CASE("hal_set_board_name stores the name string", "[hal]")
{
    esp_err_t ret = hal_set_board_name("TestBoard");
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_EQUAL_STRING("TestBoard", reg->board_name);
}

TEST_CASE("hal_display_register stores config pointer identity", "[hal]")
{
    esp_err_t ret = hal_display_register(&mock_display, &mock_display_cfg);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_EQUAL_PTR(&mock_display_cfg, reg->display_config);
}
