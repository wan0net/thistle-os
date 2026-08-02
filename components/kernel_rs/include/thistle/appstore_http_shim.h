// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct thistle_appstore_http_client thistle_appstore_http_client_t;

typedef int32_t (*thistle_appstore_http_data_cb_t)(const uint8_t *data,
                                                   size_t data_len,
                                                   void *user_data);

thistle_appstore_http_client_t *thistle_appstore_http_init(
    const char *url, int32_t timeout_ms,
    thistle_appstore_http_data_cb_t data_cb, void *user_data);
int32_t thistle_appstore_http_perform(thistle_appstore_http_client_t *client);
int32_t thistle_appstore_http_status(thistle_appstore_http_client_t *client);
int32_t thistle_appstore_http_open(thistle_appstore_http_client_t *client,
                                   int32_t write_len);
int64_t thistle_appstore_http_fetch_headers(thistle_appstore_http_client_t *client);
int32_t thistle_appstore_http_read(thistle_appstore_http_client_t *client,
                                   uint8_t *buf, int32_t len);
int32_t thistle_appstore_http_close(thistle_appstore_http_client_t *client);
void thistle_appstore_http_cleanup(thistle_appstore_http_client_t *client);

/* Shared by the ESP-IDF event adapter and hardware-independent fixture tests. */
int32_t thistle_appstore_http_dispatch_data(
    thistle_appstore_http_data_cb_t data_cb, void *user_data,
    const uint8_t *data, size_t data_len);

#ifdef __cplusplus
}
#endif
