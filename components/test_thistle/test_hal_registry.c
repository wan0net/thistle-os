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

/* --------------------------------------------------------------------------
 * Mock radio driver
 * -------------------------------------------------------------------------- */

static esp_err_t mock_radio_init(const void *c)                              { return ESP_OK; }
static void      mock_radio_deinit(void)                                     {}
static esp_err_t mock_radio_set_freq(uint32_t f)                             { return ESP_OK; }
static esp_err_t mock_radio_set_tx_power(int8_t d)                          { return ESP_OK; }
static esp_err_t mock_radio_set_bw(uint32_t b)                               { return ESP_OK; }
static esp_err_t mock_radio_set_sf(uint8_t s)                                { return ESP_OK; }
static esp_err_t mock_radio_send(const uint8_t *d, size_t l)                 { return ESP_OK; }
static esp_err_t mock_radio_start_rx(hal_radio_rx_cb_t cb, void *ud)         { return ESP_OK; }
static esp_err_t mock_radio_stop_rx(void)                                    { return ESP_OK; }
static int       mock_radio_get_rssi(void)                                   { return -70; }
static esp_err_t mock_radio_sleep(bool e)                                    { return ESP_OK; }

static const hal_radio_driver_t s_mock_radio = {
    .init               = mock_radio_init,
    .deinit             = mock_radio_deinit,
    .set_frequency      = mock_radio_set_freq,
    .set_tx_power       = mock_radio_set_tx_power,
    .set_bandwidth      = mock_radio_set_bw,
    .set_spreading_factor = mock_radio_set_sf,
    .send               = mock_radio_send,
    .start_receive      = mock_radio_start_rx,
    .stop_receive       = mock_radio_stop_rx,
    .get_rssi           = mock_radio_get_rssi,
    .sleep              = mock_radio_sleep,
    .name               = "mock_radio",
};

/* --------------------------------------------------------------------------
 * Mock GPS driver
 * -------------------------------------------------------------------------- */

static esp_err_t mock_gps_init(const void *c)                                { return ESP_OK; }
static void      mock_gps_deinit(void)                                       {}
static esp_err_t mock_gps_enable(void)                                       { return ESP_OK; }
static esp_err_t mock_gps_disable(void)                                      { return ESP_OK; }
static esp_err_t mock_gps_get_position(hal_gps_position_t *p)                { return ESP_OK; }
static esp_err_t mock_gps_register_callback(hal_gps_cb_t cb, void *ud)       { return ESP_OK; }
static esp_err_t mock_gps_sleep(bool e)                                      { return ESP_OK; }

static const hal_gps_driver_t s_mock_gps = {
    .init              = mock_gps_init,
    .deinit            = mock_gps_deinit,
    .enable            = mock_gps_enable,
    .disable           = mock_gps_disable,
    .get_position      = mock_gps_get_position,
    .register_callback = mock_gps_register_callback,
    .sleep             = mock_gps_sleep,
    .name              = "mock_gps",
};

/* --------------------------------------------------------------------------
 * Mock audio driver
 * -------------------------------------------------------------------------- */

static esp_err_t mock_audio_init(const void *c)                              { return ESP_OK; }
static void      mock_audio_deinit(void)                                     {}
static esp_err_t mock_audio_play(const uint8_t *d, size_t l)                 { return ESP_OK; }
static esp_err_t mock_audio_stop(void)                                       { return ESP_OK; }
static esp_err_t mock_audio_set_volume(uint8_t pct)                          { return ESP_OK; }
static esp_err_t mock_audio_configure(const hal_audio_config_t *cfg)         { return ESP_OK; }

static const hal_audio_driver_t __attribute__((unused)) s_mock_audio = {
    .init       = mock_audio_init,
    .deinit     = mock_audio_deinit,
    .play       = mock_audio_play,
    .stop       = mock_audio_stop,
    .set_volume = mock_audio_set_volume,
    .configure  = mock_audio_configure,
    .name       = "mock_audio",
};

/* --------------------------------------------------------------------------
 * Mock power driver
 * -------------------------------------------------------------------------- */

static esp_err_t mock_power_init(const void *c)                              { return ESP_OK; }
static void      mock_power_deinit(void)                                     {}
static esp_err_t mock_power_get_info(hal_power_info_t *info)                 { return ESP_OK; }
static uint16_t  mock_power_get_battery_mv(void)                             { return 3700; }
static uint8_t   mock_power_get_battery_percent(void)                        { return 75; }
static bool      mock_power_is_charging(void)                                { return false; }
static esp_err_t mock_power_sleep(bool e)                                    { return ESP_OK; }

static const hal_power_driver_t s_mock_power = {
    .init                 = mock_power_init,
    .deinit               = mock_power_deinit,
    .get_info             = mock_power_get_info,
    .get_battery_mv       = mock_power_get_battery_mv,
    .get_battery_percent  = mock_power_get_battery_percent,
    .is_charging          = mock_power_is_charging,
    .sleep                = mock_power_sleep,
    .name                 = "mock_power",
};

/* --------------------------------------------------------------------------
 * Additional HAL registry tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_hal_radio_register: mock radio stored and registry field is non-NULL", "[hal]")
{
    esp_err_t ret = hal_radio_register(&s_mock_radio, NULL);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_NOT_NULL(reg->radio);
}

TEST_CASE("test_hal_gps_register: mock GPS name matches after registration", "[hal]")
{
    esp_err_t ret = hal_gps_register(&s_mock_gps, NULL);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_NOT_NULL(reg->gps);
    TEST_ASSERT_EQUAL_STRING("mock_gps", reg->gps->name);
}

TEST_CASE("test_hal_audio_register_null_rejected: hal_audio_register(NULL, NULL) returns ESP_ERR_INVALID_ARG", "[hal]")
{
    esp_err_t ret = hal_audio_register(NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_hal_power_register: mock power driver is accessible via get_registry", "[hal]")
{
    esp_err_t ret = hal_power_register(&s_mock_power, NULL);
    TEST_ASSERT_EQUAL(ESP_OK, ret);

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_NOT_NULL(reg->power);
    TEST_ASSERT_EQUAL_STRING("mock_power", reg->power->name);
}

TEST_CASE("test_hal_radio_register_null_rejected: hal_radio_register(NULL, NULL) returns ESP_ERR_INVALID_ARG", "[hal]")
{
    esp_err_t ret = hal_radio_register(NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_hal_gps_register_null_rejected: hal_gps_register(NULL, NULL) returns ESP_ERR_INVALID_ARG", "[hal]")
{
    esp_err_t ret = hal_gps_register(NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_hal_power_register_null_rejected: hal_power_register(NULL, NULL) returns ESP_ERR_INVALID_ARG", "[hal]")
{
    esp_err_t ret = hal_power_register(NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_hal_radio_name_matches: registered radio name is 'mock_radio'", "[hal]")
{
    TEST_ASSERT_EQUAL(ESP_OK, hal_radio_register(&s_mock_radio, NULL));

    const hal_registry_t *reg = hal_get_registry();
    TEST_ASSERT_NOT_NULL(reg->radio);
    TEST_ASSERT_EQUAL_STRING("mock_radio", reg->radio->name);
}
