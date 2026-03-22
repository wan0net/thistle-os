// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — OTA module
//
// Port of components/kernel/src/ota.c
// Applies firmware updates from the SD card using ESP-IDF OTA APIs.
// On simulator builds, ESP-IDF OTA calls are replaced with stubs.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_INVALID_CRC: i32 = 0x109;

const OTA_BUF_SIZE: usize = 4096;
const OTA_SD_UPDATE_PATH: &str = "/sdcard/update/thistle_os.bin\0";
const MAX_OTA_SIZE: u64 = 16 * 1024 * 1024; // 16 MB

static TAG: &[u8] = b"ota\0";

// ---------------------------------------------------------------------------
// C FFI — logging
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
    fn signing_verify_file(path: *const c_char) -> i32;
}

const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;
const ESP_LOG_ERROR: i32 = 1;

// ---------------------------------------------------------------------------
// ESP-IDF OTA FFI (hardware only)
// ---------------------------------------------------------------------------

/// ESP_OTA_IMG_PENDING_VERIFY state value from esp_ota_ops.h
///
/// Replaces the C `esp_ota_img_pending_verify()` helper shim in kernel_shims.c.
/// The constant value 0x107 matches ESP_OTA_IMG_PENDING_VERIFY in ESP-IDF v5.x.
#[cfg(target_os = "espidf")]
const ESP_OTA_IMG_PENDING_VERIFY: u32 = 0x107;

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_ota_get_running_partition() -> *const c_void;
    fn esp_ota_get_state_partition(partition: *const c_void, state: *mut u32) -> i32;
    fn esp_ota_mark_app_valid_cancel_rollback() -> i32;
    fn esp_ota_mark_app_invalid_rollback_and_reboot() -> i32;
    fn esp_ota_get_next_update_partition(label: *const c_char) -> *const c_void;
    fn esp_ota_begin(partition: *const c_void, image_size: usize, handle: *mut u32) -> i32;
    fn esp_ota_write(handle: u32, data: *const u8, size: usize) -> i32;
    fn esp_ota_end(handle: u32) -> i32;
    fn esp_ota_abort(handle: u32) -> i32;
    fn esp_ota_set_boot_partition(partition: *const c_void) -> i32;
    fn esp_restart() -> !;
}

// Progress callback type — matches C typedef `void (*ota_progress_cb_t)(uint32_t written, uint32_t total, void *user_data)`
pub type OtaProgressCb = unsafe extern "C" fn(written: u32, total: u32, user_data: *mut c_void);

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the OTA subsystem. Confirms the current OTA partition if it is
/// in PENDING_VERIFY state.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ota_init() -> i32 {
    #[cfg(target_os = "espidf")]
    unsafe {
        use std::os::raw::c_void;
        let running = esp_ota_get_running_partition();
        if !running.is_null() {
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Running OTA partition initialised\0".as_ptr(),
            );
        }

        let mut state: u32 = 0;
        if esp_ota_get_state_partition(running, &mut state) == ESP_OK {
            if state == ESP_OTA_IMG_PENDING_VERIFY {
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"Confirming OTA update (marking valid)\0".as_ptr(),
                );
                esp_ota_mark_app_valid_cancel_rollback();
            }
        }
    }

    unsafe {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"OTA subsystem initialized\0".as_ptr(),
        );
    }

    ESP_OK
}

/// Return true if a firmware update file exists on the SD card.
///
/// # Safety
/// May be called from C. Thread-safe (read-only filesystem check).
#[no_mangle]
pub extern "C" fn ota_sd_update_available() -> bool {
    let path = OTA_SD_UPDATE_PATH.trim_end_matches('\0');
    match std::fs::metadata(path) {
        Ok(m) => m.len() > 0,
        Err(_) => false,
    }
}

