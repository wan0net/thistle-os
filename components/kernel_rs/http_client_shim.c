// SPDX-License-Identifier: BSD-3-Clause
// Typed C boundary for ESP-IDF's evolving HTTP client structs.

#include "thistle_http_shim.h"

#include <stddef.h>
#include <stdlib.h>

#include "esp_err.h"
#include "esp_http_client.h"

typedef struct {
    esp_http_client_handle_t inner;
    thistle_http_data_cb_t data_cb;
    void *user_data;
} thistle_http_client_t;

#ifdef ESP_PLATFORM
_Static_assert(sizeof(((esp_http_client_config_t *)0)->timeout_ms) == sizeof(int),
               "ESP-IDF HTTP timeout ABI changed");
_Static_assert(sizeof(((esp_http_client_event_t *)0)->data_len) == sizeof(int),
               "ESP-IDF HTTP event length ABI changed");
_Static_assert(offsetof(esp_http_client_event_t, client) <
                   offsetof(esp_http_client_event_t, data),
               "ESP-IDF HTTP event client/data order changed");
_Static_assert(offsetof(esp_http_client_event_t, data) <
                   offsetof(esp_http_client_event_t, data_len),
               "ESP-IDF HTTP event data/length order changed");
#endif

#ifdef ESP_PLATFORM
static esp_err_t thistle_http_event_handler(esp_http_client_event_t *event)
{
    thistle_http_client_t *client = event ? event->user_data : NULL;
    if (!client || !client->data_cb || event->event_id != HTTP_EVENT_ON_DATA) {
        return ESP_OK;
    }
    return client->data_cb((const uint8_t *)event->data,
                           event->data_len,
                           client->user_data);
}
#endif

void *thistle_http_client_init(const char *url,
                               int timeout_ms,
                               thistle_http_data_cb_t data_cb,
                               void *user_data)
{
    if (!url) {
        return NULL;
    }

    thistle_http_client_t *client = calloc(1, sizeof(*client));
    if (!client) {
        return NULL;
    }
    client->data_cb = data_cb;
    client->user_data = user_data;

    const esp_http_client_config_t config = {
        .url = url,
#ifdef ESP_PLATFORM
        .event_handler = thistle_http_event_handler,
#endif
        .user_data = client,
        .timeout_ms = timeout_ms,
    };
    client->inner = esp_http_client_init(&config);
    if (!client->inner) {
        free(client);
        return NULL;
    }
    return client;
}

static thistle_http_client_t *checked_client(void *opaque)
{
    return (thistle_http_client_t *)opaque;
}

int thistle_http_client_perform(void *opaque)
{
    thistle_http_client_t *client = checked_client(opaque);
    if (!client) {
        return ESP_ERR_INVALID_ARG;
    }
    int result = esp_http_client_perform(client->inner);
#ifndef ESP_PLATFORM
    if (result == ESP_OK && client->data_cb) {
        const char *data = sim_http_client_get_response_data(client->inner);
        size_t length = sim_http_client_get_response_length(client->inner);
        if (data && length > 0) {
            result = client->data_cb((const uint8_t *)data,
                                     (int)length,
                                     client->user_data);
        }
    }
#endif
    return result;
}

int thistle_http_client_get_status_code(void *opaque)
{
    thistle_http_client_t *client = checked_client(opaque);
    return client ? esp_http_client_get_status_code(client->inner) : 0;
}

int thistle_http_client_open(void *opaque, int write_len)
{
    thistle_http_client_t *client = checked_client(opaque);
    return client ? esp_http_client_open(client->inner, write_len) : ESP_ERR_INVALID_ARG;
}

int64_t thistle_http_client_fetch_headers(void *opaque)
{
    thistle_http_client_t *client = checked_client(opaque);
    return client ? esp_http_client_fetch_headers(client->inner) : -1;
}

int thistle_http_client_read(void *opaque, char *buffer, int length)
{
    thistle_http_client_t *client = checked_client(opaque);
    return client ? esp_http_client_read(client->inner, buffer, length) : -1;
}

int thistle_http_client_close(void *opaque)
{
    thistle_http_client_t *client = checked_client(opaque);
    return client ? esp_http_client_close(client->inner) : ESP_ERR_INVALID_ARG;
}

int thistle_http_client_cleanup(void *opaque)
{
    thistle_http_client_t *client = checked_client(opaque);
    if (!client) {
        return ESP_ERR_INVALID_ARG;
    }
#ifdef ESP_PLATFORM
    int result = esp_http_client_cleanup(client->inner);
#else
    esp_http_client_cleanup(client->inner);
    int result = ESP_OK;
#endif
    free(client);
    return result;
}
