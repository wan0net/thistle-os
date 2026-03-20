/*
 * test_permissions.c — Unit tests for the ThistleOS permissions subsystem
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls permissions_init() to reset the static app table, making
 * tests fully self-contained despite the global state.
 */

#include "unity.h"
#include "thistle/permissions.h"
#include <string.h>

/* --------------------------------------------------------------------------
 * test_permissions_init
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_init: returns ESP_OK", "[permissions]")
{
    esp_err_t ret = permissions_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

/* --------------------------------------------------------------------------
 * test_permissions_grant_and_check
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_grant_and_check: granted permission is accessible", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    TEST_ASSERT_EQUAL(ESP_OK, permissions_grant("radio_app", PERM_RADIO));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("radio_app", PERM_RADIO));
}

/* --------------------------------------------------------------------------
 * test_permissions_check_denied
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_check_denied: ungranted permission is denied", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    /* Grant only GPS; RADIO must be denied */
    TEST_ASSERT_EQUAL(ESP_OK, permissions_grant("gps_only_app", PERM_GPS));
    TEST_ASSERT_EQUAL(ESP_ERR_NOT_ALLOWED, permissions_check("gps_only_app", PERM_RADIO));
}

/* --------------------------------------------------------------------------
 * test_permissions_revoke
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_revoke: revoked permission denied but others remain", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    /* Grant everything, then revoke RADIO */
    TEST_ASSERT_EQUAL(ESP_OK, permissions_grant("full_app", PERM_ALL));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_revoke("full_app", PERM_RADIO));

    /* RADIO must now be denied */
    TEST_ASSERT_EQUAL(ESP_ERR_NOT_ALLOWED, permissions_check("full_app", PERM_RADIO));

    /* All other flags in PERM_ALL should still be granted */
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_GPS));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_STORAGE));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_NETWORK));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_AUDIO));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_SYSTEM));
    TEST_ASSERT_EQUAL(ESP_OK, permissions_check("full_app", PERM_IPC));
}

/* --------------------------------------------------------------------------
 * test_permissions_get
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_get: returns correct bitmask after grant", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    permission_set_t expected = PERM_RADIO | PERM_GPS | PERM_STORAGE;
    TEST_ASSERT_EQUAL(ESP_OK, permissions_grant("multi_app", expected));

    permission_set_t got = permissions_get("multi_app");
    TEST_ASSERT_EQUAL_UINT32(expected, got);
}

/* --------------------------------------------------------------------------
 * test_permissions_parse
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_parse: each name maps to the correct flag", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_RADIO,   (uint32_t)permissions_parse("radio"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_GPS,     (uint32_t)permissions_parse("gps"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_STORAGE, (uint32_t)permissions_parse("storage"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_NETWORK, (uint32_t)permissions_parse("network"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_AUDIO,   (uint32_t)permissions_parse("audio"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_SYSTEM,  (uint32_t)permissions_parse("system"));
    TEST_ASSERT_EQUAL_UINT32((uint32_t)PERM_IPC,     (uint32_t)permissions_parse("ipc"));

    /* Unknown names must return 0 */
    TEST_ASSERT_EQUAL_UINT32(0, (uint32_t)permissions_parse("unknown"));
    TEST_ASSERT_EQUAL_UINT32(0, (uint32_t)permissions_parse(""));
    TEST_ASSERT_EQUAL_UINT32(0, (uint32_t)permissions_parse(NULL));
}

/* --------------------------------------------------------------------------
 * test_permissions_to_string
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_to_string: output contains expected names", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    char buf[128];
    permissions_to_string(PERM_RADIO | PERM_GPS, buf, sizeof(buf));

    /* Both names must appear somewhere in the output */
    TEST_ASSERT_NOT_NULL(strstr(buf, "radio"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "gps"));

    /* A flag not in the set must NOT appear */
    TEST_ASSERT_NULL(strstr(buf, "storage"));
}

/* --------------------------------------------------------------------------
 * test_permissions_unknown_app
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_unknown_app: check returns ESP_ERR_NOT_FOUND", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    /* "ghost_app" was never registered */
    esp_err_t ret = permissions_check("ghost_app", PERM_RADIO);
    TEST_ASSERT_EQUAL(ESP_ERR_NOT_FOUND, ret);
}

/* --------------------------------------------------------------------------
 * test_permissions_max_apps
 * -------------------------------------------------------------------------- */

TEST_CASE("test_permissions_max_apps: 17th app registration returns ESP_ERR_NO_MEM", "[permissions]")
{
    TEST_ASSERT_EQUAL(ESP_OK, permissions_init());

    /* Register exactly MAX_APPS (16) unique apps */
    char app_id[32];
    for (int i = 0; i < 16; i++) {
        snprintf(app_id, sizeof(app_id), "app_%02d", i);
        esp_err_t r = permissions_grant(app_id, PERM_RADIO);
        TEST_ASSERT_EQUAL_MESSAGE(ESP_OK, r, "Expected ESP_OK for slot within MAX_APPS");
    }

    /* The 17th distinct app must fail — the slot table is full */
    esp_err_t ret = permissions_grant("app_overflow", PERM_RADIO);
    TEST_ASSERT_EQUAL(ESP_ERR_NO_MEM, ret);
}