/// Apply a firmware OTA update from the SD card.
///
/// Verifies the signature, reads the file, and writes it to the next OTA
/// partition. Reboots on success.
///
/// # Safety
/// `progress_cb` may be NULL. `user_data` is passed through to the callback.
#[no_mangle]
pub unsafe extern "C" fn ota_apply_from_sd(
    progress_cb: Option<OtaProgressCb>,
    user_data: *mut c_void,
) -> i32 {
    let update_path = OTA_SD_UPDATE_PATH.trim_end_matches('\0');
    let update_path_cstr = OTA_SD_UPDATE_PATH.as_ptr() as *const c_char;

    // 1. Verify signature
    let sig_ret = signing_verify_file(update_path_cstr);
    if sig_ret == ESP_ERR_INVALID_CRC {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"OTA update signature INVALID\0".as_ptr(),
        );
        return ESP_ERR_INVALID_CRC;
    }

    // 2. Open and size-check the update file
    let mut file = match std::fs::File::open(update_path) {
        Ok(f) => f,
        Err(_) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Cannot open update file\0".as_ptr(),
            );
            return ESP_ERR_NOT_FOUND;
        }
    };

    use std::io::Read;
    let file_size = match std::fs::metadata(update_path) {
        Ok(m) => m.len(),
        Err(_) => return ESP_ERR_NOT_FOUND,
    };

    if file_size == 0 {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Update file is empty\0".as_ptr());
        return ESP_ERR_INVALID_SIZE;
    }
    if file_size > MAX_OTA_SIZE {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"OTA file too large\0".as_ptr());
        return ESP_ERR_INVALID_SIZE;
    }

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Applying OTA update from SD (%d bytes)\0".as_ptr(),
        file_size as i32,
    );

    #[cfg(target_os = "espidf")]
    {
        let update_partition = esp_ota_get_next_update_partition(std::ptr::null());
        if update_partition.is_null() {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"No OTA partition available\0".as_ptr());
            return ESP_ERR_NOT_FOUND;
        }

        let mut ota_handle: u32 = 0;
        let ret = esp_ota_begin(update_partition, file_size as usize, &mut ota_handle);
        if ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_ota_begin failed: %d\0".as_ptr(), ret);
            return ret;
        }

        let mut buf = vec![0u8; OTA_BUF_SIZE];
        let mut written: u32 = 0;

        loop {
            let to_read = OTA_BUF_SIZE.min((file_size - written as u64) as usize);
            if to_read == 0 { break; }

            let nread = match file.read(&mut buf[..to_read]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => {
                    esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"OTA read error\0".as_ptr());
                    esp_ota_abort(ota_handle);
                    return ESP_ERR_INVALID_SIZE;
                }
            };

            let ret = esp_ota_write(ota_handle, buf.as_ptr(), nread);
            if ret != ESP_OK {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_ota_write failed: %d\0".as_ptr(), ret);
                esp_ota_abort(ota_handle);
                return ret;
            }

            written += nread as u32;

            if let Some(cb) = progress_cb {
                cb(written, file_size as u32, user_data);
            }
        }

        let ret = esp_ota_end(ota_handle);
        if ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_ota_end failed: %d\0".as_ptr(), ret);
            return ret;
        }

        let ret = esp_ota_set_boot_partition(update_partition);
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"esp_ota_set_boot_partition failed: %d\0".as_ptr(),
                ret,
            );
            return ret;
        }

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"OTA update successful. Rebooting...\0".as_ptr(),
        );

        esp_restart();
    }

    #[cfg(not(target_os = "espidf"))]
    {
        // Simulator: consume file to validate, but don't actually flash
        let _ = file;
        let _ = progress_cb;
        let _ = user_data;
        esp_log_write(
            ESP_LOG_WARN,
            TAG.as_ptr(),
            b"OTA: simulator build - not applying\0".as_ptr(),
        );
        return ESP_ERR_NOT_SUPPORTED;
    }

    #[allow(unreachable_code)]
    ESP_OK
}

/// Apply a firmware OTA update from a URL (not yet implemented).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub unsafe extern "C" fn ota_apply_from_http(
    _url: *const c_char,
    _progress_cb: Option<OtaProgressCb>,
    _user_data: *mut c_void,
) -> i32 {
    esp_log_write(
        ESP_LOG_WARN,
        TAG.as_ptr(),
        b"HTTP OTA not yet implemented\0".as_ptr(),
    );
    ESP_ERR_NOT_SUPPORTED
}

/// Return the current firmware version string.
///
/// # Safety
/// Returns a pointer to a static C string. Do not free.
#[no_mangle]
pub extern "C" fn ota_get_current_version() -> *const c_char {
    // Matches THISTLE_VERSION_STRING from version.h
    b"0.1.0\0".as_ptr() as *const c_char
}

