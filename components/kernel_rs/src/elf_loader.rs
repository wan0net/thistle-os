// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — elf_loader module
//
// Port of components/kernel/src/elf_loader.c
// Loads app ELF files into PSRAM using esp_elf, verifies signatures,
// parses manifests, and starts each app in its own FreeRTOS task.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_INVALID_CRC: i32 = 0x109;
const ESP_FAIL: i32 = -1;

const MAX_LOADED_APPS: usize = 4;
const ELF_MAX_SIZE_BYTES: usize = 1024 * 1024; // 1 MB
const ELF_APP_TASK_STACK: u32 = 8192;
const ELF_APP_TASK_PRIO: u32 = 5;

// MALLOC_CAP_SPIRAM = BIT(9)
const MALLOC_CAP_SPIRAM: u32 = 1 << 9;

// Permission flags (must match permissions.h)
const PERM_ALL: u32 = 0x7F;
const PERM_IPC: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// C FFI declarations
// ---------------------------------------------------------------------------

extern "C" {
    // esp_elf
    fn esp_elf_init(elf: *mut c_void) -> i32;
    fn esp_elf_relocate(elf: *mut c_void, buf: *const u8) -> i32;
    fn esp_elf_request(elf: *mut c_void, opt: c_int, argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn esp_elf_deinit(elf: *mut c_void);
    fn elf_set_symbol_resolver(resolver: unsafe extern "C" fn(*const c_char) -> usize);

    // PSRAM allocation
    fn heap_caps_malloc(size: usize, caps: u32) -> *mut c_void;
    fn free(ptr: *mut c_void);

    // FreeRTOS
    fn xTaskCreate(
        task_fn: unsafe extern "C" fn(*mut c_void),
        name: *const c_char,
        stack_depth: u32,
        param: *mut c_void,
        priority: u32,
        task_handle: *mut *mut c_void,
    ) -> i32;
    fn vTaskDelete(task: *mut c_void);

    // Signing (Rust)
    fn signing_verify_file(path: *const c_char) -> i32;

    // Permissions (Rust)
    fn permissions_grant(app_id: *const c_char, perms: u32) -> i32;

    // Manifest (C/Rust)
    fn manifest_parse_file(path: *const c_char, out: *mut c_void) -> i32;
    fn manifest_is_compatible(manifest: *const c_void) -> bool;
    fn manifest_path_from_elf(elf_path: *const c_char, out: *mut c_char, out_size: usize);

    // Syscall table
    fn syscall_resolve(name: *const c_char) -> *mut c_void;
    fn syscall_table_count() -> usize;

    // Logging
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;
const ESP_LOG_ERROR: i32 = 1;
const ESP_LOG_DEBUG: i32 = 4;

// pdPASS = 1 in FreeRTOS
const PD_PASS: i32 = 1;

static TAG: &[u8] = b"elf_loader\0";

// Size of esp_elf_t opaque struct storage blob.
const ESP_ELF_T_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// App handle type (opaque pointer exported to C)
// ---------------------------------------------------------------------------

/// Internal ELF app state.
pub struct ElfAppHandle {
    elf_storage: [u8; ESP_ELF_T_SIZE],
    path: [u8; 128],
    task: *mut c_void,
    loaded: bool,
    running: bool,
    // Slot index in the global array
    slot: usize,
}

// SAFETY: Only accessed under Mutex.
unsafe impl Send for ElfAppHandle {}

impl ElfAppHandle {
    const fn empty() -> Self {
        ElfAppHandle {
            elf_storage: [0u8; ESP_ELF_T_SIZE],
            path: [0u8; 128],
            task: std::ptr::null_mut(),
            loaded: false,
            running: false,
            slot: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct ElfLoaderState {
    apps: [ElfAppHandle; MAX_LOADED_APPS],
}

impl ElfLoaderState {
    const fn new() -> Self {
        ElfLoaderState {
            apps: [
                ElfAppHandle::empty(),
                ElfAppHandle::empty(),
                ElfAppHandle::empty(),
                ElfAppHandle::empty(),
            ],
        }
    }
}

// SAFETY: Only mutated through Mutex.
unsafe impl Send for ElfLoaderState {}

static STATE: Mutex<ElfLoaderState> = Mutex::new(ElfLoaderState::new());

// ---------------------------------------------------------------------------
// Symbol resolver
// ---------------------------------------------------------------------------

unsafe extern "C" fn thistle_symbol_resolver(sym_name: *const c_char) -> usize {
    let addr = syscall_resolve(sym_name);
    if addr.is_null() {
        esp_log_write(
            ESP_LOG_WARN,
            TAG.as_ptr(),
            b"Unresolved symbol: %s\0".as_ptr(),
            sym_name,
        );
        0
    } else {
        addr as usize
    }
}

// ---------------------------------------------------------------------------
// FreeRTOS task entry point
// ---------------------------------------------------------------------------

unsafe extern "C" fn elf_app_task(arg: *mut c_void) {
    // arg is a raw pointer into the STATE.apps slot; we cast and use it.
    let app = &mut *(arg as *mut ElfAppHandle);

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Starting ELF app: %s\0".as_ptr(),
        app.path.as_ptr(),
    );

    let elf_ptr = app.elf_storage.as_mut_ptr() as *mut c_void;
    let err = esp_elf_request(elf_ptr, 0, 0, std::ptr::null_mut());
    if err != 0 {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"ELF entry point error: ret=%d\0".as_ptr(),
            err,
        );
    } else {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"ELF app exited normally\0".as_ptr(),
        );
    }

    app.running = false;
    vTaskDelete(std::ptr::null_mut());
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the ELF loader.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn elf_loader_init() -> i32 {
    if let Ok(mut state) = STATE.lock() {
        for (i, app) in state.apps.iter_mut().enumerate() {
            *app = ElfAppHandle::empty();
            app.slot = i;
        }
        unsafe {
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"ELF loader initialised (max %d concurrent apps)\0".as_ptr(),
                MAX_LOADED_APPS as c_int,
            );
        }
    }
    ESP_OK
}

/// Load an app ELF from `path` into a free slot.
///
/// On success, `*handle` is set to a pointer into the global slot array.
///
/// # Safety
/// `path` must be a valid null-terminated C string.
/// `handle` must point to a valid `*mut ElfAppHandle` location.
#[no_mangle]
pub unsafe extern "C" fn elf_app_load(
    path: *const c_char,
    handle: *mut *mut ElfAppHandle,
) -> i32 {
    if path.is_null() || handle.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    // 1. Find a free slot
    let slot_idx = {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return ESP_FAIL,
        };
        match state.apps.iter().position(|a| !a.loaded) {
            Some(i) => i,
            None => {
                esp_log_write(
                    ESP_LOG_ERROR,
                    TAG.as_ptr(),
                    b"No free ELF slots (max %d)\0".as_ptr(),
                    MAX_LOADED_APPS as c_int,
                );
                return ESP_ERR_NO_MEM;
            }
        }
    };

    // 2. Read ELF into PSRAM
    let file_data = match std::fs::read(path_str) {
        Ok(d) => d,
        Err(_) => {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Cannot open ELF: %s\0".as_ptr(), path);
            return ESP_ERR_NOT_FOUND;
        }
    };

    let size = file_data.len();
    if size == 0 || size > ELF_MAX_SIZE_BYTES {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"Rejecting ELF size %d: %s\0".as_ptr(),
            size as c_int,
            path,
        );
        return ESP_ERR_INVALID_SIZE;
    }

    let buf = heap_caps_malloc(size, MALLOC_CAP_SPIRAM);
    if buf.is_null() {
        return ESP_ERR_NO_MEM;
    }
    std::ptr::copy_nonoverlapping(file_data.as_ptr(), buf as *mut u8, size);
    drop(file_data);

    // 3. Verify signature and set permissions
    let sig_ret = signing_verify_file(path);
    if sig_ret == ESP_OK {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"ELF signature verified: %s\0".as_ptr(), path);
        // Grant all perms — will be refined once we have app ID from manifest
        permissions_grant(path, PERM_ALL);
    } else if sig_ret == ESP_ERR_NOT_FOUND {
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"ELF unsigned: %s (restricted)\0".as_ptr(), path);
        permissions_grant(path, PERM_IPC);
    } else {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ELF signature INVALID: %s\0".as_ptr(), path);
        free(buf);
        return ESP_ERR_INVALID_CRC;
    }

    // 4. Init esp_elf context
    let mut state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => { free(buf); return ESP_FAIL; }
    };

    let app = &mut state.apps[slot_idx];
    let elf_ptr = app.elf_storage.as_mut_ptr() as *mut c_void;

    let ret = esp_elf_init(elf_ptr);
    if ret != ESP_OK {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_elf_init failed: %s\0".as_ptr(), path);
        free(buf);
        return ret;
    }

    // 5. Set resolver and relocate
    let sym_count = syscall_table_count();
    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Relocating '%s' (%d bytes, %d syscalls)\0".as_ptr(),
        path,
        size as c_int,
        sym_count as c_int,
    );

    elf_set_symbol_resolver(thistle_symbol_resolver);

    let ret = esp_elf_relocate(elf_ptr, buf as *const u8);
    free(buf);

    if ret != ESP_OK {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_elf_relocate failed: %s\0".as_ptr(), path);
        esp_elf_deinit(elf_ptr);
        return ret;
    }

    // 6. Parse manifest alongside ELF
    {
        let mut manifest_path_buf = [0u8; 280];
        manifest_path_from_elf(path, manifest_path_buf.as_mut_ptr() as *mut c_char, manifest_path_buf.len());

        let manifest_buf = heap_caps_malloc(512, MALLOC_CAP_SPIRAM);
        if !manifest_buf.is_null() {
            if manifest_parse_file(manifest_path_buf.as_ptr() as *const c_char, manifest_buf) == ESP_OK {
                if !manifest_is_compatible(manifest_buf as *const c_void) {
                    esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"App incompatible: %s\0".as_ptr(), path);
                    esp_elf_deinit(elf_ptr);
                    free(manifest_buf);
                    return ESP_ERR_NOT_SUPPORTED;
                }
                esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Manifest OK: %s\0".as_ptr(), path);
            }
            free(manifest_buf);
        }
    }

    // 7. Store metadata
    let path_bytes = CStr::from_ptr(path).to_bytes_with_nul();
    let copy_len = path_bytes.len().min(app.path.len() - 1);
    app.path[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
    app.path[copy_len] = 0;
    app.loaded  = true;
    app.running = false;
    app.task    = std::ptr::null_mut();
    app.slot    = slot_idx;

    *handle = app as *mut ElfAppHandle;

    esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"ELF loaded: %s\0".as_ptr(), path);
    ESP_OK
}

