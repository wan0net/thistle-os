#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef int (*thistle_http_data_cb_t)(const uint8_t *data,
                                      int data_len,
                                      void *user_data);

void *thistle_http_client_init(const char *url,
                               int timeout_ms,
                               thistle_http_data_cb_t data_cb,
                               void *user_data);
int thistle_http_client_perform(void *client);
int thistle_http_client_get_status_code(void *client);
int thistle_http_client_open(void *client, int write_len);
int64_t thistle_http_client_fetch_headers(void *client);
int thistle_http_client_read(void *client, char *buffer, int length);
int thistle_http_client_close(void *client);
int thistle_http_client_cleanup(void *client);

#ifdef __cplusplus
}
#endif
