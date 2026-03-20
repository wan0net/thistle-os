#include "esp_timer.h"
#include "esp_err.h"
#include <stdlib.h>
#include <stdio.h>
#include <time.h>
#include <string.h>

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

/* Stubs for subsystems not available in simulator */
esp_err_t ota_init(void) { return ESP_OK; }
esp_err_t permissions_init(void) { return ESP_OK; }
esp_err_t wifi_manager_init(void) { return ESP_OK; }

/* wifi_manager stubs for statusbar/launcher/settings */
int wifi_manager_get_state(void) { return 0; }
int wifi_manager_get_rssi(void) { return 0; }
const char *wifi_manager_get_ip(void) { return NULL; }
esp_err_t wifi_manager_scan(void *results, uint8_t max_results, uint8_t *out_count) {
    (void)results; (void)max_results;
    if (out_count) *out_count = 0;
    return ESP_ERR_NOT_SUPPORTED;
}
esp_err_t wifi_manager_connect(const char *ssid, const char *password, uint32_t timeout_ms) {
    (void)ssid; (void)password; (void)timeout_ms;
    return ESP_ERR_NOT_SUPPORTED;
}
esp_err_t wifi_manager_disconnect(void) { return ESP_OK; }
esp_err_t wifi_manager_ntp_sync(void) { return ESP_ERR_NOT_SUPPORTED; }
void wifi_manager_get_time_str(char *buf, unsigned long buf_len) {
    time_t now;
    struct tm tm_info;
    time(&now);
    localtime_r(&now, &tm_info);
    snprintf(buf, buf_len, "%02d:%02d", tm_info.tm_hour, tm_info.tm_min);
}
void wifi_manager_get_date_str(char *buf, unsigned long buf_len) {
    time_t now;
    struct tm tm_info;
    time(&now);
    localtime_r(&now, &tm_info);
    snprintf(buf, buf_len, "%04d-%02d-%02d",
             tm_info.tm_year + 1900, tm_info.tm_mon + 1, tm_info.tm_mday);
}

/* BLE manager stubs — BLE hardware not available in simulator */
#include "thistle/ble_manager.h"
esp_err_t ble_manager_init(const char *name) { (void)name; return ESP_OK; }
ble_state_t ble_manager_get_state(void) { return BLE_STATE_OFF; }
esp_err_t ble_manager_start_advertising(void) { return ESP_ERR_NOT_SUPPORTED; }
esp_err_t ble_manager_stop_advertising(void) { return ESP_OK; }
esp_err_t ble_manager_disconnect(void) { return ESP_OK; }
esp_err_t ble_manager_send(const uint8_t *data, size_t len) { (void)data; (void)len; return ESP_ERR_NOT_SUPPORTED; }
esp_err_t ble_manager_send_notification(const char *title, const char *body) { (void)title; (void)body; return ESP_ERR_NOT_SUPPORTED; }
esp_err_t ble_manager_register_rx_cb(ble_rx_cb_t cb, void *user_data) { (void)cb; (void)user_data; return ESP_OK; }
const char *ble_manager_get_peer_name(void) { return NULL; }

/* ELF loader stub */
esp_err_t elf_loader_init(void) {
    printf("I (elf_loader) ELF loader disabled in simulator\n");
    return ESP_OK;
}