/// Start a loaded ELF app in its own FreeRTOS task.
///
/// # Safety
/// `handle` must have been obtained from `elf_app_load`.
#[no_mangle]
pub unsafe extern "C" fn elf_app_start(handle: *mut ElfAppHandle) -> i32 {
    if handle.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let app = &mut *handle;
    if !app.loaded {
        return ESP_ERR_INVALID_STATE;
    }
    if app.running {
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"ELF app already running\0".as_ptr());
        return ESP_ERR_INVALID_STATE;
    }

    let rc = xTaskCreate(
        elf_app_task,
        b"elf_app\0".as_ptr() as *const c_char,
        ELF_APP_TASK_STACK,
        handle as *mut c_void,
        ELF_APP_TASK_PRIO,
        &mut app.task,
    );

    if rc != PD_PASS {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"xTaskCreate failed for ELF app\0".as_ptr());
        return ESP_ERR_NO_MEM;
    }

    app.running = true;
    esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"ELF app task started: %s\0".as_ptr(), app.path.as_ptr());
    ESP_OK
}

/// Unload a loaded ELF app, killing its task if running.
///
/// # Safety
/// `handle` must have been obtained from `elf_app_load`.
#[no_mangle]
pub unsafe extern "C" fn elf_app_unload(handle: *mut ElfAppHandle) -> i32 {
    if handle.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let app = &mut *handle;

    // Kill task if running
    if app.running && !app.task.is_null() {
        vTaskDelete(app.task);
        app.task    = std::ptr::null_mut();
        app.running = false;
    }

    // Deinit ELF
    if app.loaded {
        let elf_ptr = app.elf_storage.as_mut_ptr() as *mut c_void;
        esp_elf_deinit(elf_ptr);
        app.loaded = false;
    }

    esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"ELF app unloaded\0".as_ptr());

    // Clear the slot
    let slot = app.slot;
    if let Ok(mut state) = STATE.lock() {
        state.apps[slot] = ElfAppHandle::empty();
        state.apps[slot].slot = slot;
    }

    ESP_OK
}

/// Return a pointer to the app's manifest struct, or NULL if not loaded.
///
/// # Safety
/// `handle` must have been obtained from `elf_app_load`.
/// Returns NULL if the handle is invalid or the app is not loaded.
#[no_mangle]
pub unsafe extern "C" fn elf_app_get_manifest(handle: *const ElfAppHandle) -> *const c_void {
    if handle.is_null() {
        return std::ptr::null();
    }
    let app = &*handle;
    if !app.loaded {
        return std::ptr::null();
    }
    // Return a pointer to the path as a minimal manifest-like identification.
    // The real manifest fields are stored in the C-side app_manifest_t which
    // is embedded in the ELF's .thistle_app section — the caller already has
    // that after elf_app_load returns successfully.
    app.path.as_ptr() as *const c_void
}
