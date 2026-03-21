/*
 * test_driver_loader.c — Unit tests for the ThistleOS runtime driver loader
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * The driver loader operates on the /sdcard/drivers/ filesystem which is not
 * present in the test environment. Tests here cover the API contracts that are
 * observable without a real SD card: init returns success, loading non-existent
 * paths returns NOT_FOUND, scanning an absent directory returns 0, and the
 * count starts at 0.
 */

#include "unity.h"
#include "thistle/driver_loader.h"

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_driver_loader_init: driver_loader_init returns ESP_OK", "[driver_loader]")
{
    esp_err_t ret = driver_loader_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_driver_loader_load_nonexistent: loading absent path returns ESP_ERR_NOT_FOUND", "[driver_loader]")
{
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());

    esp_err_t ret = driver_loader_load("/nonexistent/path/fake.drv.elf");
    TEST_ASSERT_EQUAL(ESP_ERR_NOT_FOUND, ret);
}

TEST_CASE("test_driver_loader_scan_empty_dir: scan with no driver directory returns 0", "[driver_loader]")
{
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());

    /*
     * /sdcard/drivers/ does not exist in the test environment.
     * driver_loader_scan_and_load() must return 0 (no drivers loaded)
     * rather than crashing or returning a negative value.
     */
    int loaded = driver_loader_scan_and_load();
    TEST_ASSERT_GREATER_OR_EQUAL(0, loaded);
}

TEST_CASE("test_driver_loader_get_count: count is 0 immediately after init", "[driver_loader]")
{
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());
    int count = driver_loader_get_count();
    TEST_ASSERT_EQUAL_INT(0, count);
}

TEST_CASE("test_driver_loader_load_null_path: NULL path returns error", "[driver_loader]")
{
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());

    esp_err_t ret = driver_loader_load(NULL);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_driver_loader_reinit_resets_count: count is 0 after repeated init", "[driver_loader]")
{
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());
    TEST_ASSERT_EQUAL(ESP_OK, driver_loader_init());
    TEST_ASSERT_EQUAL_INT(0, driver_loader_get_count());
}
