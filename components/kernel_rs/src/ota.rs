// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — OTA module
//
// Port of components/kernel/src/ota.c
// Applies firmware updates from the SD card using ESP-IDF OTA APIs.
// On simulator builds, ESP-IDF OTA calls are replaced with stubs.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::sync::Mutex;

use crate::signing::verify_exact_reader_with;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_FAIL: i32 = -1;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;

const OTA_SD_UPDATE_PATH: &str = "/sdcard/update/thistle_os.bin\0";
const BUNDLE_TRANSACTION_PATH: &str = "/sdcard/.thistle-bundle-transaction";
const BUNDLE_DOWNLOAD_PATH: &str = "/sdcard/.thistle-bundle-download";
const MAX_OTA_SIZE: u64 = 16 * 1024 * 1024; // 16 MB

static TAG: &[u8] = b"ota\0";

// ---------------------------------------------------------------------------
// C FFI — logging
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO: i32 = 3;
const ESP_LOG_WARN: i32 = 2;
const ESP_LOG_ERROR: i32 = 1;

// ---------------------------------------------------------------------------
// ESP-IDF OTA FFI (hardware only)
// ---------------------------------------------------------------------------

/// ESP_OTA_IMG_PENDING_VERIFY state value from esp_ota_ops.h
///
/// Replaces the C `esp_ota_img_pending_verify()` helper shim in kernel_shims.c.
/// ESP-IDF defines this enum value in `esp_flash_partitions.h` as 0x1.
const ESP_OTA_IMG_PENDING_VERIFY: u32 = 0x1;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootValidationState {
    NotPending,
    Pending,
    Validated,
}

impl BootValidationState {
    fn observe_partition(&mut self, pending: bool) {
        *self = if pending {
            Self::Pending
        } else {
            Self::NotPending
        };
    }

    fn mark_valid_once_with<F>(&mut self, mark_valid: F) -> i32
    where
        F: FnOnce() -> i32,
    {
        if *self != Self::Pending {
            return ESP_OK;
        }

        let result = mark_valid();
        if result == ESP_OK {
            *self = Self::Validated;
        }
        result
    }
}

static BOOT_VALIDATION_STATE: Mutex<BootValidationState> =
    Mutex::new(BootValidationState::NotPending);

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the OTA subsystem and remember whether the running partition is
/// pending verification. Confirmation is deliberately deferred until the
/// explicit healthy-boot milestone calls `ota_mark_valid()`.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ota_init() -> i32 {
    let mut pending = false;

    #[cfg(target_os = "espidf")]
    unsafe {
        let running = esp_ota_get_running_partition();
        if !running.is_null() {
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Running OTA partition initialised\0".as_ptr(),
            );

            let mut state: u32 = 0;
            if esp_ota_get_state_partition(running, &mut state) == ESP_OK
                && state == ESP_OTA_IMG_PENDING_VERIFY
            {
                pending = true;
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"OTA update awaiting healthy-boot confirmation\0".as_ptr(),
                );
            }
        }
    }

    match BOOT_VALIDATION_STATE.lock() {
        Ok(mut state) => state.observe_partition(pending),
        Err(_) => return ESP_FAIL,
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
/// Opens the image once, streams those exact bytes through signature
/// verification and the inactive OTA partition, and reboots on success.
///
/// # Safety
/// `progress_cb` may be NULL. `user_data` is passed through to the callback.
#[no_mangle]
pub unsafe extern "C" fn ota_apply_from_sd(
    progress_cb: Option<OtaProgressCb>,
    user_data: *mut c_void,
) -> i32 {
    let update_path = OTA_SD_UPDATE_PATH.trim_end_matches('\0');

    // Keep one file-description boundary from signature verification through
    // flashing. The mutable pathname is never reopened.
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

    let file_size = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return ESP_ERR_NOT_FOUND,
    };

    if file_size == 0 {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"Update file is empty\0".as_ptr(),
        );
        return ESP_ERR_INVALID_SIZE;
    }
    if file_size > MAX_OTA_SIZE {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"OTA file too large\0".as_ptr(),
        );
        return ESP_ERR_INVALID_SIZE;
    }

    let signature = match std::fs::read(format!("{update_path}.sig")) {
        Ok(bytes) => bytes,
        Err(_) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Cannot open OTA signature\0".as_ptr(),
            );
            return ESP_ERR_NOT_FOUND;
        }
    };
    if signature.len() != 64 {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"OTA signature has invalid size\0".as_ptr(),
        );
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
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"No OTA partition available\0".as_ptr(),
            );
            return ESP_ERR_NOT_FOUND;
        }

        let mut ota_handle: u32 = 0;
        let ret = esp_ota_begin(update_partition, file_size as usize, &mut ota_handle);
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"esp_ota_begin failed: %d\0".as_ptr(),
                ret,
            );
            return ret;
        }

        let mut written: u32 = 0;
        let stream_result =
            verify_exact_reader_with(&mut file, file_size as usize, &signature, |chunk| {
                let ret = esp_ota_write(ota_handle, chunk.as_ptr(), chunk.len());
                if ret != ESP_OK {
                    return Err(ret);
                }
                written += chunk.len() as u32;
                if let Some(cb) = progress_cb {
                    cb(written, file_size as u32, user_data);
                }
                Ok(())
            });
        if let Err(error) = stream_result {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"OTA stream verification/write failed: %d\0".as_ptr(),
                error,
            );
            esp_ota_abort(ota_handle);
            return error;
        }

        let ret = esp_ota_end(ota_handle);
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"esp_ota_end failed: %d\0".as_ptr(),
                ret,
            );
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
        // Simulator: exercise the same one-open-object verification boundary.
        if let Err(error) =
            verify_exact_reader_with(&mut file, file_size as usize, &signature, |_| Ok(()))
        {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"OTA stream verification failed: %d\0".as_ptr(),
                error,
            );
            return error;
        }
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
    b"0.5.0\0".as_ptr() as *const c_char
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
    {
        return match BOOT_VALIDATION_STATE.lock() {
            Ok(mut state) => {
                state.mark_valid_once_with(|| unsafe { esp_ota_mark_app_valid_cancel_rollback() })
            }
            Err(_) => ESP_FAIL,
        };
    }
    #[cfg(not(target_os = "espidf"))]
    ESP_OK
}

