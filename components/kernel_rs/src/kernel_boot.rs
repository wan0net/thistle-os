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

    // Permissions (Rust)
    crate::permissions::init();

    // Signing (Rust) — dev key
    let dev_key: [u8; 32] = [
        0x25, 0xd3, 0xfc, 0xbc, 0x28, 0x2d, 0xb4, 0x6f,
        0xf4, 0x37, 0x78, 0x5c, 0x32, 0x90, 0xaf, 0x73,
        0x98, 0x17, 0xf2, 0x0d, 0xb4, 0x37, 0x88, 0x27,
        0xf9, 0x00, 0xc3, 0xf7, 0x7b, 0xe0, 0x27, 0xb7,
    ];
    unsafe { crate::signing::signing_init(dev_key.as_ptr()); }

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

#[no_mangle]
pub extern "C" fn kernel_run() {
    // LVGL tick is driven by ui component. This is the kernel heartbeat.
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[no_mangle]
pub extern "C" fn kernel_uptime_ms() -> u32 {
    let boot = BOOT_TIME_US.lock().map(|t| *t).unwrap_or(0);
    let now = unsafe { esp_timer_get_time() };
    ((now - boot) / 1000) as u32
}
