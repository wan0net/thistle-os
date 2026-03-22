// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel boot sequence

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Mutex;

const ESP_OK: i32 = 0;

// ESP-IDF FFI
extern "C" {
    fn esp_timer_get_time() -> i64;
}

// Subsystem init functions (some Rust, some still C)
extern "C" {
    fn nvs_flash_init_safe() -> i32;
    fn spiffs_mount() -> i32;
    fn net_manager_init() -> i32;
    fn driver_manager_init() -> i32;
    fn driver_manager_start_all() -> i32;
    fn syscall_table_init() -> i32;
    fn board_config_init(path: *const c_char) -> i32;
    fn board_init() -> i32;
    fn driver_loader_init() -> i32;
    fn driver_loader_scan_and_load() -> i32;
    fn elf_loader_init() -> i32;
    fn ota_init() -> i32;
    fn wifi_manager_init() -> i32;
    fn net_manager_register_wifi();
}

static BOOT_TIME_US: Mutex<i64> = Mutex::new(0);

// kernel_init, kernel_run, kernel_uptime_ms — same names as C

#[no_mangle]
pub extern "C" fn kernel_init() -> i32 {
    if let Ok(mut t) = BOOT_TIME_US.lock() {
        *t = unsafe { esp_timer_get_time() };
    }

    // NVS flash — required by WiFi, BLE, and other ESP-IDF subsystems
    let ret = unsafe { nvs_flash_init_safe() };
    if ret != ESP_OK { return ret; }

    // Mount SPIFFS — apps, drivers, config, themes live here
    let ret = unsafe { spiffs_mount() };
    if ret != ESP_OK {
        // Non-fatal: SD card can still provide apps/config
    }

    // Event bus (Rust)
    let ret = crate::event::event_bus_init();
    if ret != ESP_OK { return ret; }

    // IPC (Rust)
    let ret = crate::ipc::ipc_init();
    if ret != ESP_OK { return ret; }

    // Network manager (C)
    let ret = unsafe { net_manager_init() };
    if ret != ESP_OK { return ret; }

    // Syscall table (C for now)
    let ret = unsafe { syscall_table_init() };
    if ret != ESP_OK { return ret; }

    // Board config: reads board.json, inits buses, loads drivers
    let ret = unsafe { board_config_init(std::ptr::null()) };
    if ret != ESP_OK {
        // Fallback to compiled board_init + driver_manager
        unsafe {
            driver_manager_init();
            driver_manager_start_all();
            driver_loader_init();
            driver_loader_scan_and_load();
        }
    }

    // App manager (Rust)
    crate::app_manager::app_manager_init();

    // Register the thistle-tk launcher (always register — main.c decides
    // whether to launch it based on the active WM)
    crate::tk_launcher::register();

    // Permissions (Rust)
    crate::permissions::init();

    // Signing (Rust) — Ed25519 public key for signature verification
    // Production key: generated 2026-03-21, private key stored offline
    // The old dev key (0x25,0xd3,...) has been rotated and is no longer valid.
    #[cfg(not(debug_assertions))]
    let signing_key: [u8; 32] = [
        0x27, 0x2d, 0x8d, 0xec, 0x46, 0x69, 0x3f, 0xb1,
        0xe4, 0x2b, 0x25, 0x75, 0xef, 0xc4, 0x6b, 0xba,
        0xd9, 0xda, 0x77, 0xf7, 0x5c, 0x1c, 0xab, 0xc3,
        0xc1, 0x8b, 0x29, 0x5b, 0x8d, 0x90, 0x6a, 0xb4,
    ];
    // Dev key — only used in debug builds. Signs nothing in production.
    #[cfg(debug_assertions)]
    let signing_key: [u8; 32] = [
        0x27, 0x2d, 0x8d, 0xec, 0x46, 0x69, 0x3f, 0xb1,
        0xe4, 0x2b, 0x25, 0x75, 0xef, 0xc4, 0x6b, 0xba,
        0xd9, 0xda, 0x77, 0xf7, 0x5c, 0x1c, 0xab, 0xc3,
        0xc1, 0x8b, 0x29, 0x5b, 0x8d, 0x90, 0x6a, 0xb4,
    ];
    unsafe { crate::signing::signing_init(signing_key.as_ptr()); }

    // ELF loader (C)
    let ret = unsafe { elf_loader_init() };
    if ret != ESP_OK { return ret; }

    // OTA (C)
    let ret = unsafe { ota_init() };
    if ret != ESP_OK { return ret; }

    // WiFi (C, non-fatal)
    let ret = unsafe { wifi_manager_init() };
    if ret == ESP_OK {
        unsafe { net_manager_register_wifi(); }
    }

    // Publish SYSTEM_BOOT event
    crate::event::event_publish_simple(0); // EVENT_SYSTEM_BOOT = 0

    ESP_OK
}

#[cfg(not(test))]
extern "C" {
    fn vTaskDelay(ticks: u32);
}

#[no_mangle]
pub extern "C" fn kernel_run() {
    // LVGL tick is driven by ui component. This is the kernel heartbeat.
    loop {
        #[cfg(target_os = "espidf")]
        unsafe { vTaskDelay(1); } // 1 tick = 1ms at 1000Hz FreeRTOS
        #[cfg(not(target_os = "espidf"))]
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[no_mangle]
pub extern "C" fn kernel_uptime_ms() -> u32 {
    let boot = BOOT_TIME_US.lock().map(|t| *t).unwrap_or(0);
    let now = unsafe { esp_timer_get_time() };
    ((now - boot) / 1000) as u32
}
