/*
 * Minimal C test framework for simulator tests.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once
#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include <math.h>

static int _tests_run = 0;
static int _tests_passed = 0;
static int _tests_failed = 0;

#define TEST(name) void test_##name(void)

#define RUN_TEST(name) do { \
    _tests_run++; \
    printf("  TEST %-50s ", #name); \
    test_##name(); \
    printf("PASS\n"); \
    _tests_passed++; \
} while(0)

#define ASSERT_EQ(a, b) do { \
    if ((a) != (b)) { \
        printf("FAIL\n    %s:%d: %s != %s (%d != %d)\n", __FILE__, __LINE__, #a, #b, (int)(a), (int)(b)); \
        _tests_failed++; \
        return; \
    } \
} while(0)

#define ASSERT_TRUE(x) do { \
    if (!(x)) { \
        printf("FAIL\n    %s:%d: %s is false\n", __FILE__, __LINE__, #x); \
        _tests_failed++; \
        return; \
    } \
} while(0)

#define ASSERT_NEAR(a, b, eps) do { \
    if (fabs((double)(a) - (double)(b)) > (eps)) { \
        printf("FAIL\n    %s:%d: %s != %s (%.4f != %.4f)\n", __FILE__, __LINE__, #a, #b, (double)(a), (double)(b)); \
        _tests_failed++; \
        return; \
    } \
} while(0)

#define ASSERT_STR_CONTAINS(haystack, needle) do { \
    if (strstr((haystack), (needle)) == NULL) { \
        printf("FAIL\n    %s:%d: \"%s\" not found in output\n", __FILE__, __LINE__, (needle)); \
        _tests_failed++; \
        return; \
    } \
} while(0)

#define TEST_SUMMARY() do { \
    printf("\n  %d/%d tests passed", _tests_passed, _tests_run); \
    if (_tests_failed > 0) printf(" (%d FAILED)", _tests_failed); \
    printf("\n"); \
} while(0)
