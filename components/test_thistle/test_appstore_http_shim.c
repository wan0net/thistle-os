/* SPDX-License-Identifier: BSD-3-Clause */

#include "unity.h"
#include "esp_err.h"
#include "thistle/appstore_http_shim.h"

#include <string.h>

typedef struct {
    uint8_t data[32];
    size_t data_len;
    int calls;
    int status_code;
} http_fixture_t;

static int32_t fixture_data_cb(const uint8_t *data, size_t data_len,
                               void *user_data)
{
    http_fixture_t *fixture = user_data;
    fixture->calls++;
    fixture->data_len = data_len;
    memcpy(fixture->data, data, data_len);
    return 17;
}

TEST_CASE("appstore HTTP shim dispatches response data and user context",
          "[appstore][http]")
{
    static const uint8_t payload[] = "fixture-response";
    http_fixture_t fixture = {.status_code = 200};

    int32_t ret = thistle_appstore_http_dispatch_data(
        fixture_data_cb, &fixture, payload, sizeof(payload) - 1);

    TEST_ASSERT_EQUAL(17, ret);
    TEST_ASSERT_EQUAL(200, fixture.status_code);
    TEST_ASSERT_EQUAL(1, fixture.calls);
    TEST_ASSERT_EQUAL(sizeof(payload) - 1, fixture.data_len);
    TEST_ASSERT_EQUAL_UINT8_ARRAY(payload, fixture.data, sizeof(payload) - 1);
}

TEST_CASE("appstore HTTP shim rejects invalid callback data",
          "[appstore][http]")
{
    http_fixture_t fixture = {0};

    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG,
                      thistle_appstore_http_dispatch_data(
                          fixture_data_cb, &fixture, NULL, 1));
    TEST_ASSERT_EQUAL(0, fixture.calls);
}
