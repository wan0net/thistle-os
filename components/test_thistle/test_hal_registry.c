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

/* --------------------------------------------------------------------------
 * Mock storage drivers
 * -------------------------------------------------------------------------- */

static esp_err_t mock_storage_init(const void *c)      { return ESP_OK; }
static void      mock_storage_deinit(void)              {}
static esp_err_t mock_storage_mount(const char *mp)    { return ESP_OK; }
static esp_err_t mock_storage_unmount(void)             { return ESP_OK; }
static bool      mock_storage_is_mounted(void)          { return false; }
static uint64_t  mock_storage_total(void)               { return 0; }
static uint64_t  mock_storage_free(void)                { return 0; }

static const hal_storage_driver_t mock_storage_1 = {
    .init           = mock_storage_init,
    .deinit         = mock_storage_deinit,
    .mount          = mock_storage_mount,
    .unmount        = mock_storage_unmount,
    .is_mounted     = mock_storage_is_mounted,
    .get_total_bytes = mock_storage_total,
    .get_free_bytes  = mock_storage_free,
    .type           = HAL_STORAGE_TYPE_SD,
    .name           = "mock_sd",
};

static const hal_storage_driver_t mock_storage_2 = {
    .init           = mock_storage_init,
    .deinit         = mock_storage_deinit,
    .mount          = mock_storage_mount,
    .unmount        = mock_storage_unmount,
    .is_mounted     = mock_storage_is_mounted,
    .get_total_bytes = mock_storage_total,
    .get_free_bytes  = mock_storage_free,
    .type           = HAL_STORAGE_TYPE_INTERNAL,
    .name           = "mock_internal",
};

TEST_CASE("test_hal_storage_register_multiple: two storage drivers both accessible", "[hal]")
{
    /*
     * The HAL registry is a static global; storage slots accumulate across
     * tests. We need at least 2 free slots. This test only works correctly
     * when run in isolation (or first in the storage suite).
     */
    const hal_registry_t *reg = hal_get_registry();
    uint8_t prior = reg->storage_count;

    TEST_ASSERT_TRUE_MESSAGE(prior + 2 <= HAL_MAX_STORAGE_DRIVERS,
        "Not enough free storage slots — run test in isolation");

    TEST_ASSERT_EQUAL(ESP_OK, hal_storage_register(&mock_storage_1, NULL));
    TEST_ASSERT_EQUAL(ESP_OK, hal_storage_register(&mock_storage_2, NULL));

    TEST_ASSERT_EQUAL_UINT8(prior + 2, reg->storage_count);
    TEST_ASSERT_EQUAL_STRING("mock_sd",       reg->storage[prior]->name);
    TEST_ASSERT_EQUAL_STRING("mock_internal", reg->storage[prior + 1]->name);
}

TEST_CASE("test_hal_storage_overflow: registering beyond HAL_MAX_STORAGE_DRIVERS fails", "[hal]")
{
    const hal_registry_t *reg = hal_get_registry();

    /* Fill any remaining slots */
    while (reg->storage_count < HAL_MAX_STORAGE_DRIVERS) {
        esp_err_t r = hal_storage_register(&mock_storage_1, NULL);
        TEST_ASSERT_EQUAL(ESP_OK, r);
    }

    /* One more must fail with no-mem */
    esp_err_t ret = hal_storage_register(&mock_storage_1, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_NO_MEM, ret);
}

TEST_CASE("test_hal_display_config_preserved: display_config pointer matches what was passed", "[hal]")
{
    static const uint32_t cfg_sentinel = 0xCAFEBABE;

    esp_err_t ret = hal_display_register(&mock_display, &cfg_sentinel);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_EQUAL_PTR(&cfg_sentinel, reg->display_config);
}
