// SPDX-License-Identifier: BSD-3-Clause
// Board config — reads board.json from SPIFFS, inits buses, loads drivers

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const ESP_OK: i32 = 0;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;

const MAX_CONFIG_SIZE: usize = 8192;

static BOARD_NAME: Mutex<[u8; 64]> = Mutex::new([0u8; 64]);

// ESP-IDF FFI for bus init and driver loading
extern "C" {
    fn hal_set_board_name(name: *const c_char) -> i32;
    fn hal_bus_register_spi(host_id: i32, handle: *mut std::os::raw::c_void) -> i32;
    fn hal_bus_register_i2c(port: i32, handle: *mut std::os::raw::c_void) -> i32;
    fn driver_loader_init() -> i32;
    fn driver_loader_load_with_config(path: *const c_char, config: *const c_char) -> i32;
    fn board_init() -> i32;
}

// SPI/I2C init — only on real hardware
#[cfg(not(test))]
extern "C" {
    fn spi_bus_initialize(host: i32, config: *const std::os::raw::c_void, dma: i32) -> i32;
    fn i2c_new_master_bus(config: *const std::os::raw::c_void, handle: *mut *mut std::os::raw::c_void) -> i32;
}

// Reuse manifest JSON helpers
use crate::manifest::{json_get_string, json_get_int};

fn set_board_name(name: &str) {
    if let Ok(mut buf) = BOARD_NAME.lock() {
        let bytes = name.as_bytes();
        let len = bytes.len().min(63);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;
    }
}

// This function does the main work but requires ESP-IDF APIs for bus init.
// On the simulator, it falls back to compiled board_init().
fn load_config(config_path: &str) -> i32 {
    let json = match fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(_) => {
            // Fall back to compiled board_init() + start all drivers
            let ret = unsafe { board_init() };
            if ret != ESP_OK { return ret; }
            unsafe { crate::driver_manager::driver_manager_start_all(); }
            return ESP_OK;
        }
    };

    if json.len() > MAX_CONFIG_SIZE {
        return ESP_ERR_INVALID_SIZE;
    }

    // Parse board name
    if let Some(board_section) = extract_object(&json, "board") {
        if let Some(name) = json_get_string(&board_section, "name") {
            set_board_name(&name);
            if let Ok(cname) = CString::new(name.as_str()) {
                unsafe { hal_set_board_name(cname.as_ptr()); }
            }
        }
    }

    // Bus initialization would happen here on real hardware
    // For now, the C board_config.c handles this

    // Load drivers from config
    unsafe { driver_loader_init(); }

    if let Some(drivers_arr) = find_array(&json, "drivers") {
        for i in 0..12 {
            if let Some(drv) = nth_object(&drivers_arr, i) {
                let entry = json_get_string(&drv, "entry").unwrap_or_default();
                let _id = json_get_string(&drv, "id").unwrap_or_default();
                let _hal = json_get_string(&drv, "hal").unwrap_or_default();

                if entry.is_empty() { continue; }

                // Extract config sub-object
                let config = extract_object(&drv, "config").unwrap_or_else(|| "{}".to_string());

                // Try SPIFFS first, then SD
                let paths = [
                    format!("/spiffs/drivers/{}", entry),
                    format!("/tmp/thistle_sdcard/drivers/{}", entry),
                ];

                let mut found = false;
                for path in &paths {
                    if Path::new(path).exists() {
                        if let (Ok(cpath), Ok(cconfig)) = (CString::new(path.as_str()), CString::new(config.as_str())) {
                            unsafe {
                                driver_loader_load_with_config(cpath.as_ptr(), cconfig.as_ptr());
                            }
                        }
                        found = true;
                        break;
                    }
                }

                if !found {
                    // Driver not found on disk — skip
                }
            }
        }
    }

    ESP_OK
}

// Simple JSON array/object extraction helpers
fn find_array(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after = &json[start + pattern.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let trimmed = after.trim_start();
    if !trimmed.starts_with('[') { return None; }
    let mut depth = 0;
    for (i, ch) in trimmed.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(trimmed[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_object(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after = &json[start + pattern.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let trimmed = after.trim_start();
    if !trimmed.starts_with('{') { return None; }
    let mut depth = 0;
    for (i, ch) in trimmed.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(trimmed[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn nth_object(array_str: &str, n: usize) -> Option<String> {
    let inner = array_str.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut depth = 0;
    let mut count = 0;
    let mut obj_start = None;
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    if count == n {
                        obj_start = Some(i);
                    }
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if count == n {
                        return Some(inner[obj_start?..=i].to_string());
                    }
                    count += 1;
                }
            }
            _ => {}
        }
    }
    None
}

// FFI exports

#[no_mangle]
pub unsafe extern "C" fn board_config_init(config_path: *const c_char) -> i32 {
    let path = if config_path.is_null() {
        "/spiffs/config/board.json".to_string()
    } else {
        match CStr::from_ptr(config_path).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return ESP_ERR_INVALID_ARG,
        }
    };

    load_config(&path)
}

#[no_mangle]
pub extern "C" fn board_config_get_name() -> *const c_char {
    match BOARD_NAME.lock() {
        Ok(buf) => buf.as_ptr() as *const c_char,
        Err(_) => b"Unknown\0".as_ptr() as *const c_char,
    }
}
