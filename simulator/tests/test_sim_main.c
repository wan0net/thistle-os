/*
 * Simulator unit test runner.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "test_runner.h"

/* Forward declarations from test files */
/* test_sim_assert.c */
extern void test_assert_uninitialized_returns_zero(void);
extern void test_assert_positive_match_passes(void);
extern void test_assert_positive_not_found_fails(void);
extern void test_assert_negative_found_fails(void);

/* test_i2c_bus.c */
extern void test_i2c_bus_init_succeeds(void);
extern void test_i2c_bus_get_out_of_range_returns_null(void);
extern void test_i2c_add_device_returns_handle(void);
extern void test_i2c_unregistered_address_returns_sentinel(void);
extern void test_pcf8563_who_am_i(void);
extern void test_pcf8563_reads_time(void);
extern void test_qmi8658c_who_am_i(void);
extern void test_qmi8658c_reads_accel_data(void);
extern void test_tca8418_empty_fifo(void);
extern void test_tca8418_key_injection(void);
extern void test_cst328_no_touch(void);
extern void test_cst328_touch_injection(void);

int main(void)
{
    printf("=== Simulator Unit Tests ===\n\n");

    printf("[sim_assert]\n");
    RUN_TEST(assert_uninitialized_returns_zero);
    RUN_TEST(assert_positive_match_passes);
    RUN_TEST(assert_positive_not_found_fails);
    RUN_TEST(assert_negative_found_fails);

    printf("\n[i2c_bus]\n");
    RUN_TEST(i2c_bus_init_succeeds);
    RUN_TEST(i2c_bus_get_out_of_range_returns_null);
    RUN_TEST(i2c_add_device_returns_handle);
    RUN_TEST(i2c_unregistered_address_returns_sentinel);

    printf("\n[pcf8563]\n");
    RUN_TEST(pcf8563_who_am_i);
    RUN_TEST(pcf8563_reads_time);

    printf("\n[qmi8658c]\n");
    RUN_TEST(qmi8658c_who_am_i);
    RUN_TEST(qmi8658c_reads_accel_data);

    printf("\n[tca8418]\n");
    RUN_TEST(tca8418_empty_fifo);
    RUN_TEST(tca8418_key_injection);

    printf("\n[cst328]\n");
    RUN_TEST(cst328_no_touch);
    RUN_TEST(cst328_touch_injection);

    TEST_SUMMARY();
    return _tests_failed > 0 ? 1 : 0;
}
