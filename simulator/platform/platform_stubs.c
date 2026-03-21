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
/* permissions, ipc, event, app_manager, kernel, signing, display_server,
 * board_config — all provided by Rust kernel lib (libthistle_kernel.a) */

/* esp_timer_get_time — Rust kernel_boot needs a real symbol, not an inline */
#include <sys/time.h>
int64_t esp_timer_get_time(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (int64_t)tv.tv_sec * 1000000LL + (int64_t)tv.tv_usec;
}

/* ESP-IDF heap API stub for Rust app_manager */
#include <stddef.h>
size_t heap_caps_get_free_size(unsigned int caps) { (void)caps; return 4 * 1024 * 1024; }

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
esp_err_t driver_loader_load_with_config(const char *path, const char *config_json) {
    (void)path; (void)config_json; return ESP_ERR_NOT_SUPPORTED;
}
int driver_loader_get_count(void) { return 0; }
const char *driver_loader_get_config(void) { return "{}"; }

/* Signing — provided by Rust kernel lib (ed25519-dalek) */

/* A7682E modem PPP stubs */
esp_err_t drv_a7682e_start_ppp(void) { return ESP_ERR_NOT_SUPPORTED; }
esp_err_t drv_a7682e_stop_ppp(void) { return ESP_OK; }
_Bool drv_a7682e_ppp_connected(void) { return 0; }

/* appstore_client — real HTTP via libcurl (sim_http.c + esp_http_client.h shim) */
