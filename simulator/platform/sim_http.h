#pragma once

/* Simulator HTTP client — wraps libcurl for host-side HTTP requests.
 * Provides the same interface as esp_http_client for code compatibility.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "esp_err.h"
#include <stddef.h>

typedef struct sim_http_client *sim_http_client_handle_t;

typedef struct {
    const char *url;
    int timeout_ms;
} sim_http_client_config_t;

sim_http_client_handle_t sim_http_client_init(const sim_http_client_config_t *config);
esp_err_t sim_http_client_perform(sim_http_client_handle_t client);
int sim_http_client_get_status_code(sim_http_client_handle_t client);
int sim_http_client_get_content_length(sim_http_client_handle_t client);
const char *sim_http_client_get_response_data(sim_http_client_handle_t client);
size_t sim_http_client_get_response_length(sim_http_client_handle_t client);
esp_err_t sim_http_client_open(sim_http_client_handle_t client, int write_len);
int sim_http_client_fetch_headers(sim_http_client_handle_t client);
int sim_http_client_read(sim_http_client_handle_t client, char *buf, int len);
esp_err_t sim_http_client_close(sim_http_client_handle_t client);
void sim_http_client_cleanup(sim_http_client_handle_t client);
