// SPDX-License-Identifier: BSD-3-Clause
// ABI-safe adapter between Rust app-store code and ESP-IDF HTTP client.

#include "thistle/appstore_http_shim.h"

#include <stdlib.h>

#include "esp_err.h"
#include "esp_http_client.h"

#ifdef SIMULATOR_BUILD
#include "sim_http.h"
#endif

_Static_assert(sizeof(esp_err_t) == sizeof(int32_t),
               "esp_err_t must fit the stable Rust shim ABI");
_Static_assert(sizeof(((esp_http_client_event_t *)0)->data_len) == sizeof(int),
               "ESP-IDF HTTP event data_len must be an int");

struct thistle_appstore_http_client {
    esp_http_client_handle_t handle;
    thistle_appstore_http_data_cb_t data_cb;
    void *user_data;
};

int32_t thistle_appstore_http_dispatch_data(
    thistle_appstore_http_data_cb_t data_cb, void *user_data,
    const uint8_t *data, size_t data_len)
{
    if (!data_cb || (!data && data_len != 0)) {
        return ESP_ERR_INVALID_ARG;
    }
    if (data_len == 0) {
        return ESP_OK;
    }
    return data_cb(data, data_len, user_data);
}

static esp_err_t http_event_handler(esp_http_client_event_t *event)
{
    thistle_appstore_http_client_t *client = event->user_data;
    if (event->event_id != HTTP_EVENT_ON_DATA || !client || !client->data_cb) {
        return ESP_OK;
    }
    return thistle_appstore_http_dispatch_data(
        client->data_cb, client->user_data, event->data,
        (size_t)event->data_len);
}

thistle_appstore_http_client_t *thistle_appstore_http_init(
    const char *url, int32_t timeout_ms,
    thistle_appstore_http_data_cb_t data_cb, void *user_data)
{
    if (!url) {
        return NULL;
    }

    thistle_appstore_http_client_t *client = calloc(1, sizeof(*client));
    if (!client) {
        return NULL;
    }
    client->data_cb = data_cb;
    client->user_data = user_data;

    esp_http_client_config_t config = {
        .url = url,
        .event_handler = http_event_handler,
        .user_data = client,
        .timeout_ms = timeout_ms,
    };
    client->handle = esp_http_client_init(&config);
    if (!client->handle) {
        free(client);
        return NULL;
    }
    return client;
}

int32_t thistle_appstore_http_perform(thistle_appstore_http_client_t *client)
{
    if (!client || !client->handle) {
        return ESP_ERR_INVALID_ARG;
    }
    esp_err_t ret = esp_http_client_perform(client->handle);
#ifdef SIMULATOR_BUILD
    /* The simulator backend buffers responses and does not emit IDF events. */
    if (ret == ESP_OK && client->data_cb) {
        const char *data = sim_http_client_get_response_data(client->handle);
        size_t len = sim_http_client_get_response_length(client->handle);
        ret = thistle_appstore_http_dispatch_data(
            client->data_cb, client->user_data, (const uint8_t *)data, len);
    }
#endif
    return ret;
}

int32_t thistle_appstore_http_status(thistle_appstore_http_client_t *client)
{
    if (!client || !client->handle) {
        return 0;
    }
    return esp_http_client_get_status_code(client->handle);
}

int32_t thistle_appstore_http_open(thistle_appstore_http_client_t *client,
                                   int32_t write_len)
{
    if (!client || !client->handle) {
        return ESP_ERR_INVALID_ARG;
    }
    return esp_http_client_open(client->handle, write_len);
}

int64_t thistle_appstore_http_fetch_headers(thistle_appstore_http_client_t *client)
{
    if (!client || !client->handle) {
        return -1;
    }
    return esp_http_client_fetch_headers(client->handle);
}

int32_t thistle_appstore_http_read(thistle_appstore_http_client_t *client,
                                   uint8_t *buf, int32_t len)
{
    if (!client || !client->handle || !buf || len < 0) {
        return ESP_ERR_INVALID_ARG;
    }
    return esp_http_client_read(client->handle, (char *)buf, len);
}

int32_t thistle_appstore_http_close(thistle_appstore_http_client_t *client)
{
    if (!client || !client->handle) {
        return ESP_ERR_INVALID_ARG;
    }
    return esp_http_client_close(client->handle);
}

void thistle_appstore_http_cleanup(thistle_appstore_http_client_t *client)
{
    if (!client) {
        return;
    }
    if (client->handle) {
        esp_http_client_cleanup(client->handle);
    }
    free(client);
}
