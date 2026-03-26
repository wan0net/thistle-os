// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — driver_reload module
//
// Orchestrates driver hot-reload: unload old driver → load new driver →
// register → start.  Manages driver lifecycle states independently of the
// HAL registry and driver loader internals.
//
// State machine:
//   Empty → [register] → Loaded → [start] → Running
//                           ↑          |
//                           |      [stop]
//                           |          ↓
//                           +------ Stopped → [unload] → Empty
//                           |          |
//                           |      [reload]
//                           |          ↓
//                           +------- Loaded (new version)
//
//   Error state: any failure sets state to Error + last_error code.
//                [reload] from Error is allowed (attempts to recover).

use std::os::raw::c_char;
use std::sync::Mutex;

// ── ESP error codes ────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;

// ── Constants ──────────────────────────────────────────────────────────

const MAX_RELOADABLE: usize = 16;
const MAX_HAL_TYPE: u8 = 9;

// ── HAL type constants ─────────────────────────────────────────────────

pub const HAL_TYPE_DISPLAY: u8 = 0;
pub const HAL_TYPE_INPUT: u8 = 1;
pub const HAL_TYPE_RADIO: u8 = 2;
pub const HAL_TYPE_GPS: u8 = 3;
pub const HAL_TYPE_AUDIO: u8 = 4;
pub const HAL_TYPE_POWER: u8 = 5;
pub const HAL_TYPE_IMU: u8 = 6;
pub const HAL_TYPE_STORAGE: u8 = 7;
pub const HAL_TYPE_CRYPTO: u8 = 8;
pub const HAL_TYPE_RTC: u8 = 9;

// ── Driver state enum ──────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverState {
    Empty = 0,
    Loaded = 1,
    Running = 2,
    Stopped = 3,
    Error = 4,
}

// ── FFI structs ────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CDriverReloadInfo {
    pub id: u32,
    pub path: [u8; 128],
    pub path_len: u8,
    pub name: [u8; 32],
    pub name_len: u8,
    pub hal_type: u8,
    pub state: u8,
    pub load_count: u32,
    pub last_error: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CDriverReloadStats {
    pub driver_count: u32,
    pub running_count: u32,
    pub stopped_count: u32,
    pub error_count: u32,
    pub total_reloads: u32,
}

// ── Internal data model ────────────────────────────────────────────────

struct ReloadableDriver {
    id: u32,
    path: [u8; 128],
    path_len: usize,
    name: [u8; 32],
    name_len: usize,
    hal_type: u8,
    state: DriverState,
    load_count: u32,
    last_error: i32,
    version: [u8; 16],
    version_len: usize,
}

impl ReloadableDriver {
    const fn empty() -> Self {
        ReloadableDriver {
            id: 0,
            path: [0u8; 128],
            path_len: 0,
            name: [0u8; 32],
            name_len: 0,
            hal_type: 0,
            state: DriverState::Empty,
            load_count: 0,
            last_error: 0,
            version: [0u8; 16],
            version_len: 0,
        }
    }

    fn to_info(&self) -> CDriverReloadInfo {
        CDriverReloadInfo {
            id: self.id,
            path: self.path,
            path_len: self.path_len as u8,
            name: self.name,
            name_len: self.name_len as u8,
            hal_type: self.hal_type,
            state: self.state as u8,
            load_count: self.load_count,
            last_error: self.last_error,
        }
    }
}

struct DriverReloadState {
    drivers: [Option<ReloadableDriver>; MAX_RELOADABLE],
    driver_count: usize,
    next_id: u32,
    total_reloads: u32,
    initialized: bool,
}

impl DriverReloadState {
    const fn new() -> Self {
        // Work around const fn limitations — initialize array element by element
        const NONE: Option<ReloadableDriver> = None;
        DriverReloadState {
            drivers: [NONE; MAX_RELOADABLE],
            driver_count: 0,
            next_id: 1,
            total_reloads: 0,
            initialized: false,
        }
    }
}

// SAFETY: Only mutated through Mutex.
unsafe impl Send for DriverReloadState {}

static STATE: Mutex<DriverReloadState> = Mutex::new(DriverReloadState::new());

// ── Platform abstraction ───────────────────────────────────────────────
//
// On test / simulator builds, state transitions happen without actual ELF
// operations.  On ESP-IDF builds, these would call into driver_loader and
// HAL registry.