fn finalize_bundle_transaction_at(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Remove rollback files only after the matching OTA image is healthy and valid.
/// Recovery can also finalize this journal on a later boot if SD cleanup fails.
#[no_mangle]
pub extern "C" fn ota_finalize_bundle_transaction() -> i32 {
    for path in [BUNDLE_TRANSACTION_PATH, BUNDLE_DOWNLOAD_PATH] {
        if finalize_bundle_transaction_at(Path::new(path)).is_err() {
            return ESP_FAIL;
        }
    }
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

    #[test]
    fn test_pending_image_is_not_marked_before_health_milestone() {
        let mut state = BootValidationState::NotPending;
        let mark_calls = std::cell::Cell::new(0);

        state.observe_partition(true);

        assert_eq!(state, BootValidationState::Pending);
        assert_eq!(mark_calls.get(), 0);
    }

    #[test]
    fn test_pending_verify_value_matches_esp_idf_abi() {
        assert_eq!(ESP_OTA_IMG_PENDING_VERIFY, 0x1);
    }

    #[test]
    fn test_healthy_boot_marks_pending_image_exactly_once() {
        let mut state = BootValidationState::NotPending;
        let mark_calls = std::cell::Cell::new(0);
        state.observe_partition(true);

        for _ in 0..2 {
            assert_eq!(
                state.mark_valid_once_with(|| {
                    mark_calls.set(mark_calls.get() + 1);
                    ESP_OK
                }),
                ESP_OK
            );
        }

        assert_eq!(state, BootValidationState::Validated);
        assert_eq!(mark_calls.get(), 1);
    }

    #[test]
    fn test_failed_confirmation_remains_pending_for_retry() {
        let mut state = BootValidationState::NotPending;
        state.observe_partition(true);

        assert_eq!(state.mark_valid_once_with(|| ESP_FAIL), ESP_FAIL);
        assert_eq!(state, BootValidationState::Pending);
    }

    // -----------------------------------------------------------------------
    // test_get_current_version_non_null
    // Mirrors test_ota.c: ota_get_current_version() must return a non-null pointer.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_current_version_non_null() {
        let ptr = ota_get_current_version();
        assert!(
            !ptr.is_null(),
            "ota_get_current_version() must not return NULL"
        );
    }

    // -----------------------------------------------------------------------
    // test_get_current_version_matches_expected
    // Mirrors test_ota.c: version string must match THISTLE_VERSION_STRING ("0.1.0").
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_current_version_matches_expected() {
        let ptr = ota_get_current_version();
        let version = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert_eq!(version, "0.5.0", "OTA version must match VERSION_STRING");
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
        assert!(
            !available,
            "ota_sd_update_available() must return false when no SD card"
        );
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

    #[test]
    fn test_bundle_transaction_cleanup_is_idempotent() {
        let root =
            std::env::temp_dir().join(format!("thistle-ota-finalize-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("backup")).unwrap();
        std::fs::write(root.join("journal"), b"transaction").unwrap();

        finalize_bundle_transaction_at(&root).unwrap();
        assert!(!root.exists());
        finalize_bundle_transaction_at(&root).unwrap();
    }

    // -----------------------------------------------------------------------
    // test_get_running_partition_non_null
    // ota_get_running_partition() returns "unknown" on host builds.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_running_partition_non_null() {
        let ptr = ota_get_running_partition();
        assert!(
            !ptr.is_null(),
            "ota_get_running_partition() must not return NULL"
        );
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
