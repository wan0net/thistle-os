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

__attribute__((weak)) uint32_t wm_widget_get_app_root(void) { return 0; }
__attribute__((weak)) uint32_t wm_widget_create_container(uint32_t p) { return 0; }
__attribute__((weak)) uint32_t wm_widget_create_label(uint32_t p, const char *t) { return 0; }
__attribute__((weak)) uint32_t wm_widget_create_button(uint32_t p, const char *t) { return 0; }
__attribute__((weak)) uint32_t wm_widget_create_text_input(uint32_t p, const char *t) { return 0; }
__attribute__((weak)) void wm_widget_destroy(uint32_t w) {}
__attribute__((weak)) void wm_widget_set_text(uint32_t w, const char *t) {}
__attribute__((weak)) const char *wm_widget_get_text(uint32_t w) { return ""; }
__attribute__((weak)) void wm_widget_set_size(uint32_t w, int32_t width, int32_t h) {}
__attribute__((weak)) void wm_widget_set_pos(uint32_t w, int32_t x, int32_t y) {}
__attribute__((weak)) void wm_widget_set_visible(uint32_t w, bool v) {}
__attribute__((weak)) void wm_widget_set_bg_color(uint32_t w, uint32_t c) {}
__attribute__((weak)) void wm_widget_set_text_color(uint32_t w, uint32_t c) {}
__attribute__((weak)) void wm_widget_set_font_size(uint32_t w, int32_t s) {}
__attribute__((weak)) void wm_widget_set_layout(uint32_t w, int32_t l) {}
__attribute__((weak)) void wm_widget_set_align(uint32_t w, int32_t m, int32_t c) {}
__attribute__((weak)) void wm_widget_set_gap(uint32_t w, int32_t g) {}
__attribute__((weak)) void wm_widget_set_flex_grow(uint32_t w, int32_t g) {}
__attribute__((weak)) void wm_widget_set_scrollable(uint32_t w, bool s) {}
__attribute__((weak)) void wm_widget_set_padding(uint32_t w, int32_t t, int32_t r, int32_t b, int32_t l) {}
__attribute__((weak)) void wm_widget_set_border_width(uint32_t w, int32_t bw) {}
__attribute__((weak)) void wm_widget_set_radius(uint32_t w, int32_t r) {}
__attribute__((weak)) void wm_widget_on_event(uint32_t w, int32_t e, const void *cb, void *ud) {}
__attribute__((weak)) void wm_widget_set_password_mode(uint32_t w, bool p) {}
__attribute__((weak)) void wm_widget_set_one_line(uint32_t w, bool o) {}
__attribute__((weak)) void wm_widget_set_placeholder(uint32_t w, const char *t) {}
__attribute__((weak)) uint32_t wm_widget_theme_primary(void) { return 0x000000; }
__attribute__((weak)) uint32_t wm_widget_theme_bg(void) { return 0xFFFFFF; }
__attribute__((weak)) uint32_t wm_widget_theme_surface(void) { return 0xF0F0F0; }
__attribute__((weak)) uint32_t wm_widget_theme_text(void) { return 0x000000; }
__attribute__((weak)) uint32_t wm_widget_theme_text_secondary(void) { return 0x808080; }
