// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — driver_manager module
//
// Port of components/kernel/src/driver_manager.c
// Calls board_init() and iterates the HAL registry to start/stop drivers.
// The HAL registry is an opaque C struct accessed only via C helper functions.

use std::os::raw::c_void;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;

// ---------------------------------------------------------------------------
// HAL registry FFI
//
// The registry is opaque — we obtain it via hal_get_registry() and pass
// pointers into C functions that understand the layout.
// ---------------------------------------------------------------------------

extern "C" {
    fn board_init() -> i32;
    fn hal_get_registry() -> *const c_void;

    // Per-driver init/deinit — called through the opaque registry by the
    // C-side helpers below.  These are thin wrappers that accept the opaque
    // registry pointer and dispatch into the vtable fields.
    fn hal_registry_start_all() -> i32;
    fn hal_registry_stop_all() -> i32;

    // Logging helpers
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

// ESP log levels
const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_ERROR: i32 = 1;

static TAG: &[u8] = b"drv_mgr\0";

// ---------------------------------------------------------------------------
// FFI exports — same names as C originals
// ---------------------------------------------------------------------------

/// Initialise the driver manager: call board_init() and log the board name.
///
/// # Safety
/// May be called from C. Internally calls board_init() and hal_get_registry().
#[no_mangle]
pub extern "C" fn driver_manager_init() -> i32 {
    unsafe {
        let ret = board_init();
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"board_init() failed: %d\0".as_ptr(),
                ret,
            );
            return ret;
        }

        // hal_get_registry() returns a pointer to the global registry struct.
        // We only log the board name here; the registry contents are opaque.
        let _reg = hal_get_registry();

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"driver_manager_init() OK\0".as_ptr(),
        );
    }

    ESP_OK
}

/// Start all drivers listed in the HAL registry.
///
/// Iterates display, inputs, radio, GPS, audio, power, IMU, storage in that
/// order, calling each driver's init() vtable function.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn driver_manager_start_all() -> i32 {
    unsafe {
        let reg = hal_get_registry();
        if reg.is_null() {
            return ESP_OK; // Nothing registered yet
        }

        let ret = hal_registry_start_all();
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"driver start failed: %d\0".as_ptr(),
                ret,
            );
            return ret;
        }

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"All drivers started\0".as_ptr(),
        );
    }

    ESP_OK
}

/// Stop all drivers in reverse initialisation order.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn driver_manager_stop_all() -> i32 {
    unsafe {
        let reg = hal_get_registry();
        if reg.is_null() {
            return ESP_OK;
        }

        hal_registry_stop_all();

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"All drivers stopped\0".as_ptr(),
        );
    }

    ESP_OK
}