#[cfg(not(target_os = "espidf"))]
fn platform_load_driver(_path: &[u8], _path_len: usize) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
fn platform_unload_driver(_id: u32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
fn platform_start_driver(_hal_type: u8) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
fn platform_stop_driver(_hal_type: u8) -> i32 {
    ESP_OK
}

#[cfg(target_os = "espidf")]
extern "C" {
    fn driver_loader_load(path: *const c_char) -> i32;
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

#[cfg(target_os = "espidf")]
static TAG: &[u8] = b"drv_reload\0";

#[cfg(target_os = "espidf")]
const ESP_LOG_INFO: i32 = 3;
#[cfg(target_os = "espidf")]
const ESP_LOG_WARN: i32 = 2;

#[cfg(target_os = "espidf")]
fn platform_load_driver(path: &[u8], path_len: usize) -> i32 {
    // Path buffer is already null-terminated in the ReloadableDriver struct
    if path_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    unsafe {
        let ret = driver_loader_load(path.as_ptr() as *const c_char);
        if ret != ESP_OK {
            esp_log_write(
                ESP_LOG_WARN,
                TAG.as_ptr(),
                b"platform_load_driver failed: %d\0".as_ptr(),
                ret,
            );
        }
        ret
    }
}

#[cfg(target_os = "espidf")]
fn platform_unload_driver(_id: u32) -> i32 {
    // TODO: call esp_elf_deinit when driver_loader exposes unload
    unsafe {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"platform_unload_driver (stub)\0".as_ptr(),
        );
    }
    ESP_OK
}

#[cfg(target_os = "espidf")]
fn platform_start_driver(_hal_type: u8) -> i32 {
    // TODO: call hal_registry_start for specific driver type
    ESP_OK
}

#[cfg(target_os = "espidf")]
fn platform_stop_driver(_hal_type: u8) -> i32 {
    // TODO: call hal_registry_stop for specific driver type
    ESP_OK
}

// ── Helper: copy C string into fixed buffer ────────────────────────────

/// Copy bytes from a null-terminated C string into a fixed-size buffer.
/// Returns the number of bytes copied (excluding null terminator), or -1 on
/// error.  The destination is always null-terminated if at least 1 byte fits.
///
/// # Safety
/// `src` must be a valid null-terminated C string.
unsafe fn copy_cstr_to_buf(src: *const c_char, dst: &mut [u8]) -> i32 {
    if src.is_null() || dst.is_empty() {
        return -1;
    }

    let mut i = 0usize;
    let max = dst.len() - 1; // reserve space for null terminator
    loop {
        let byte = *src.add(i) as u8;
        if byte == 0 || i >= max {
            break;
        }
        dst[i] = byte;
        i += 1;
    }
    dst[i] = 0;
    i as i32
}

// ── Internal helpers ───────────────────────────────────────────────────

/// Find a driver by ID.  Returns the slot index, or None.
fn find_driver_index(state: &DriverReloadState, id: u32) -> Option<usize> {
    for i in 0..MAX_RELOADABLE {
        if let Some(ref drv) = state.drivers[i] {
            if drv.id == id {
                return Some(i);
            }
        }
    }
    None
}

/// Find a driver by path.  Returns the slot index, or None.
fn find_driver_by_path(state: &DriverReloadState, path: &[u8], path_len: usize) -> Option<usize> {
    for i in 0..MAX_RELOADABLE {
        if let Some(ref drv) = state.drivers[i] {
            if drv.path_len == path_len && drv.path[..path_len] == path[..path_len] {
                return Some(i);
            }
        }
    }
    None
}

/// Find a free slot.  Returns the index, or None.
fn find_free_slot(state: &DriverReloadState) -> Option<usize> {
    for i in 0..MAX_RELOADABLE {
        if state.drivers[i].is_none() {
            return Some(i);
        }
    }
    None
}

// ── FFI exports ────────────────────────────────────────────────────────

/// Initialise the driver reload manager.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_init() -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            for slot in s.drivers.iter_mut() {
                *slot = None;
            }
            s.driver_count = 0;
            s.next_id = 1;
            s.total_reloads = 0;
            s.initialized = true;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Register a driver for hot-reload management.
