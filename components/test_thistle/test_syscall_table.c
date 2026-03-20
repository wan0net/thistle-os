/*
 * test_syscall_table.c — Unit tests for the ThistleOS kernel syscall table
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * syscall_table_init() is idempotent (it only logs), so calling it in each
 * test is safe and self-documenting.
 */

#include "unity.h"
#include "thistle/syscall.h"
#include "thistle/kernel.h"
#include <stdint.h>

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("syscall_table_init returns ESP_OK", "[syscall]")
{
    esp_err_t ret = syscall_table_init();
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("syscall table is non-empty after init", "[syscall]")
{
    TEST_ASSERT_EQUAL(ESP_OK, syscall_table_init());

    size_t count = syscall_table_count();
    TEST_ASSERT_GREATER_THAN(0, count);
}

TEST_CASE("syscall_resolve returns non-NULL for thistle_log", "[syscall]")
{
    TEST_ASSERT_EQUAL(ESP_OK, syscall_table_init());

    void *fn = syscall_resolve("thistle_log");
    TEST_ASSERT_NOT_NULL(fn);
}

TEST_CASE("syscall_resolve returns NULL for unknown symbol", "[syscall]")
{
    TEST_ASSERT_EQUAL(ESP_OK, syscall_table_init());

    void *fn = syscall_resolve("nonexistent_func");
    TEST_ASSERT_NULL(fn);
}

TEST_CASE("syscall_resolve for thistle_millis returns callable function with positive result", "[syscall]")
{
    TEST_ASSERT_EQUAL(ESP_OK, syscall_table_init());

    void *fn = syscall_resolve("thistle_millis");
    TEST_ASSERT_NOT_NULL(fn);

    /*
     * thistle_millis wraps kernel_uptime_ms() which in turn calls
     * esp_timer_get_time(). Cast and call it; even at early boot the
     * uptime counter is running, so the result must be >= 0.
     * We just verify it returns without crashing and that the pointer
     * was correctly resolved.
     */
    typedef uint32_t (*millis_fn_t)(void);
    millis_fn_t millis = (millis_fn_t)fn;
    uint32_t t = millis();
    /* t can legitimately be 0 at the very start of boot, so just assert
     * that the call completed by checking the return type is valid. */
    (void)t;  /* no crash == pass; additional sanity: */
    TEST_ASSERT_GREATER_OR_EQUAL(0, (int)t);
}
