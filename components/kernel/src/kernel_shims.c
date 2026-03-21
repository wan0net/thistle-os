// SPDX-License-Identifier: BSD-3-Clause
// Thin C shims for Rust kernel FFI
//
// The Rust kernel modules reference these C functions via extern "C".
// Some are ESP-IDF APIs that need wrappers, others are helpers.

#include "esp_log.h"
#include "hal/board.h"
#include "hal/display.h"
#include "thistle/permissions.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

// ── Memory wrappers (Rust syscall_table exports these) ──────────────
void *libc_malloc(size_t sz) { return malloc(sz); }
void libc_free(void *p) { free(p); }
void *libc_realloc(void *p, size_t sz) { return realloc(p, sz); }

// ── Display helpers ─────────────────────────────────────────────────
uint16_t hal_display_get_width_helper(void) {
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->display) ? reg->display->width : 320;
}

uint16_t hal_display_get_height_helper(void) {
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->display) ? reg->display->height : 240;
}

// ── File syscall implementations ────────────────────────────────────
void *thistle_fs_open_impl(const char *path, const char *mode) { return fopen(path, mode); }
int thistle_fs_read_impl(void *f, void *buf, unsigned int size) { return (int)fread(buf, 1, size, f); }
int thistle_fs_write_impl(void *f, const void *buf, unsigned int size) { return (int)fwrite(buf, 1, size, f); }
int thistle_fs_close_impl(void *f) { return fclose(f); }

// ── HAL syscall implementations ─────────────────────────────────────
void thistle_input_register_cb_impl(void *cb, void *ud) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg) {
        for (int i = 0; i < reg->input_count; i++) {
            if (reg->inputs[i] && reg->inputs[i]->register_callback) {
                reg->inputs[i]->register_callback(cb, ud);
            }
        }
    }
}

int thistle_radio_send_impl(const void *data, unsigned int len) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->radio && reg->radio->send) return reg->radio->send(data, len);
    return -1;
}
int thistle_radio_start_rx_impl(void) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->radio && reg->radio->start_rx) return reg->radio->start_rx();
    return -1;
}
int thistle_radio_set_freq_impl(float freq) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->radio && reg->radio->set_frequency) return reg->radio->set_frequency(freq);
    return -1;
}
int thistle_gps_get_position_impl(void *pos) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->gps && reg->gps->get_position) return reg->gps->get_position(pos);
    return -1;
}
int thistle_gps_enable_impl(int enable) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->gps && reg->gps->enable) return reg->gps->enable(enable);
    return -1;
}
int thistle_power_get_battery_mv_impl(void) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->power && reg->power->get_battery_mv) return reg->power->get_battery_mv();
    return 0;
}
int thistle_power_get_battery_pct_impl(void) {
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->power && reg->power->get_battery_pct) return reg->power->get_battery_pct();
    return 0;
}

// ── Permissions functions called by C tests ──────────────────────────
// These are Rust-internal but the C test suite references them.
// Simple C wrappers that call the same logic.
permission_t permissions_parse(const char *name) {
    if (!name) return 0;
    if (strcmp(name, "radio") == 0) return PERM_RADIO;
    if (strcmp(name, "gps") == 0) return PERM_GPS;
    if (strcmp(name, "storage") == 0) return PERM_STORAGE;
    if (strcmp(name, "network") == 0) return PERM_NETWORK;
    if (strcmp(name, "audio") == 0) return PERM_AUDIO;
    if (strcmp(name, "system") == 0) return PERM_SYSTEM;
    if (strcmp(name, "ipc") == 0) return PERM_IPC;
    return 0;
}

char *permissions_to_string(permission_set_t perms, char *buf, size_t buf_len) {
    if (!buf || buf_len == 0) return buf;
    buf[0] = '\0';
    static const struct { permission_t flag; const char *name; } map[] = {
        { PERM_RADIO, "radio" }, { PERM_GPS, "gps" }, { PERM_STORAGE, "storage" },
        { PERM_NETWORK, "network" }, { PERM_AUDIO, "audio" }, { PERM_SYSTEM, "system" },
        { PERM_IPC, "ipc" },
    };
    size_t pos = 0;
    for (int i = 0; i < 7; i++) {
        if (perms & map[i].flag) {
            if (pos > 0 && pos < buf_len - 1) buf[pos++] = ',';
            size_t nlen = strlen(map[i].name);
            if (pos + nlen < buf_len) { memcpy(buf + pos, map[i].name, nlen); pos += nlen; }
        }
    }
    buf[pos < buf_len ? pos : buf_len - 1] = '\0';
    return buf;
}

// ── WiFi C shims ────────────────────────────────────────────────────
// The Rust wifi_manager calls these via extern "C"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"

int wifi_manager_init_hardware(void) {
    esp_err_t ret;
    ret = esp_netif_init();
    if (ret != ESP_OK) return ret;
    ret = esp_event_loop_create_default();
    if (ret != ESP_OK && ret != ESP_ERR_INVALID_STATE) return ret;
    esp_netif_create_default_wifi_sta();
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    return esp_wifi_init(&cfg);
}

int wifi_manager_do_ntp_sync(void) {
    // NTP sync stub — implement with SNTP when needed
    return 0;
}

// WiFi AP record accessors
#include "esp_wifi_types.h"
const char *wifi_ap_record_get_ssid(const wifi_ap_record_t *ap) {
    return (const char *)ap->ssid;
}
int wifi_ap_record_get_rssi(const wifi_ap_record_t *ap) {
    return ap->rssi;
}
int wifi_ap_record_get_channel(const wifi_ap_record_t *ap) {
    return ap->primary;
}
int wifi_ap_record_is_open(const wifi_ap_record_t *ap) {
    return ap->authmode == WIFI_AUTH_OPEN;
}

// ── BLE C shims ─────────────────────────────────────────────────────
// The Rust ble_manager calls these for NimBLE operations
void ble_manager_do_advertise(void) {
    // NimBLE advertising — implemented in ble C shim when needed
}
void ble_manager_register_gatt_services(void) {
    // NimBLE GATT service registration — implemented in ble C shim when needed
}

// ── Crypto HAL accessor ─────────────────────────────────────────────
const void *hal_crypto_get(void) {
    const hal_registry_t *reg = hal_get_registry();
    return reg ? (const void *)reg->crypto : NULL;
}

// ── OTA helper ──────────────────────────────────────────────────────
#include "esp_ota_ops.h"
int esp_ota_img_pending_verify(void) {
    esp_ota_img_states_t state;
    const esp_partition_t *running = esp_ota_get_running_partition();
    if (!running) return 0;
    if (esp_ota_get_state_partition(running, &state) != ESP_OK) return 0;
    return state == ESP_OTA_IMG_PENDING_VERIFY;
}
