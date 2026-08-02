// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef esp_err_t (*board_bus_register_fn_t)(int bus_id, void *resource);
typedef esp_err_t (*board_bus_cleanup_fn_t)(void *resource);

/**
 * Register a newly prepared bus resource and release it if registration fails.
 * The registration error is returned unchanged even if cleanup also fails.
 */
esp_err_t board_bus_register_resource(int bus_id, void *resource,
                                      board_bus_register_fn_t register_fn,
                                      board_bus_cleanup_fn_t cleanup_on_failure);

esp_err_t board_bus_init_spi(int host, int mosi, int miso, int sclk,
                             int max_transfer_bytes);
esp_err_t board_bus_init_i2c(int port, int sda, int scl, int freq_hz);

#ifdef __cplusplus
}
#endif
