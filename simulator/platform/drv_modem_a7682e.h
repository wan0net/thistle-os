/*
 * Simulator stub for A7682E modem driver header.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#pragma once

#include "esp_err.h"
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

typedef void (*drv_a7682e_sms_cb_t)(int index, void *user_data);

static inline bool drv_a7682e_is_ready(void) { return false; }
static inline esp_err_t drv_a7682e_send_sms(const char *dest, const char *text) {
    (void)dest; (void)text; return ESP_ERR_NOT_SUPPORTED;
}
static inline esp_err_t drv_a7682e_read_sms(int index, char *sender, size_t sender_sz,
                                             char *body, size_t body_sz) {
    (void)index; (void)sender; (void)sender_sz; (void)body; (void)body_sz;
    return ESP_ERR_NOT_SUPPORTED;
}
static inline esp_err_t drv_a7682e_delete_sms(int index) { (void)index; return ESP_OK; }
static inline esp_err_t drv_a7682e_sms_init(void) { return ESP_ERR_NOT_SUPPORTED; }
static inline void drv_a7682e_register_sms_cb(drv_a7682e_sms_cb_t cb, void *user_data) {
    (void)cb; (void)user_data;
}
