/* SPDX-License-Identifier: BSD-3-Clause */

#include "unity.h"
#include "thistle/board_bus_registration.h"

static int s_register_calls;
static int s_cleanup_calls;
static int s_last_bus_id;
static void *s_last_resource;

static esp_err_t register_ok(int bus_id, void *resource)
{
    s_register_calls++;
    s_last_bus_id = bus_id;
    s_last_resource = resource;
    return ESP_OK;
}

static esp_err_t register_full(int bus_id, void *resource)
{
    s_register_calls++;
    s_last_bus_id = bus_id;
    s_last_resource = resource;
    return ESP_ERR_NO_MEM;
}

static esp_err_t cleanup_ok(void *resource)
{
    s_cleanup_calls++;
    s_last_resource = resource;
    return ESP_OK;
}

static esp_err_t cleanup_fail(void *resource)
{
    s_cleanup_calls++;
    s_last_resource = resource;
    return ESP_FAIL;
}

static void reset_mocks(void)
{
    s_register_calls = 0;
    s_cleanup_calls = 0;
    s_last_bus_id = -1;
    s_last_resource = NULL;
}

TEST_CASE("board bus registration keeps accepted resource", "[board][bus]")
{
    int resource;
    reset_mocks();

    TEST_ASSERT_EQUAL(ESP_OK,
                      board_bus_register_resource(2, &resource, register_ok,
                                                  cleanup_ok));
    TEST_ASSERT_EQUAL(1, s_register_calls);
    TEST_ASSERT_EQUAL(0, s_cleanup_calls);
    TEST_ASSERT_EQUAL(2, s_last_bus_id);
    TEST_ASSERT_EQUAL_PTR(&resource, s_last_resource);
}

TEST_CASE("board bus registration cleans rejected resource", "[board][bus]")
{
    int resource;
    reset_mocks();

    TEST_ASSERT_EQUAL(ESP_ERR_NO_MEM,
                      board_bus_register_resource(0, &resource, register_full,
                                                  cleanup_ok));
    TEST_ASSERT_EQUAL(1, s_register_calls);
    TEST_ASSERT_EQUAL(1, s_cleanup_calls);
    TEST_ASSERT_EQUAL_PTR(&resource, s_last_resource);
}

TEST_CASE("board bus registration rejects null encoded resource", "[board][bus]")
{
    reset_mocks();

    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG,
                      board_bus_register_resource(0, NULL, register_ok,
                                                  cleanup_ok));
    TEST_ASSERT_EQUAL(0, s_register_calls);
    TEST_ASSERT_EQUAL(0, s_cleanup_calls);
}

TEST_CASE("board bus registration preserves rejection when cleanup fails", "[board][bus]")
{
    int resource;
    reset_mocks();

    TEST_ASSERT_EQUAL(ESP_ERR_NO_MEM,
                      board_bus_register_resource(1, &resource, register_full,
                                                  cleanup_fail));
    TEST_ASSERT_EQUAL(1, s_cleanup_calls);
}

TEST_CASE("board SPI initialization rejects host zero encoding", "[board][bus]")
{
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG,
                      board_bus_init_spi(0, 1, 2, 3, 4096));
}
