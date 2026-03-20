/*
 * test_wifi_manager.c — Unit tests for the ThistleOS WiFi manager API surface
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * WiFi cannot actually connect in the test environment, so these tests cover
 * observable API contracts that do not require hardware: initial state,
 * formatted time/date strings, and null-safety of getters when disconnected.
 *
 * wifi_manager_init() is intentionally NOT called in these tests because it
 * invokes esp_wifi_init() and related IDF stack functions that would fault
 * without real NVS / RF hardware. The state variable is initialised to
 * WIFI_STATE_DISCONNECTED at file scope (see wifi_manager.c), so pre-init
 * contract checks are valid.
 */

#include "unity.h"
#include "thistle/wifi_manager.h"
#include <string.h>
#include <ctype.h>

/* --------------------------------------------------------------------------
 * Helpers
 * -------------------------------------------------------------------------- */

/*
 * Returns true if buf is exactly "--:--" (the placeholder the implementation
 * writes when the system clock has not been set via NTP).
 */
static bool is_placeholder_time(const char *buf)
{
    return strcmp(buf, "--:--") == 0;
}

/*
 * Returns true if buf matches the HH:MM pattern (digits and colon only,
 * hours 00-23, minutes 00-59).
 */
static bool is_valid_time_str(const char *buf)
{
    if (strlen(buf) != 5) return false;
    if (!isdigit((unsigned char)buf[0])) return false;
    if (!isdigit((unsigned char)buf[1])) return false;
    if (buf[2] != ':') return false;
    if (!isdigit((unsigned char)buf[3])) return false;
    if (!isdigit((unsigned char)buf[4])) return false;
    int hour = (buf[0] - '0') * 10 + (buf[1] - '0');
    int min  = (buf[3] - '0') * 10 + (buf[4] - '0');
    return (hour >= 0 && hour <= 23 && min >= 0 && min <= 59);
}

/*
 * Returns true if buf is the placeholder "----/--/--" used before NTP sync,
 * or a valid YYYY-MM-DD date string.
 */
static bool is_placeholder_date(const char *buf)
{
    return strcmp(buf, "----/--/--") == 0;
}

static bool is_valid_date_str(const char *buf)
{
    /* Expected format: "YYYY-MM-DD" */
    if (strlen(buf) != 10) return false;
    for (int i = 0; i < 4; i++) {
        if (!isdigit((unsigned char)buf[i])) return false;
    }
    if (buf[4] != '-') return false;
    if (!isdigit((unsigned char)buf[5])) return false;
    if (!isdigit((unsigned char)buf[6])) return false;
    if (buf[7] != '-') return false;
    if (!isdigit((unsigned char)buf[8])) return false;
    if (!isdigit((unsigned char)buf[9])) return false;
    return true;
}

/* --------------------------------------------------------------------------
 * test_wifi_get_state_initial
 * -------------------------------------------------------------------------- */

TEST_CASE("test_wifi_get_state_initial: state is DISCONNECTED before init", "[wifi]")
{
    /*
     * The static variable s_state in wifi_manager.c is initialised to
     * WIFI_STATE_DISCONNECTED at compile time. Without calling
     * wifi_manager_init() (which requires real hardware), the getter must
     * still return a valid enum value — DISCONNECTED is the only safe one.
     */
    wifi_state_t state = wifi_manager_get_state();
    TEST_ASSERT_EQUAL(WIFI_STATE_DISCONNECTED, state);
}

/* --------------------------------------------------------------------------
 * test_wifi_get_time_str
 * -------------------------------------------------------------------------- */

TEST_CASE("test_wifi_get_time_str: returns placeholder or valid HH:MM string", "[wifi]")
{
    char buf[16] = {0};
    wifi_manager_get_time_str(buf, sizeof(buf));

    /*
     * In the test environment the system clock is either unset (year < 2024
     * epoch threshold used by the implementation) or the host clock is valid.
     * Both outcomes are acceptable; only a malformed string is a failure.
     */
    bool ok = is_placeholder_time(buf) || is_valid_time_str(buf);
    TEST_ASSERT_TRUE_MESSAGE(ok, "Time string is neither '--:--' nor a valid HH:MM value");
}

/* --------------------------------------------------------------------------
 * test_wifi_get_date_str
 * -------------------------------------------------------------------------- */

TEST_CASE("test_wifi_get_date_str: returns placeholder or valid YYYY-MM-DD string", "[wifi]")
{
    char buf[16] = {0};
    wifi_manager_get_date_str(buf, sizeof(buf));

    bool ok = is_placeholder_date(buf) || is_valid_date_str(buf);
    TEST_ASSERT_TRUE_MESSAGE(ok, "Date string is neither '----/--/--' nor a valid YYYY-MM-DD value");
}
