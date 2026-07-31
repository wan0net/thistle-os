// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "assistant_request.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>

#define HISTORY_COUNT 6U
#define MAX_MESSAGE_TEXT 511U

static void fill_hostile_maximum(char *text, size_t length)
{
    static const char pattern[] = {'A', '"', '\\', '\n', '\r', '\t', '\x01'};
    for (size_t index = 0; index < length; index++) {
        text[index] = pattern[index % sizeof(pattern)];
    }
    text[length] = '\0';
}

static void test_maximum_history_is_safely_serialized(void)
{
    char content[MAX_MESSAGE_TEXT + 1U];
    char system_prompt[256];
    assistant_request_message_t history[HISTORY_COUNT];
    fill_hostile_maximum(content, MAX_MESSAGE_TEXT);
    fill_hostile_maximum(system_prompt, sizeof(system_prompt) - 1U);
    for (size_t index = 0; index < HISTORY_COUNT; index++) {
        history[index].role = index % 2U == 0 ? "user" : "assistant";
        history[index].content = content;
    }

    size_t length = 0;
    char *body = assistant_request_build(
        "model-with-\"quote", system_prompt, history, HISTORY_COUNT,
        content, &length);
    assert(body != NULL);
    assert(length == strlen(body));
    assert(length > 2048U);
    assert(strstr(body, "\\\"") != NULL);
    assert(strstr(body, "\\\\") != NULL);
    assert(strstr(body, "\\u0001") != NULL);
    assert(strcmp(body + length - 3U, "}]}") == 0);
    free(body);
}

static void test_invalid_inputs_fail_closed(void)
{
    size_t length = 123U;
    assert(assistant_request_build(NULL, "system", NULL, 0, "message",
                                   &length) == NULL);
    assert(length == 0U);
    assert(assistant_request_build("model", "system", NULL, 1, "message",
                                   &length) == NULL);
    assert(assistant_request_build("model", "system", NULL, 0, "message",
                                   NULL) == NULL);
}

int main(void)
{
    test_maximum_history_is_safely_serialized();
    test_invalid_inputs_fail_closed();
    return 0;
}
