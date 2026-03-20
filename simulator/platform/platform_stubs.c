/*
 * Simulator platform stubs — minimal ESP-IDF API stubs for host build.
 * WiFi and BLE are now in dedicated sim_wifi.c and sim_ble.c.
 * SPDX-License-Identifier: BSD-3-Clause
 */

/* Undef VFS wrappers for this file — we don't call fopen/opendir here */
#undef fopen
#undef opendir
#undef stat

#include "esp_timer.h"
#include "esp_err.h"
#include <stdlib.h>
#include <stdio.h>

/* esp_timer stubs — LVGL tick is driven by main loop */
struct esp_timer { int dummy; };

esp_err_t esp_timer_create(const esp_timer_create_args_t *args, esp_timer_handle_t *handle) {
    (void)args;
    *handle = (esp_timer_handle_t)calloc(1, sizeof(struct esp_timer));
    return ESP_OK;
}

esp_err_t esp_timer_start_periodic(esp_timer_handle_t handle, uint64_t period_us) {
    (void)handle; (void)period_us;
    return ESP_OK;
}

esp_err_t esp_timer_start_once(esp_timer_handle_t handle, uint64_t timeout_us) {
    (void)handle; (void)timeout_us;
    return ESP_OK;
}

esp_err_t esp_timer_delete(esp_timer_handle_t handle) {
    free(handle);
    return ESP_OK;
}

esp_err_t esp_timer_stop(esp_timer_handle_t handle) {
    (void)handle;
    return ESP_OK;
}

/* Kernel subsystem stubs */
esp_err_t ota_init(void) { return ESP_OK; }
bool ota_sd_update_available(void) { return false; }
esp_err_t ota_apply_from_sd(void *progress_cb, void *user_data) {
    (void)progress_cb; (void)user_data;
    return ESP_ERR_NOT_SUPPORTED;
}
const char *ota_get_current_version(void) { return "0.1.0"; }
const char *ota_get_running_partition(void) { return "sim"; }
esp_err_t ota_mark_valid(void) { return ESP_OK; }
esp_err_t ota_rollback(void) { return ESP_ERR_NOT_SUPPORTED; }
esp_err_t permissions_init(void) { return ESP_OK; }

/* ELF loader stub */
esp_err_t elf_loader_init(void) {
    printf("[sim] ELF loader disabled in simulator\n");
    return ESP_OK;
}

/* Driver loader stubs — SD card ELF loading not available in simulator */
#include "thistle/driver_loader.h"
esp_err_t driver_loader_init(void) { return ESP_OK; }
int driver_loader_scan_and_load(void) { return 0; }
esp_err_t driver_loader_load(const char *path) { (void)path; return ESP_ERR_NOT_SUPPORTED; }
int driver_loader_get_count(void) { return 0; }

/* Signing subsystem stubs (simulator build — no mbedtls) */
#include "thistle/signing.h"
static char s_sim_key_hex[THISTLE_SIGN_KEY_SIZE * 2 + 1] = "(simulator)";
esp_err_t signing_init(const uint8_t public_key[THISTLE_SIGN_KEY_SIZE]) {
    (void)public_key;
    return ESP_OK;
}
esp_err_t signing_verify(const uint8_t *data, size_t data_len,
                          const uint8_t signature[THISTLE_SIGN_SIG_SIZE]) {
    (void)data; (void)data_len; (void)signature;
    return ESP_ERR_NOT_FOUND; /* unsigned in sim */
}
esp_err_t signing_verify_file(const char *elf_path) {
    (void)elf_path;
    return ESP_ERR_NOT_FOUND; /* unsigned in sim */
}
bool signing_has_signature(const char *elf_path) {
    (void)elf_path;
    return false;
}
const char *signing_get_public_key_hex(void) {
    return s_sim_key_hex;
}

/* A7682E modem PPP stubs */
esp_err_t drv_a7682e_start_ppp(void) { return ESP_ERR_NOT_SUPPORTED; }
esp_err_t drv_a7682e_stop_ppp(void) { return ESP_OK; }
_Bool drv_a7682e_ppp_connected(void) { return 0; }

/* appstore_client — real HTTP via libcurl (sim_http.c + esp_http_client.h shim) */
