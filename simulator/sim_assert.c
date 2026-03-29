/*
 * Simulator log assertion framework.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "sim_assert.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

#define MAX_ASSERTIONS 64

typedef struct {
    char pattern[256];
    bool expected;   /* true = must appear (+), false = must NOT appear (-) */
    bool matched;    /* true if pattern was seen in log output */
} sim_assertion_t;

static sim_assertion_t s_assertions[MAX_ASSERTIONS];
static int s_count = 0;
static bool s_initialized = false;

void sim_assert_init(const char *assert_file)
{
    if (!assert_file) return;

    FILE *f = fopen(assert_file, "r");
    if (!f) {
        fprintf(stderr, "sim_assert: cannot open %s\n", assert_file);
        return;
    }

    char line[512];
    while (fgets(line, sizeof(line), f) && s_count < MAX_ASSERTIONS) {
        /* Strip trailing newline */
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
            line[--len] = '\0';

        /* Skip blank lines and comments */
        if (len == 0 || line[0] == '#') continue;

        /* Parse +/- prefix */
        if (line[0] != '+' && line[0] != '-') continue;

        sim_assertion_t *a = &s_assertions[s_count];
        a->expected = (line[0] == '+');
        a->matched = false;

        /* Copy pattern (skip the +/- prefix, trim leading whitespace) */
        const char *p = line + 1;
        while (*p == ' ' || *p == '\t') p++;
        strncpy(a->pattern, p, sizeof(a->pattern) - 1);
        a->pattern[sizeof(a->pattern) - 1] = '\0';

        if (strlen(a->pattern) > 0) {
            s_count++;
        }
    }

    fclose(f);
    s_initialized = true;
    printf("sim_assert: loaded %d assertions from %s\n", s_count, assert_file);
}

void sim_assert_check_line(const char *line)
{
    if (!s_initialized || !line) return;

    for (int i = 0; i < s_count; i++) {
        if (!s_assertions[i].matched && strstr(line, s_assertions[i].pattern)) {
            s_assertions[i].matched = true;
        }
    }
}

int sim_assert_evaluate(void)
{
    if (!s_initialized) return 0;

    int failures = 0;
    printf("\n=== Assertion Results ===\n");

    for (int i = 0; i < s_count; i++) {
        sim_assertion_t *a = &s_assertions[i];
        bool pass;

        if (a->expected) {
            /* +pattern: must have been seen */
            pass = a->matched;
        } else {
            /* -pattern: must NOT have been seen */
            pass = !a->matched;
        }

        printf("  %s %c%s%s\n",
               pass ? "PASS" : "FAIL",
               a->expected ? '+' : '-',
               a->pattern,
               pass ? "" : (a->expected ? " (not found)" : " (found!)"));

        if (!pass) failures++;
    }

    printf("=== %d/%d assertions passed ===\n", s_count - failures, s_count);
    return failures > 0 ? 1 : 0;
}

void sim_assert_reset(void)
{
    s_count = 0;
    s_initialized = false;
    memset(s_assertions, 0, sizeof(s_assertions));
}
