/*
 * test_appstore_client.c — Unit tests for the ThistleOS app store client
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * These tests cover the API contracts that do not require a network connection
 * or a real SD card: default URL shape, NULL argument rejection, and struct
 * field sizes. Network-dependent tests (actual catalog fetch, real download)
 * are excluded as they are hardware/connectivity dependent.
 */

#include "unity.h"
#include "thistle/appstore_client.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_appstore_get_catalog_url_default: default URL contains 'thistle-apps'", "[appstore]")
{
    const char *url = appstore_get_catalog_url();
    TEST_ASSERT_NOT_NULL(url);
    TEST_ASSERT_NOT_NULL_MESSAGE(strstr(url, "thistle-apps"),
                                 "Default catalog URL does not contain 'thistle-apps'");
}

TEST_CASE("test_appstore_get_catalog_url_nonempty: default URL is a non-empty string", "[appstore]")
{
    const char *url = appstore_get_catalog_url();
    TEST_ASSERT_NOT_NULL(url);
    TEST_ASSERT_GREATER_THAN(0, (int)strlen(url));
}

TEST_CASE("test_appstore_fetch_null_entries: NULL entries pointer returns ESP_ERR_INVALID_ARG", "[appstore]")
{
    int count = 0;
    esp_err_t ret = appstore_fetch_catalog("https://example.com/catalog.json",
                                            NULL, 10, &count);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_appstore_fetch_null_url: NULL catalog URL returns error", "[appstore]")
{
    catalog_entry_t entries[1];
    int count = 0;
    esp_err_t ret = appstore_fetch_catalog(NULL, entries, 1, &count);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_appstore_fetch_null_out_count: NULL out_count returns error", "[appstore]")
{
    catalog_entry_t entries[1];
    esp_err_t ret = appstore_fetch_catalog("https://example.com/catalog.json",
                                            entries, 1, NULL);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_appstore_download_null_url: NULL url returns ESP_ERR_INVALID_ARG", "[appstore]")
{
    esp_err_t ret = appstore_download_file(NULL, "/sdcard/apps/test.elf",
                                            NULL, NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_appstore_download_null_path: NULL dest_path returns ESP_ERR_INVALID_ARG", "[appstore]")
{
    esp_err_t ret = appstore_download_file("https://example.com/test.elf", NULL,
                                            NULL, NULL, NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_appstore_install_null_entry: NULL entry returns error", "[appstore]")
{
    esp_err_t ret = appstore_install_entry(NULL, NULL, NULL);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_appstore_catalog_entry_sizes: struct fields fit within defined limits", "[appstore]")
{
    /* Compile-time sanity: APPSTORE_URL_MAX and APPSTORE_HASH_HEX_LEN must fit
     * in the catalog_entry_t fields as declared in the header. */
    catalog_entry_t e;
    memset(&e, 0, sizeof(e));

    TEST_ASSERT_EQUAL_INT(APPSTORE_HASH_HEX_LEN + 1, (int)sizeof(e.sha256_hex));
    TEST_ASSERT_EQUAL_INT(APPSTORE_URL_MAX, (int)sizeof(e.url));
    TEST_ASSERT_EQUAL_INT(APPSTORE_URL_MAX, (int)sizeof(e.sig_url));
}
