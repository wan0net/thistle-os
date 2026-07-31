// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include <stddef.h>

typedef struct {
    const char *role;
    const char *content;
} assistant_request_message_t;

/* Build one Messages API request as an exactly sized heap allocation.
 * Returns NULL on invalid input, size overflow, or allocation failure. */
char *assistant_request_build(const char *model,
                              const char *system_prompt,
                              const assistant_request_message_t *history,
                              size_t history_count,
                              const char *user_message,
                              size_t *out_length);
