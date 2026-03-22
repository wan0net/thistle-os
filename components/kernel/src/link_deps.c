// SPDX-License-Identifier: BSD-3-Clause
// Force-link ESP-IDF symbols that the Rust kernel references via FFI.
//
// The Rust staticlib (libthistle_kernel.a) calls these functions directly.
// Without this file, the linker may not pull them in from ESP-IDF component
// archives because the Rust lib is processed before those archives.
//
// This file is never called — it just ensures the symbols are present.

#include "esp_spiffs.h"
#include "nvs_flash.h"
#include "esp_adc/adc_cali_scheme.h"
#include "host/ble_gap.h"
#include "host/ble_gatt.h"
#include "host/ble_hs.h"
#include "host/ble_hs_mbuf.h"
#include "services/gap/ble_svc_gap.h"
#include "services/gatt/ble_svc_gatt.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "esp_elf.h"

// Reference each symbol so the linker pulls it from the ESP-IDF archive.
// This function is never called — it's dead code that exists only to
// create linker references.
__attribute__((used))
static void _force_link_deps(void) {
    // SPIFFS
    (void)esp_vfs_spiffs_register;
    (void)esp_spiffs_info;
    // NVS
    (void)nvs_flash_init;
    (void)nvs_flash_erase;
    // BLE / NimBLE
    (void)ble_gap_adv_stop;
    (void)ble_gap_terminate;
    (void)ble_svc_gap_device_name_set;
    (void)ble_svc_gap_init;
    (void)ble_svc_gatt_init;
    (void)nimble_port_init;
    (void)nimble_port_freertos_init;
    (void)nimble_port_freertos_deinit;
    (void)nimble_port_run;
    (void)ble_hs_mbuf_from_flat;
    (void)ble_gatts_notify_custom;
    // ELF loader
    (void)esp_elf_init;
    (void)esp_elf_relocate;
    (void)esp_elf_request;
    (void)esp_elf_deinit;
    // ADC calibration
#if ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
    (void)adc_cali_create_scheme_line_fitting;
    (void)adc_cali_delete_scheme_line_fitting;
#endif
    // FreeRTOS (xTaskCreate is a macro in v5.5, may not need this)
}
