/*
 * Simulator log assertion framework.
 * Checks log output against expected/forbidden patterns.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

/*
 * Load assertions from a file. Format:
 *   +pattern   — pattern MUST appear in log output
 *   -pattern   — pattern must NOT appear in log output
 *   # comment  — ignored
 *   blank lines — ignored
 */
void sim_assert_init(const char *assert_file);

/* Check a single line of log output against all assertions. */
void sim_assert_check_line(const char *line);

/* Evaluate all assertions. Returns 0 if all pass, 1 if any fail. Prints summary. */
int sim_assert_evaluate(void);

/* Reset all assertion state (for unit tests) */
void sim_assert_reset(void);
