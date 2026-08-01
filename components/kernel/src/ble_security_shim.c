// SPDX-License-Identifier: BSD-3-Clause
// NimBLE security ABI bridge for the Rust BLE manager.
//
// NimBLE exposes bitfields and a version-specific event union. Keeping those
// accesses in C avoids duplicating an unstable layout in Rust while leaving
// policy and application-data dispatch in the Rust kernel.

#include <stdbool.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdatomic.h>

#include "esp_log.h"
#include "esp_random.h"
#include "host/ble_gap.h"
#include "host/ble_hs.h"
#include "host/ble_sm.h"

static const char *TAG = "ble_security";
static _Atomic uint32_t s_pairing_passkey;

void thistle_ble_security_configure(void)
{
    ble_hs_cfg.sm_io_cap = BLE_SM_IO_CAP_DISP_ONLY;
    ble_hs_cfg.sm_bonding = 1;
    ble_hs_cfg.sm_mitm = 1;
    ble_hs_cfg.sm_sc = 1;
    ble_hs_cfg.sm_sc_only = 1;
    ble_hs_cfg.sm_sec_lvl = 4;
}

int thistle_ble_security_handle_event(struct ble_gap_event *event)
{
    if (event == NULL) {
        return 0;
    }
    if (event->type == BLE_GAP_EVENT_DISCONNECT) {
        atomic_store_explicit(&s_pairing_passkey, 0, memory_order_relaxed);
        return 0;
    }
    if (event->type != BLE_GAP_EVENT_PASSKEY_ACTION) {
        return 0;
    }

    if (event->passkey.params.action != BLE_SM_IOACT_DISP) {
        ESP_LOGE(TAG, "unsupported pairing action: %u",
                 event->passkey.params.action);
        return BLE_HS_ENOTSUP;
    }

    struct ble_sm_io io = {0};
    io.action = BLE_SM_IOACT_DISP;
    io.passkey = 100000U + (esp_random() % 900000U);
    atomic_store_explicit(&s_pairing_passkey, io.passkey, memory_order_relaxed);
    ESP_LOGW(TAG, "BLE pairing passkey: %06" PRIu32, io.passkey);
    return ble_sm_inject_io(event->passkey.conn_handle, &io);
}

uint32_t thistle_ble_pairing_passkey(void)
{
    return atomic_load_explicit(&s_pairing_passkey, memory_order_relaxed);
}

bool thistle_ble_conn_is_authenticated(uint16_t conn_handle)
{
    struct ble_gap_conn_desc desc;
    if (ble_gap_conn_find(conn_handle, &desc) != 0) {
        return false;
    }

    return desc.sec_state.encrypted &&
           desc.sec_state.authenticated &&
           desc.sec_state.key_size >= 16;
}
