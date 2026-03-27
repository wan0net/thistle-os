// SPDX-License-Identifier: BSD-3-Clause
// Simulator stub — cert bundle not needed (libcurl/browser handles HTTPS)
#pragma once

#include "esp_err.h"

static inline esp_err_t esp_crt_bundle_attach(void *conf) {
    (void)conf;
    return ESP_OK;
}
