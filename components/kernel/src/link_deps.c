// SPDX-License-Identifier: BSD-3-Clause
// Force-link ESP-IDF symbols that the Rust kernel references via FFI.
//
// The Rust staticlib (libthistle_kernel.a) calls these functions directly.
// Without this file, the linker may not pull them in from ESP-IDF component
// archives because the Rust lib is processed before those archives.
//
// This file is never called — it just ensures the symbols are present.

#include "esp_wifi.h"
#include "esp_spiffs.h"
#include "nvs_flash.h"
#include "esp_adc/adc_oneshot.h"
#include "esp_adc/adc_cali.h"
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
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/uart.h"
#include "driver/gpio.h"
#include "driver/i2c_master.h"
#include "driver/spi_master.h"

// esp_ptr_in_drom became `inline static` in ESP-IDF v6 (esp_memory_utils.h),
// which makes it not addressable for FFI. Expose a normal extern wrapper so
// Rust callers can keep using a regular extern "C" declaration.
#include "esp_memory_utils.h"
bool thistle_esp_ptr_in_drom(const void *p)
{
    return esp_ptr_in_drom(p);
}

// WiFi init shim — WIFI_INIT_CONFIG_DEFAULT is a C macro that can't be called
// directly from Rust FFI. This wrapper creates the properly-initialised config
// (including magic values that ESP-IDF v6 validates strictly) and calls
// esp_wifi_init. The zeroed-buffer approach used before broke in v6.
#include "esp_log.h"
static const char *WIFI_SHIM_TAG = "wifi_shim";
esp_err_t thistle_wifi_init(void)
{
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    // Debug: confirm shim is running and osi_funcs is populated.
    // osi_funcs NULL means WIFI_INIT_CONFIG_DEFAULT expanded incorrectly.
    ESP_LOGI(WIFI_SHIM_TAG, "osi_funcs=%p magic=0x%08x",
             (void *)cfg.osi_funcs, (unsigned)cfg.magic);
    esp_err_t ret = esp_wifi_init(&cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(WIFI_SHIM_TAG, "esp_wifi_init returned 0x%x", ret);
    }
    return ret;
}

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
    // ADC oneshot + calibration (called by drv_power_tp4065b.rs)
    (void)adc_oneshot_new_unit;
    (void)adc_oneshot_del_unit;
    (void)adc_oneshot_config_channel;
    (void)adc_oneshot_read;
    (void)adc_cali_raw_to_voltage;
#if ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
    (void)adc_cali_create_scheme_line_fitting;
    (void)adc_cali_delete_scheme_line_fitting;
#endif
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
    (void)adc_cali_create_scheme_curve_fitting;
    (void)adc_cali_delete_scheme_curve_fitting;
#endif
    // FreeRTOS (xTaskCreate is a macro wrapping xTaskCreatePinnedToCore)
    (void)xTaskCreatePinnedToCore;
    (void)vTaskDelete;
    (void)vTaskDelay;
    // UART (called by drv_gps_mia_m10q.rs)
    // Note: uart_set_pin is a variadic macro in ESP-IDF v6 — not addressable.
    (void)uart_driver_install;
    (void)uart_driver_delete;
    (void)uart_param_config;
    (void)uart_read_bytes;
    (void)uart_write_bytes;
    // GPIO (called by multiple drivers)
    (void)gpio_set_direction;
    (void)gpio_set_level;
    (void)gpio_get_level;
    (void)gpio_set_pull_mode;
    (void)gpio_isr_handler_add;
    (void)gpio_isr_handler_remove;
    // I2C master (called by keyboard, touch, accel, OLED, IMU, light drivers)
    (void)i2c_master_bus_add_device;
    (void)i2c_master_transmit;
    (void)i2c_master_transmit_receive;
    // SPI (called by e-paper, SD card drivers)
    (void)spi_device_polling_transmit;
}
