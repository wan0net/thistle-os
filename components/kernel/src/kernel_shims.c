// SPDX-License-Identifier: BSD-3-Clause
// Weak widget stubs — resolve link-time deps from Rust widget.rs
//
// Real implementations are in ui/widget_shims.c. The linker processes
// kernel_rs before ui, so these weak stubs satisfy the static-lib
// extraction; the ui component's strong symbols override at final link.
//
// All other functions formerly in this file have been migrated to pure
// Rust modules in components/kernel_rs/src/:
//   spiffs_mount()                → kernel_boot.rs
//   nvs_flash_init_safe()         → kernel_boot.rs
//   libc_malloc/free/realloc      → syscall_table.rs (direct libc)
//   hal_display_get_width/height  → syscall_table.rs (Rust HAL registry)
//   thistle_fs_*_impl             → syscall_table.rs (direct libc)
//   thistle_input/radio/gps/power → syscall_table.rs (Rust HAL registry)
//   permissions_parse/to_string   → permissions.rs
//   wifi_manager_init_hardware    → wifi_manager.rs
//   wifi_manager_do_ntp_sync      → wifi_manager.rs
//   wifi_ap_record_get_*          → wifi_manager.rs
//   ble_shim_*                    → ble_manager.rs (direct NimBLE FFI)
//   ble_manager_do_advertise()    → ble_manager.rs (do_advertise + ble_gap_adv_start)
//   ble_manager_register_gatt_services() → ble_manager.rs (register_gatt_services)
//   hal_crypto_get()              → hal_registry.rs
//   esp_ota_img_pending_verify()  → ota.rs (Rust constant)

#include <stdint.h>
#include <stdbool.h>

// ADC calibration shims — on chips that don't support a given scheme, provide
// a weak stub that returns ESP_ERR_NOT_SUPPORTED so the Rust driver's fallback
// chain works without linker errors.
#include "esp_err.h"
#include "esp_adc/adc_cali_scheme.h"

#if !ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
__attribute__((weak)) int adc_cali_create_scheme_line_fitting(const void *cfg, void **out) {
    (void)cfg; (void)out;
    return ESP_ERR_NOT_SUPPORTED;
}
__attribute__((weak)) int adc_cali_delete_scheme_line_fitting(void *handle) {
    (void)handle;
    return ESP_ERR_NOT_SUPPORTED;
}
#endif

#if !ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
__attribute__((weak)) int adc_cali_create_scheme_curve_fitting(const void *cfg, void **out) {
    (void)cfg; (void)out;
    return ESP_ERR_NOT_SUPPORTED;
}
__attribute__((weak)) int adc_cali_delete_scheme_curve_fitting(void *handle) {
    (void)handle;
    return ESP_ERR_NOT_SUPPORTED;
}
#endif

// wm_widget_* shims live in ui/src/widget_shims.c (they need to look up the
// active WM vtable from the display server). They MUST NOT be duplicated
// here as weak stubs: when both kernel.a (weak) and ui.a (strong) define the
// symbol, the linker pulls in kernel.a first to satisfy the Rust extern
// reference, the symbol is "resolved", and ui.a's strong vtable-dispatching
// version is never pulled in. End result: every thistle_ui_create_* call
// returns 0 and no widgets ever get created.

// lstat shim — ESP-IDF newlib doesn't provide lstat (no symlinks on SPIFFS/FAT).
// Rust std::fs::metadata calls lstat internally. Forward to stat.
#include <sys/stat.h>
int __attribute__((weak)) lstat(const char *path, struct stat *buf) {
    return stat(path, buf);
}
