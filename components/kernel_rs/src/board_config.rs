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
        for i in 0..8 {
            if let Some(drv) = nth_object(&drivers_arr, i) {
                let entry = json_get_string(&drv, "entry").unwrap_or_default();

                if entry.is_empty() { continue; }

                // Sanitize entry path — reject traversal and absolute paths
                if entry.contains("..") || entry.contains('/') || entry.contains('\\') || entry.starts_with('.') {
                    continue;
                }

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

pub fn extract_object(json: &str, key: &str) -> Option<String> {
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

// ── Window manager preference from system.json ──────────────────────

static WM_NAME: Mutex<[u8; 64]> = Mutex::new([0u8; 64]);

/// Read the "window_manager" value from system.json.
/// Tries /spiffs/config/system.json first, then SD card.
fn load_wm_name() {
    let paths = [
        "/spiffs/config/system.json",
        "/tmp/thistle_sdcard/config/system.json",
    ];
    for path in &paths {
        if let Ok(json) = fs::read_to_string(path) {
            // Look inside "thistle_os" object for "window_manager"
            if let Some(os_obj) = extract_object(&json, "thistle_os") {
                if let Some(wm) = json_get_string(&os_obj, "window_manager") {
                    if let Ok(mut buf) = WM_NAME.lock() {
                        let bytes = wm.as_bytes();
                        let len = bytes.len().min(63);
                        buf[..len].copy_from_slice(&bytes[..len]);
                        buf[len] = 0;
                    }
                    return;
                }
            }
        }
    }
}

/// Return the configured WM name, or NULL if not set.
/// Reads system.json on first call and caches the result.
#[no_mangle]
pub extern "C" fn board_config_get_wm_name() -> *const c_char {
    // Load lazily on first call
    {
        let buf = WM_NAME.lock().unwrap();
        if buf[0] == 0 {
            drop(buf);
            load_wm_name();
        }
    }
    let buf = WM_NAME.lock().unwrap();
    if buf[0] == 0 {
        return std::ptr::null();
    }
    buf.as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── find_array tests ────────────────────────────────────────────────

    #[test]
    fn test_find_array_simple() {
        let json = r#"{"drivers": [1, 2, 3]}"#;
        let result = find_array(json, "drivers").unwrap();
        assert_eq!(result, "[1, 2, 3]");
    }

    #[test]
    fn test_find_array_with_whitespace() {
        let json = r#"{"drivers"  :  [1, 2, 3]}"#;
        let result = find_array(json, "drivers").unwrap();
        assert_eq!(result, "[1, 2, 3]");
    }

    #[test]
    fn test_find_array_nested_arrays() {
        let json = r#"{"items": [{"a": [1]}, {"b": 2}]}"#;
        let result = find_array(json, "items").unwrap();
        assert_eq!(result, r#"[{"a": [1]}, {"b": 2}]"#);
    }

    #[test]
    fn test_find_array_missing_key() {
        let json = r#"{"drivers": [1, 2, 3]}"#;
        let result = find_array(json, "nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_array_key_not_array() {
        let json = r#"{"drivers": "not an array"}"#;
        let result = find_array(json, "drivers");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_array_empty_array() {
        let json = r#"{"items": []}"#;
        let result = find_array(json, "items").unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_find_array_nested_mixed() {
        let json = r#"{"config": [{"name": "test", "values": [1, 2, 3]}, {"name": "other"}]}"#;
        let result = find_array(json, "config").unwrap();
        assert_eq!(
            result,
            r#"[{"name": "test", "values": [1, 2, 3]}, {"name": "other"}]"#
        );
    }

    // ─── extract_object tests ────────────────────────────────────────────

    #[test]
    fn test_extract_object_simple() {
        let json = r#"{"board": {"name": "tdeck"}}"#;
        let result = extract_object(json, "board").unwrap();
        assert_eq!(result, r#"{"name": "tdeck"}"#);
    }

    #[test]
    fn test_extract_object_with_whitespace() {
        let json = r#"{"board"  :  {"name": "tdeck"}}"#;
        let result = extract_object(json, "board").unwrap();
        assert_eq!(result, r#"{"name": "tdeck"}"#);
    }

    #[test]
    fn test_extract_object_nested_objects() {
        let json = r#"{"root": {"inner": {"deep": "value"}}}"#;
        let result = extract_object(json, "root").unwrap();
        assert_eq!(result, r#"{"inner": {"deep": "value"}}"#);
    }

    #[test]
    fn test_extract_object_missing_key() {
        let json = r#"{"board": {"name": "tdeck"}}"#;
        let result = extract_object(json, "nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_object_key_not_object() {
        let json = r#"{"board": "not an object"}"#;
        let result = extract_object(json, "board");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_object_empty_object() {
        let json = r#"{"empty": {}}"#;
        let result = extract_object(json, "empty").unwrap();
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_extract_object_with_nested_array() {
        let json = r#"{"config": {"items": [1, 2, 3], "name": "test"}}"#;
        let result = extract_object(json, "config").unwrap();
        assert_eq!(result, r#"{"items": [1, 2, 3], "name": "test"}"#);
    }

    // ─── nth_object tests ────────────────────────────────────────────────

    #[test]
    fn test_nth_object_first() {
        let array = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        let result = nth_object(array, 0).unwrap();
        assert_eq!(result, r#"{"a": 1}"#);
    }

    #[test]
    fn test_nth_object_middle() {
        let array = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        let result = nth_object(array, 1).unwrap();
        assert_eq!(result, r#"{"b": 2}"#);
    }

    #[test]
    fn test_nth_object_last() {
        let array = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        let result = nth_object(array, 2).unwrap();
        assert_eq!(result, r#"{"c": 3}"#);
    }

    #[test]
    fn test_nth_object_out_of_range() {
        let array = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        let result = nth_object(array, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_nth_object_empty_array() {
        let array = "[]";
        let result = nth_object(array, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_nth_object_single_element() {
        let array = r#"[{"single": true}]"#;
        let result = nth_object(array, 0).unwrap();
        assert_eq!(result, r#"{"single": true}"#);
    }

    #[test]
    fn test_nth_object_nested_objects() {
        let array = r#"[{"outer": {"inner": "value"}}, {"other": "data"}]"#;
        let result = nth_object(array, 0).unwrap();
        assert_eq!(result, r#"{"outer": {"inner": "value"}}"#);
    }

    #[test]
    fn test_nth_object_with_whitespace() {
        let array = r#"[  {"a": 1}  ,  {"b": 2}  ]"#;
        let result = nth_object(array, 0).unwrap();
        assert_eq!(result, r#"{"a": 1}"#);
    }

    #[test]
    fn test_nth_object_with_whitespace_index_one() {
        let array = r#"[  {"a": 1}  ,  {"b": 2}  ]"#;
        let result = nth_object(array, 1).unwrap();
        assert_eq!(result, r#"{"b": 2}"#);
    }

    // ─── set_board_name tests ────────────────────────────────────────────

    #[test]
    fn test_set_board_name_simple() {
        // Clear BOARD_NAME first
        if let Ok(mut buf) = BOARD_NAME.lock() {
            buf[0] = 0;
        }

        set_board_name("tdeck");
        let buf = BOARD_NAME.lock().unwrap();
        let cstr = CStr::from_bytes_until_nul(&buf[..]).unwrap();
        let name = cstr.to_str().unwrap();
        assert_eq!(name, "tdeck");
    }

    #[test]
    fn test_set_board_name_longer_string() {
        // Clear BOARD_NAME first
        if let Ok(mut buf) = BOARD_NAME.lock() {
            buf[0] = 0;
        }

        set_board_name("t-display-s3-pro");
        let buf = BOARD_NAME.lock().unwrap();
        let cstr = CStr::from_bytes_until_nul(&buf[..]).unwrap();
        let name = cstr.to_str().unwrap();
        assert_eq!(name, "t-display-s3-pro");
    }

    #[test]
    fn test_set_board_name_truncate() {
        // Clear BOARD_NAME first
        if let Ok(mut buf) = BOARD_NAME.lock() {
            buf[0] = 0;
        }

        let long_name = "a".repeat(100);
        set_board_name(&long_name);

        let buf = BOARD_NAME.lock().unwrap();
        let cstr = CStr::from_bytes_until_nul(&buf[..]).unwrap();
        let name = cstr.to_str().unwrap();
        assert_eq!(name.len(), 63);
        assert_eq!(name, "a".repeat(63));
    }

    #[test]
    fn test_set_board_name_empty() {
        // Clear BOARD_NAME first
        if let Ok(mut buf) = BOARD_NAME.lock() {
            buf[0] = 0;
        }

        set_board_name("");
        let buf = BOARD_NAME.lock().unwrap();
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_set_board_name_overwrite() {
        // Clear BOARD_NAME first
        if let Ok(mut buf) = BOARD_NAME.lock() {
            buf[0] = 0;
        }

        set_board_name("first");
        let buf1 = BOARD_NAME.lock().unwrap();
        let cstr1 = CStr::from_bytes_until_nul(&buf1[..]).unwrap();
        assert_eq!(cstr1.to_str().unwrap(), "first");
        drop(buf1);

        set_board_name("second");
        let buf2 = BOARD_NAME.lock().unwrap();
        let cstr2 = CStr::from_bytes_until_nul(&buf2[..]).unwrap();
        assert_eq!(cstr2.to_str().unwrap(), "second");
    }
}
