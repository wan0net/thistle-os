// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "assistant_request.h"

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *buffer;
    size_t capacity;
    size_t length;
    bool valid;
} json_writer_t;

static bool writer_reserve(json_writer_t *writer, size_t additional)
{
    if (!writer->valid || additional > SIZE_MAX - writer->length - 1U) {
        writer->valid = false;
        return false;
    }
    if (writer->buffer && writer->length + additional >= writer->capacity) {
        writer->valid = false;
        return false;
    }
    return true;
}

static bool writer_append_bytes(json_writer_t *writer,
                                const char *bytes,
                                size_t length)
{
    if (!writer_reserve(writer, length)) {
        return false;
    }
    if (writer->buffer && length > 0) {
        memcpy(writer->buffer + writer->length, bytes, length);
    }
    writer->length += length;
    return true;
}

static bool writer_append_literal(json_writer_t *writer, const char *literal)
{
    return writer_append_bytes(writer, literal, strlen(literal));
}

static bool writer_append_char(json_writer_t *writer, char value)
{
    return writer_append_bytes(writer, &value, 1U);
}

static bool writer_append_json_string(json_writer_t *writer, const char *value)
{
    static const char hex[] = "0123456789abcdef";
    if (!value || !writer_append_char(writer, '"')) {
        return false;
    }

    for (const unsigned char *cursor = (const unsigned char *)value;
         *cursor != '\0'; cursor++) {
        const char *escape = NULL;
        switch (*cursor) {
            case '"': escape = "\\\""; break;
            case '\\': escape = "\\\\"; break;
            case '\b': escape = "\\b"; break;
            case '\f': escape = "\\f"; break;
            case '\n': escape = "\\n"; break;
            case '\r': escape = "\\r"; break;
            case '\t': escape = "\\t"; break;
            default: break;
        }
        if (escape) {
            if (!writer_append_bytes(writer, escape, 2U)) {
                return false;
            }
        } else if (*cursor < 0x20U) {
            char unicode_escape[6] = {
                '\\', 'u', '0', '0', hex[*cursor >> 4], hex[*cursor & 0x0f]
            };
            if (!writer_append_bytes(writer, unicode_escape,
                                     sizeof(unicode_escape))) {
                return false;
            }
        } else if (!writer_append_char(writer, (char)*cursor)) {
            return false;
        }
    }
    return writer_append_char(writer, '"');
}

static bool write_request(json_writer_t *writer,
                          const char *model,
                          const char *system_prompt,
                          const assistant_request_message_t *history,
                          size_t history_count,
                          const char *user_message)
{
    if (!model || !system_prompt || !user_message
        || (history_count > 0 && !history)) {
        return false;
    }

    if (!writer_append_literal(writer, "{\"model\":")
        || !writer_append_json_string(writer, model)
        || !writer_append_literal(writer,
            ",\"max_tokens\":512,\"system\":")
        || !writer_append_json_string(writer, system_prompt)
        || !writer_append_literal(writer, ",\"messages\":[")) {
        return false;
    }

    for (size_t index = 0; index < history_count; index++) {
        if (!history[index].role || !history[index].content
            || (index > 0 && !writer_append_char(writer, ','))
            || !writer_append_literal(writer, "{\"role\":")
            || !writer_append_json_string(writer, history[index].role)
            || !writer_append_literal(writer, ",\"content\":")
            || !writer_append_json_string(writer, history[index].content)
            || !writer_append_char(writer, '}')) {
            return false;
        }
    }

    if ((history_count > 0 && !writer_append_char(writer, ','))
        || !writer_append_literal(writer, "{\"role\":\"user\",\"content\":")
        || !writer_append_json_string(writer, user_message)
        || !writer_append_literal(writer, "}]}")) {
        return false;
    }
    return writer->valid;
}

char *assistant_request_build(const char *model,
                              const char *system_prompt,
                              const assistant_request_message_t *history,
                              size_t history_count,
                              const char *user_message,
                              size_t *out_length)
{
    if (!out_length) {
        return NULL;
    }
    *out_length = 0;

    json_writer_t measure = {
        .buffer = NULL,
        .capacity = 0,
        .length = 0,
        .valid = true,
    };
    if (!write_request(&measure, model, system_prompt, history, history_count,
                       user_message)
        || measure.length == SIZE_MAX) {
        return NULL;
    }

    char *body = malloc(measure.length + 1U);
    if (!body) {
        return NULL;
    }
    json_writer_t writer = {
        .buffer = body,
        .capacity = measure.length + 1U,
        .length = 0,
        .valid = true,
    };
    if (!write_request(&writer, model, system_prompt, history, history_count,
                       user_message)
        || writer.length != measure.length) {
        free(body);
        return NULL;
    }
    body[writer.length] = '\0';
    *out_length = writer.length;
    return body;
}