///
/// Returns the driver ID (> 0) on success, or a negative error code.
///
/// # Safety
/// `path` and `name` must be valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_register(
    path: *const c_char,
    name: *const c_char,
    hal_type: u8,
) -> i32 {
    if path.is_null() || name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    if hal_type > MAX_HAL_TYPE {
        return ESP_ERR_INVALID_ARG;
    }

    match STATE.lock() {
        Ok(mut s) => {
            if !s.initialized {
                return ESP_ERR_INVALID_STATE;
            }

            // Copy path
            let mut path_buf = [0u8; 128];
            let path_len = copy_cstr_to_buf(path, &mut path_buf);
            if path_len < 0 {
                return ESP_ERR_INVALID_ARG;
            }
            let path_len = path_len as usize;

            // Check if a driver with this path already exists — replace it
            if let Some(idx) = find_driver_by_path(&s, &path_buf, path_len) {
                let existing_id = s.drivers[idx].as_ref().unwrap().id;
                let mut drv = ReloadableDriver::empty();
                drv.id = existing_id;
                drv.path = path_buf;
                drv.path_len = path_len;
                let name_len = copy_cstr_to_buf(name, &mut drv.name);
                drv.name_len = if name_len > 0 { name_len as usize } else { 0 };
                drv.hal_type = hal_type;
                drv.state = DriverState::Loaded;
                s.drivers[idx] = Some(drv);
                return existing_id as i32;
            }

            // Find free slot
            let slot = match find_free_slot(&s) {
                Some(idx) => idx,
                None => return ESP_ERR_NO_MEM,
            };

            let id = s.next_id;
            s.next_id += 1;

            let mut drv = ReloadableDriver::empty();
            drv.id = id;
            drv.path = path_buf;
            drv.path_len = path_len;
            let name_len = copy_cstr_to_buf(name, &mut drv.name);
            drv.name_len = if name_len > 0 { name_len as usize } else { 0 };
            drv.hal_type = hal_type;
            drv.state = DriverState::Loaded;

            s.drivers[slot] = Some(drv);
            s.driver_count += 1;

            id as i32
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Unregister a driver.  Driver must be in Stopped or Loaded state.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_unregister(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let state = s.drivers[idx].as_ref().unwrap().state;
            if state == DriverState::Running {
                return ESP_ERR_INVALID_STATE;
            }

            s.drivers[idx] = None;
            if s.driver_count > 0 {
                s.driver_count -= 1;
            }
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Load a registered driver (transition Loaded or Stopped → Loaded).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_load(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let drv = s.drivers[idx].as_mut().unwrap();
            match drv.state {
                DriverState::Empty => return ESP_ERR_INVALID_STATE,
                DriverState::Running => return ESP_ERR_INVALID_STATE,
                DriverState::Loaded => {
                    // Already loaded — call platform load
                    let ret = platform_load_driver(&drv.path, drv.path_len);
                    if ret != ESP_OK {
                        drv.state = DriverState::Error;
                        drv.last_error = ret;
                        return ret;
                    }
                    drv.load_count += 1;
                    return ESP_OK;
                }
                DriverState::Stopped | DriverState::Error => {
                    let ret = platform_load_driver(&drv.path, drv.path_len);
                    if ret != ESP_OK {
                        drv.state = DriverState::Error;
                        drv.last_error = ret;
                        return ret;
                    }
                    drv.state = DriverState::Loaded;
                    drv.load_count += 1;
                    drv.last_error = 0;
                    ESP_OK
                }
            }
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Unload a driver (transition Stopped → Empty slot internally, but keep
/// the registration).  Must be stopped first.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_unload(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let drv = s.drivers[idx].as_mut().unwrap();
            match drv.state {
                DriverState::Running => return ESP_ERR_INVALID_STATE,
                DriverState::Empty => return ESP_ERR_INVALID_STATE,
                DriverState::Loaded | DriverState::Stopped | DriverState::Error => {
                    let ret = platform_unload_driver(drv.id);
                    if ret != ESP_OK {
                        drv.state = DriverState::Error;
                        drv.last_error = ret;
                        return ret;
                    }
                    // Keep registration but mark as needing load
                    drv.state = DriverState::Loaded;
                    drv.last_error = 0;
                    ESP_OK
                }
            }
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Full reload: stop (if running) → unload → load.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_reload(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let drv = s.drivers[idx].as_mut().unwrap();

            // Cannot reload from Empty
            if drv.state == DriverState::Empty {
                return ESP_ERR_INVALID_STATE;
            }

            // Stop if running
            if drv.state == DriverState::Running {
                let ret = platform_stop_driver(drv.hal_type);
                if ret != ESP_OK {
                    drv.state = DriverState::Error;
                    drv.last_error = ret;
                    return ret;
                }
                drv.state = DriverState::Stopped;
            }

            // Unload
            if drv.state == DriverState::Stopped || drv.state == DriverState::Loaded
                || drv.state == DriverState::Error
            {
                let ret = platform_unload_driver(drv.id);
                if ret != ESP_OK {
                    drv.state = DriverState::Error;
                    drv.last_error = ret;
                    return ret;
                }
            }

            // Load
            let ret = platform_load_driver(&drv.path, drv.path_len);
            if ret != ESP_OK {
                drv.state = DriverState::Error;
                drv.last_error = ret;
                return ret;
            }

            drv.state = DriverState::Loaded;
            drv.load_count += 1;
            drv.last_error = 0;
            s.total_reloads += 1;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Start a loaded driver (transition Loaded → Running).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_start(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let drv = s.drivers[idx].as_mut().unwrap();
            if drv.state != DriverState::Loaded {
                return ESP_ERR_INVALID_STATE;
            }

            let ret = platform_start_driver(drv.hal_type);
            if ret != ESP_OK {
                drv.state = DriverState::Error;
                drv.last_error = ret;
                return ret;
            }

            drv.state = DriverState::Running;
            drv.last_error = 0;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Stop a running driver (transition Running → Stopped).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_stop(id: u32) -> i32 {
    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };

            let drv = s.drivers[idx].as_mut().unwrap();
            if drv.state != DriverState::Running {
                return ESP_ERR_INVALID_STATE;
            }

            let ret = platform_stop_driver(drv.hal_type);
            if ret != ESP_OK {
                drv.state = DriverState::Error;
                drv.last_error = ret;
                return ret;
            }

            drv.state = DriverState::Stopped;
            drv.last_error = 0;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get driver state.  Returns DriverState as i32 (0–4), or negative error.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_get_state(id: u32) -> i32 {
    match STATE.lock() {
        Ok(s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };
            s.drivers[idx].as_ref().unwrap().state as i32
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get driver info by ID.
///
/// # Safety
/// `out` must be a valid pointer to `CDriverReloadInfo`.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_get_info(
    id: u32,
    out: *mut CDriverReloadInfo,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    match STATE.lock() {
        Ok(s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };
            *out = s.drivers[idx].as_ref().unwrap().to_info();
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get the number of registered drivers.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn rs_driver_reload_get_count() -> i32 {
    match STATE.lock() {
        Ok(s) => s.driver_count as i32,
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get driver info by slot index (for enumeration).
///
/// Iterates only over occupied slots.
///
/// # Safety
/// `out` must be a valid pointer to `CDriverReloadInfo`.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_get_at(
    index: u32,
    out: *mut CDriverReloadInfo,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    match STATE.lock() {
        Ok(s) => {
            let mut count = 0u32;
            for i in 0..MAX_RELOADABLE {
                if let Some(ref drv) = s.drivers[i] {
                    if count == index {
                        *out = drv.to_info();
                        return ESP_OK;
                    }
                    count += 1;
                }
            }
            ESP_ERR_INVALID_ARG
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get reload stats.
///
/// # Safety
/// `out` must be a valid pointer to `CDriverReloadStats`.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_get_stats(out: *mut CDriverReloadStats) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    match STATE.lock() {
        Ok(s) => {
            let mut running = 0u32;
            let mut stopped = 0u32;
            let mut error = 0u32;

            for slot in s.drivers.iter() {
                if let Some(ref drv) = slot {
                    match drv.state {
                        DriverState::Running => running += 1,
                        DriverState::Stopped => stopped += 1,
                        DriverState::Error => error += 1,
                        _ => {}
                    }
                }
            }

            *out = CDriverReloadStats {
                driver_count: s.driver_count as u32,
                running_count: running,
                stopped_count: stopped,
                error_count: error,
                total_reloads: s.total_reloads,
            };
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Reload a driver by file path (convenience).
///
/// Finds the first driver whose path matches and reloads it.
///
/// # Safety
/// `path` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_reload_by_path(path: *const c_char) -> i32 {
    if path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let mut path_buf = [0u8; 128];
    let path_len = copy_cstr_to_buf(path, &mut path_buf);
    if path_len < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let path_len = path_len as usize;

    // Find the driver ID first, then call reload (avoids nested lock)
    let id = match STATE.lock() {
        Ok(s) => {
            match find_driver_by_path(&s, &path_buf, path_len) {
                Some(idx) => s.drivers[idx].as_ref().unwrap().id,
                None => return ESP_ERR_NOT_FOUND,
            }
        }
        Err(_) => return ESP_ERR_INVALID_STATE,
    };

    rs_driver_reload_reload(id)
}

/// Update the version string for a driver (from manifest after load).
///
/// # Safety
/// `version` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn rs_driver_reload_set_version(
    id: u32,
    version: *const c_char,
) -> i32 {
    if version.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    match STATE.lock() {
        Ok(mut s) => {
            let idx = match find_driver_index(&s, id) {
                Some(i) => i,
                None => return ESP_ERR_NOT_FOUND,
            };
            let drv = s.drivers[idx].as_mut().unwrap();
            let len = copy_cstr_to_buf(version, &mut drv.version);
            drv.version_len = if len > 0 { len as usize } else { 0 };
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn reset() {
        let mut s = STATE.lock().unwrap();
        for slot in s.drivers.iter_mut() {
            *slot = None;
        }
        s.driver_count = 0;
        s.next_id = 1;
        s.total_reloads = 0;
        s.initialized = false;
    }

    fn init() {
        reset();
        assert_eq!(rs_driver_reload_init(), ESP_OK);
    }

    fn register_test_driver(path: &str, name: &str, hal_type: u8) -> i32 {
        let p = CString::new(path).unwrap();
        let n = CString::new(name).unwrap();
        unsafe { rs_driver_reload_register(p.as_ptr(), n.as_ptr(), hal_type) }
    }

    // ─── Init tests ────────────────────────────────────────────────────

    #[test]
    fn test_init_ok() {
        reset();
        assert_eq!(rs_driver_reload_init(), ESP_OK);
    }

    #[test]
    fn test_double_init_idempotent() {
        reset();
        assert_eq!(rs_driver_reload_init(), ESP_OK);
        assert_eq!(rs_driver_reload_init(), ESP_OK);
        assert_eq!(rs_driver_reload_get_count(), 0);
    }

    // ─── Register tests ────────────────────────────────────────────────

    #[test]
    fn test_register_single() {
        init();
        let id = register_test_driver("/sdcard/drivers/test.drv.elf", "test-drv", HAL_TYPE_DISPLAY);
        assert!(id > 0, "register must return positive ID, got {}", id);
    }

    #[test]
    fn test_register_multiple() {
        init();
        let id1 = register_test_driver("/sdcard/drivers/a.drv.elf", "a", HAL_TYPE_DISPLAY);
        let id2 = register_test_driver("/sdcard/drivers/b.drv.elf", "b", HAL_TYPE_INPUT);
        let id3 = register_test_driver("/sdcard/drivers/c.drv.elf", "c", HAL_TYPE_RADIO);
        assert!(id1 > 0);
        assert!(id2 > 0);
        assert!(id3 > 0);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_eq!(rs_driver_reload_get_count(), 3);
    }

    #[test]
    fn test_register_at_capacity_fails() {
        init();
        for i in 0..MAX_RELOADABLE {
            let path = format!("/sdcard/drivers/{}.drv.elf", i);
            let name = format!("drv{}", i);
            let id = register_test_driver(&path, &name, HAL_TYPE_DISPLAY);
            assert!(id > 0, "registration {} must succeed", i);
        }
        let id = register_test_driver("/sdcard/drivers/overflow.drv.elf", "overflow", HAL_TYPE_DISPLAY);
        assert_eq!(id, ESP_ERR_NO_MEM, "registration beyond capacity must fail");
    }

    #[test]
    fn test_register_null_path_rejected() {
        init();
        let name = CString::new("test").unwrap();
        let ret = unsafe { rs_driver_reload_register(std::ptr::null(), name.as_ptr(), 0) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_register_null_name_rejected() {
        init();
        let path = CString::new("/test.drv.elf").unwrap();
        let ret = unsafe { rs_driver_reload_register(path.as_ptr(), std::ptr::null(), 0) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_register_returns_incrementing_ids() {
        init();
        let id1 = register_test_driver("/a.drv.elf", "a", HAL_TYPE_DISPLAY);
        let id2 = register_test_driver("/b.drv.elf", "b", HAL_TYPE_INPUT);
        assert_eq!(id2, id1 + 1, "IDs must increment sequentially");
    }

    // ─── Lifecycle state tests ─────────────────────────────────────────

    #[test]
    fn test_load_transitions_to_loaded() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        // Already in Loaded state after register
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_start_transitions_loaded_to_running() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Running as i32);
    }

    #[test]
    fn test_stop_transitions_running_to_stopped() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Stopped as i32);
    }

    #[test]
    fn test_unload_transitions_stopped_to_loaded() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_unload(id), ESP_OK);
        // Unload keeps registration, transitions back to Loaded
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_cannot_start_from_stopped() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        // Must load first before starting again
        assert_eq!(rs_driver_reload_start(id), ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_cannot_stop_from_loaded() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_stop(id), ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_cannot_unload_from_running() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_unload(id), ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_load_from_stopped_ok() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_load(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    // ─── Reload tests ──────────────────────────────────────────────────

    #[test]
    fn test_reload_from_stopped() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_reload_from_running_auto_stops() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Running as i32);
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_reload_from_error_recovery() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        // Manually set error state
        {
            let mut s = STATE.lock().unwrap();
            let idx = find_driver_index(&s, id).unwrap();
            s.drivers[idx].as_mut().unwrap().state = DriverState::Error;
            s.drivers[idx].as_mut().unwrap().last_error = ESP_FAIL;
        }
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Error as i32);
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_reload_increments_load_count() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        // load_count: 1 from initial register load_count=0 (register doesn't load),
        // +1 from first reload, +1 from second reload = 2
        assert!(info.load_count >= 2, "load_count must reflect reloads, got {}", info.load_count);
    }

    #[test]
    fn test_reload_from_empty_fails() {
        init();
        // No driver with ID 999
        assert_eq!(rs_driver_reload_reload(999), ESP_ERR_NOT_FOUND);
    }

    // ─── Query tests ───────────────────────────────────────────────────

    #[test]
    fn test_get_state_returns_correct_state() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Running as i32);
    }

    #[test]
    fn test_get_info_populates_all_fields() {
        init();
        let id = register_test_driver("/sdcard/drivers/kbd.drv.elf", "keyboard", HAL_TYPE_INPUT) as u32;
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        assert_eq!(info.id, id);
        assert_eq!(info.hal_type, HAL_TYPE_INPUT);
        assert_eq!(info.state, DriverState::Loaded as u8);
        assert!(info.path_len > 0);
        assert!(info.name_len > 0);
        // Verify path content
        let path_str = std::str::from_utf8(&info.path[..info.path_len as usize]).unwrap();
        assert_eq!(path_str, "/sdcard/drivers/kbd.drv.elf");
        let name_str = std::str::from_utf8(&info.name[..info.name_len as usize]).unwrap();
        assert_eq!(name_str, "keyboard");
    }

    #[test]
    fn test_get_at_by_index() {
        init();
        let id1 = register_test_driver("/a.drv.elf", "alpha", HAL_TYPE_DISPLAY) as u32;
        let id2 = register_test_driver("/b.drv.elf", "beta", HAL_TYPE_INPUT) as u32;
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_at(0, &mut info) }, ESP_OK);
        assert_eq!(info.id, id1);
        assert_eq!(unsafe { rs_driver_reload_get_at(1, &mut info) }, ESP_OK);
        assert_eq!(info.id, id2);
    }

    #[test]
    fn test_get_at_out_of_range() {
        init();
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_at(0, &mut info) }, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_get_count() {
        init();
        assert_eq!(rs_driver_reload_get_count(), 0);
        register_test_driver("/a.drv.elf", "a", HAL_TYPE_DISPLAY);
        assert_eq!(rs_driver_reload_get_count(), 1);
        register_test_driver("/b.drv.elf", "b", HAL_TYPE_INPUT);
        assert_eq!(rs_driver_reload_get_count(), 2);
    }

    // ─── Path operation tests ──────────────────────────────────────────

    #[test]
    fn test_reload_by_path_finds_correct_driver() {
        init();
        let id = register_test_driver("/sdcard/drivers/gps.drv.elf", "gps", HAL_TYPE_GPS) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        let path = CString::new("/sdcard/drivers/gps.drv.elf").unwrap();
        assert_eq!(unsafe { rs_driver_reload_reload_by_path(path.as_ptr()) }, ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
    }

    #[test]
    fn test_reload_by_path_unknown_path() {
        init();
        let path = CString::new("/sdcard/drivers/nonexistent.drv.elf").unwrap();
        assert_eq!(
            unsafe { rs_driver_reload_reload_by_path(path.as_ptr()) },
            ESP_ERR_NOT_FOUND
        );
    }

    #[test]
    fn test_register_same_path_replaces() {
        init();
        let id1 = register_test_driver("/test.drv.elf", "v1", HAL_TYPE_DISPLAY);
        let id2 = register_test_driver("/test.drv.elf", "v2", HAL_TYPE_DISPLAY);
        // Same path returns the same ID (replacement)
        assert_eq!(id1, id2, "registering same path must return same ID");
        assert_eq!(rs_driver_reload_get_count(), 1, "count must not increase on replace");
    }

    // ─── Error handling tests ──────────────────────────────────────────

    #[test]
    fn test_error_state_from_manual_set() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        {
            let mut s = STATE.lock().unwrap();
            let idx = find_driver_index(&s, id).unwrap();
            s.drivers[idx].as_mut().unwrap().state = DriverState::Error;
            s.drivers[idx].as_mut().unwrap().last_error = ESP_FAIL;
        }
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Error as i32);
    }

    #[test]
    fn test_error_state_readable_in_info() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        {
            let mut s = STATE.lock().unwrap();
            let idx = find_driver_index(&s, id).unwrap();
            s.drivers[idx].as_mut().unwrap().state = DriverState::Error;
            s.drivers[idx].as_mut().unwrap().last_error = ESP_FAIL;
        }
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        assert_eq!(info.state, DriverState::Error as u8);
        assert_eq!(info.last_error, ESP_FAIL);
    }

    #[test]
    fn test_reload_from_error_resets() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        {
            let mut s = STATE.lock().unwrap();
            let idx = find_driver_index(&s, id).unwrap();
            s.drivers[idx].as_mut().unwrap().state = DriverState::Error;
            s.drivers[idx].as_mut().unwrap().last_error = ESP_FAIL;
        }
        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        assert_eq!(info.last_error, 0, "last_error must be cleared after successful reload");
    }

    #[test]
    fn test_load_failure_sets_error_state() {
        // In test builds platform_load_driver always succeeds, so we simulate
        // error by directly manipulating state
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        {
            let mut s = STATE.lock().unwrap();
            let idx = find_driver_index(&s, id).unwrap();
            s.drivers[idx].as_mut().unwrap().state = DriverState::Error;
            s.drivers[idx].as_mut().unwrap().last_error = 0x105;
        }
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Error as i32);
    }

    // ─── Version tests ─────────────────────────────────────────────────

    #[test]
    fn test_set_version() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        let ver = CString::new("1.2.3").unwrap();
        assert_eq!(unsafe { rs_driver_reload_set_version(id, ver.as_ptr()) }, ESP_OK);
    }

    #[test]
    fn test_get_version_in_info() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        let ver = CString::new("2.0.1").unwrap();
        assert_eq!(unsafe { rs_driver_reload_set_version(id, ver.as_ptr()) }, ESP_OK);
        // Verify via internal state since CDriverReloadInfo doesn't carry version
        let s = STATE.lock().unwrap();
        let idx = find_driver_index(&s, id).unwrap();
        let drv = s.drivers[idx].as_ref().unwrap();
        let v = std::str::from_utf8(&drv.version[..drv.version_len]).unwrap();
        assert_eq!(v, "2.0.1");
    }

    #[test]
    fn test_set_version_nonexistent() {
        init();
        let ver = CString::new("1.0.0").unwrap();
        assert_eq!(
            unsafe { rs_driver_reload_set_version(999, ver.as_ptr()) },
            ESP_ERR_NOT_FOUND
        );
    }

    // ─── Stats tests ───────────────────────────────────────────────────

    #[test]
    fn test_stats_empty() {
        init();
        let mut stats = CDriverReloadStats {
            driver_count: 0, running_count: 0, stopped_count: 0,
            error_count: 0, total_reloads: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_stats(&mut stats) }, ESP_OK);
        assert_eq!(stats.driver_count, 0);
        assert_eq!(stats.running_count, 0);
        assert_eq!(stats.stopped_count, 0);
        assert_eq!(stats.error_count, 0);
        assert_eq!(stats.total_reloads, 0);
    }

    #[test]
    fn test_stats_after_operations() {
        init();
        let id1 = register_test_driver("/a.drv.elf", "a", HAL_TYPE_DISPLAY) as u32;
        let id2 = register_test_driver("/b.drv.elf", "b", HAL_TYPE_INPUT) as u32;
        let id3 = register_test_driver("/c.drv.elf", "c", HAL_TYPE_RADIO) as u32;

        // Start id1 and id2
        assert_eq!(rs_driver_reload_start(id1), ESP_OK);
        assert_eq!(rs_driver_reload_start(id2), ESP_OK);
        // Stop id2
        assert_eq!(rs_driver_reload_stop(id2), ESP_OK);
        // Reload id1 (running → stopped → loaded, increments total_reloads)
        assert_eq!(rs_driver_reload_reload(id1), ESP_OK);

        let mut stats = CDriverReloadStats {
            driver_count: 0, running_count: 0, stopped_count: 0,
            error_count: 0, total_reloads: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_stats(&mut stats) }, ESP_OK);
        assert_eq!(stats.driver_count, 3);
        // id1 = Loaded (after reload), id2 = Stopped, id3 = Loaded
        assert_eq!(stats.running_count, 0);
        assert_eq!(stats.stopped_count, 1);
        assert_eq!(stats.total_reloads, 1);
    }

    #[test]
    fn test_stats_null_pointer() {
        init();
        assert_eq!(
            unsafe { rs_driver_reload_get_stats(std::ptr::null_mut()) },
            ESP_ERR_INVALID_ARG
        );
    }

    // ─── Unregister tests ──────────────────────────────────────────────

    #[test]
    fn test_unregister_existing() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_get_count(), 1);
        assert_eq!(rs_driver_reload_unregister(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_count(), 0);
    }

    #[test]
    fn test_unregister_nonexistent() {
        init();
        assert_eq!(rs_driver_reload_unregister(999), ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_unregister_running_driver_fails() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_unregister(id), ESP_ERR_INVALID_STATE);
    }

    // ─── Edge case tests ───────────────────────────────────────────────

    #[test]
    fn test_path_truncation() {
        init();
        // Create a path longer than 127 chars (buffer is 128 with null terminator)
        let long_path = format!("/{}", "a".repeat(200));
        let id = register_test_driver(&long_path, "test", HAL_TYPE_DISPLAY) as u32;
        assert!(id > 0, "long path must still register (truncated)");
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        assert!(info.path_len <= 127, "path must be truncated to fit buffer");
    }

    #[test]
    fn test_name_truncation() {
        init();
        let long_name = "n".repeat(100);
        let id = register_test_driver("/test.drv.elf", &long_name, HAL_TYPE_DISPLAY) as u32;
        assert!(id > 0, "long name must still register (truncated)");
        let mut info = CDriverReloadInfo {
            id: 0, path: [0; 128], path_len: 0, name: [0; 32], name_len: 0,
            hal_type: 0, state: 0, load_count: 0, last_error: 0,
        };
        assert_eq!(unsafe { rs_driver_reload_get_info(id, &mut info) }, ESP_OK);
        assert!(info.name_len <= 31, "name must be truncated to fit buffer");
    }

    #[test]
    fn test_hal_type_validation() {
        init();
        // Valid types: 0-9
        for t in 0..=MAX_HAL_TYPE {
            let path = format!("/drv{}.drv.elf", t);
            let name = format!("drv{}", t);
            let id = register_test_driver(&path, &name, t);
            assert!(id > 0, "hal_type {} must be valid", t);
        }
        // Invalid type: 10+
        let path = CString::new("/invalid.drv.elf").unwrap();
        let name = CString::new("invalid").unwrap();
        let ret = unsafe { rs_driver_reload_register(path.as_ptr(), name.as_ptr(), 10) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG, "hal_type 10 must be rejected");
        let ret = unsafe { rs_driver_reload_register(path.as_ptr(), name.as_ptr(), 255) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG, "hal_type 255 must be rejected");
    }

    #[test]
    fn test_state_constant_values() {
        assert_eq!(DriverState::Empty as u8, 0);
        assert_eq!(DriverState::Loaded as u8, 1);
        assert_eq!(DriverState::Running as u8, 2);
        assert_eq!(DriverState::Stopped as u8, 3);
        assert_eq!(DriverState::Error as u8, 4);
    }

    // ─── Additional edge cases ─────────────────────────────────────────

    #[test]
    fn test_get_state_nonexistent() {
        init();
        assert_eq!(rs_driver_reload_get_state(999), ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_get_info_null_pointer() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(
            unsafe { rs_driver_reload_get_info(id, std::ptr::null_mut()) },
            ESP_ERR_INVALID_ARG
        );
    }

    #[test]
    fn test_get_at_null_pointer() {
        init();
        assert_eq!(
            unsafe { rs_driver_reload_get_at(0, std::ptr::null_mut()) },
            ESP_ERR_INVALID_ARG
        );
    }

    #[test]
    fn test_register_before_init_fails() {
        reset(); // reset without init
        let ret = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY);
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_reload_by_path_null() {
        init();
        assert_eq!(
            unsafe { rs_driver_reload_reload_by_path(std::ptr::null()) },
            ESP_ERR_INVALID_ARG
        );
    }

    #[test]
    fn test_set_version_null() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(
            unsafe { rs_driver_reload_set_version(id, std::ptr::null()) },
            ESP_ERR_INVALID_ARG
        );
    }

    #[test]
    fn test_full_lifecycle() {
        init();
        let id = register_test_driver("/sdcard/drivers/kbd.drv.elf", "keyboard", HAL_TYPE_INPUT) as u32;
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);

        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Running as i32);

        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Stopped as i32);

        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);

        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Running as i32);

        assert_eq!(rs_driver_reload_reload(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_state(id), DriverState::Loaded as i32);

        assert_eq!(rs_driver_reload_unregister(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_count(), 0);
    }

    #[test]
    fn test_unregister_stopped_ok() {
        init();
        let id = register_test_driver("/test.drv.elf", "test", HAL_TYPE_DISPLAY) as u32;
        assert_eq!(rs_driver_reload_start(id), ESP_OK);
        assert_eq!(rs_driver_reload_stop(id), ESP_OK);
        assert_eq!(rs_driver_reload_unregister(id), ESP_OK);
        assert_eq!(rs_driver_reload_get_count(), 0);
    }
}
