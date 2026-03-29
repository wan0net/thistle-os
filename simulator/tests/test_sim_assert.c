/*
 * Unit tests for sim_assert log assertion framework.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "test_runner.h"
#include <stdio.h>
#include <stdlib.h>

/* Declared in sim_assert.h */
extern void sim_assert_init(const char *path);
extern void sim_assert_check_line(const char *line);
extern int  sim_assert_evaluate(void);
extern void sim_assert_reset(void);

static void write_temp_assertions(const char *path, const char *content) {
    FILE *f = fopen(path, "w");
    fprintf(f, "%s", content);
    fclose(f);
}

TEST(assert_uninitialized_returns_zero) {
    sim_assert_reset();
    ASSERT_EQ(sim_assert_evaluate(), 0);
}

TEST(assert_positive_match_passes) {
    sim_assert_reset();
    write_temp_assertions("/tmp/test_assert.txt",
        "+hello world\n"
        "-bad thing\n"
    );
    sim_assert_init("/tmp/test_assert.txt");
    sim_assert_check_line("hello world from kernel");
    ASSERT_EQ(sim_assert_evaluate(), 0);
}

TEST(assert_positive_not_found_fails) {
    sim_assert_reset();
    write_temp_assertions("/tmp/test_assert2.txt",
        "+must see this\n"
    );
    sim_assert_init("/tmp/test_assert2.txt");
    /* Don't check any lines */
    ASSERT_EQ(sim_assert_evaluate(), 1);
}

TEST(assert_negative_found_fails) {
    sim_assert_reset();
    write_temp_assertions("/tmp/test_assert3.txt",
        "-PANIC\n"
    );
    sim_assert_init("/tmp/test_assert3.txt");
    sim_assert_check_line("kernel PANIC detected");
    ASSERT_EQ(sim_assert_evaluate(), 1);
}
