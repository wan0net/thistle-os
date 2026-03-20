#pragma once

/* Simulator shim — maps esp_http_client API to sim_http using libcurl.
 *
 * This header is picked up by the simulator build BEFORE any ESP-IDF
 * headers (the platform/ directory is first in the include path), so
 * appstore_client.c compiles without any #ifdef guards.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_http.h"

typedef sim_http_client_handle_t esp_http_client_handle_t;

typedef struct {
    const char *url;
    void       *event_handler;  /* ignored in simulator */
    void       *user_data;      /* ignored in simulator */
    int         timeout_ms;
} esp_http_client_config_t;

typedef enum {
    HTTP_EVENT_ON_DATA = 0,
} esp_http_client_event_id_t;

typedef struct {
    esp_http_client_event_id_t event_id;
    void *data;
    int   data_len;
    void *user_data;
} esp_http_client_event_t;

static inline esp_http_client_handle_t esp_http_client_init(const esp_http_client_config_t *config)
{
    sim_http_client_config_t sc = {
        .url        = config->url,
        .timeout_ms = config->timeout_ms,
    };
    return sim_http_client_init(&sc);
}

static inline esp_err_t esp_http_client_perform(esp_http_client_handle_t c) {
    return sim_http_client_perform(c);
}

static inline int esp_http_client_get_status_code(esp_http_client_handle_t c) {
    return sim_http_client_get_status_code(c);
}

static inline esp_err_t esp_http_client_open(esp_http_client_handle_t c, int len) {
    return sim_http_client_open(c, len);
}

static inline int esp_http_client_fetch_headers(esp_http_client_handle_t c) {
    return sim_http_client_fetch_headers(c);
}

static inline int esp_http_client_read(esp_http_client_handle_t c, char *buf, int len) {
    return sim_http_client_read(c, buf, len);
}

static inline esp_err_t esp_http_client_close(esp_http_client_handle_t c) {
    return sim_http_client_close(c);
}

static inline void esp_http_client_cleanup(esp_http_client_handle_t c) {
    sim_http_client_cleanup(c);
}