/// Return the label of the currently running OTA partition.
///
/// # Safety
/// Returns a pointer to a static C string (from ESP-IDF) or "unknown".
#[no_mangle]
pub extern "C" fn ota_get_running_partition() -> *const c_char {
    #[cfg(target_os = "espidf")]
    unsafe {
        let p = esp_ota_get_running_partition();
        if !p.is_null() {
            // The partition struct's label field is at a known offset (4 bytes).
            // We return a pointer to it directly — stable for the process lifetime.
            return (p as *const u8).add(4) as *const c_char;
        }
    }
    b"unknown\0".as_ptr() as *const c_char
}

/// Mark the current OTA partition as valid (cancel rollback).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ota_mark_valid() -> i32 {
    #[cfg(target_os = "espidf")]
    unsafe {
        return esp_ota_mark_app_valid_cancel_rollback();
    }
    #[cfg(not(target_os = "espidf"))]
    ESP_OK
}

/// Rollback to the previous OTA partition and reboot.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ota_rollback() -> i32 {
    #[cfg(target_os = "espidf")]
    unsafe {
        return esp_ota_mark_app_invalid_rollback_and_reboot();
    }
    #[cfg(not(target_os = "espidf"))]
    ESP_ERR_NOT_SUPPORTED
}

// ---------------------------------------------------------------------------
// Tests
//
// Only functions that are safe on aarch64-apple-darwin (no esp_log_write,
// no flash access) are tested here:
//   ota_get_current_version() — returns a static string
//   ota_sd_update_available()  — calls std::fs::metadata (safe on host)
//   ota_mark_valid()           — returns ESP_OK on non-espidf
//   ota_get_running_partition() — returns "unknown" on non-espidf
//   ota_rollback()             — returns ESP_ERR_NOT_SUPPORTED on non-espidf
//
// ota_init() and ota_apply_from_sd() are NOT tested: they call esp_log_write.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // -----------------------------------------------------------------------
    // test_get_current_version_non_null
    // Mirrors test_ota.c: ota_get_current_version() must return a non-null pointer.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_current_version_non_null() {
        let ptr = ota_get_current_version();
        assert!(!ptr.is_null(), "ota_get_current_version() must not return NULL");
    }

    // -----------------------------------------------------------------------
    // test_get_current_version_matches_expected
    // Mirrors test_ota.c: version string must match THISTLE_VERSION_STRING ("0.1.0").
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_current_version_matches_expected() {
        let ptr = ota_get_current_version();
        let version = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert_eq!(version, "0.1.0", "OTA version must be \"0.1.0\"");
    }

    // -----------------------------------------------------------------------
    // test_sd_update_available_false_when_no_card
    // Mirrors test_ota.c: without an SD card the update file does not exist.
    // -----------------------------------------------------------------------

    #[test]
    fn test_sd_update_available_false_when_no_card() {
        // In the test environment there is no SD card, so the update path
        // /sdcard/update/thistle_os.bin does not exist.
        let available = ota_sd_update_available();
        assert!(!available, "ota_sd_update_available() must return false when no SD card");
    }

    // -----------------------------------------------------------------------
    // test_mark_valid_returns_ok_on_host
    // ota_mark_valid() is a no-op stub on non-espidf; must return ESP_OK.
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_valid_returns_ok_on_host() {
        let rc = ota_mark_valid();
        assert_eq!(rc, ESP_OK, "ota_mark_valid() must return ESP_OK on host");
    }

    // -----------------------------------------------------------------------
    // test_get_running_partition_non_null
    // ota_get_running_partition() returns "unknown" on host builds.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_running_partition_non_null() {
        let ptr = ota_get_running_partition();
        assert!(!ptr.is_null(), "ota_get_running_partition() must not return NULL");
        let s = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert_eq!(s, "unknown", "partition must be \"unknown\" on host builds");
    }

    // -----------------------------------------------------------------------
    // test_rollback_not_supported_on_host
    // ota_rollback() returns ESP_ERR_NOT_SUPPORTED on non-espidf.
    // -----------------------------------------------------------------------

    #[test]
    fn test_rollback_not_supported_on_host() {
        let rc = ota_rollback();
        assert_eq!(
            rc, ESP_ERR_NOT_SUPPORTED,
            "ota_rollback() must return ESP_ERR_NOT_SUPPORTED on host"
        );
    }
}
