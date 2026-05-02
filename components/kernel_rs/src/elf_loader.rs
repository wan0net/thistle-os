// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — elf_loader module
//
// Port of components/kernel/src/elf_loader.c
// Loads app ELF files into PSRAM using esp_elf, verifies signatures,
// parses manifests, and starts each app in its own FreeRTOS task.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

use crate::app_manager::{CAppEntry, CAppManifest};
use crate::ffi::CManifest;

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

const MAX_LOADED_APPS: usize = 16;
const ELF_MAX_SIZE_BYTES: usize = 1024 * 1024; // 1 MB
const ELF_APP_TASK_STACK: u32 = 8192;
const ELF_APP_TASK_PRIO: u32 = 5;

// MALLOC_CAP_SPIRAM = BIT(9)
const MALLOC_CAP_SPIRAM: u32 = 1 << 9;

// Permission flags (must match permissions.h)
const PERM_ALL: u32 = 0x7F;
const PERM_IPC: u32 = 1 << 6;

// Current architecture string for manifest compatibility checks
static CURRENT_ARCH: &[u8] = b"xtensa-esp32s3\0";

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
    fn xTaskCreatePinnedToCore(
        task_fn: unsafe extern "C" fn(*mut c_void),
        name: *const c_char,
        stack_depth: u32,
        param: *mut c_void,
        priority: u32,
        task_handle: *mut *mut c_void,
        core_id: i32,
    ) -> i32;
    fn vTaskDelete(task: *mut c_void);

    // Signing (Rust)
    fn signing_verify(data: *const u8, data_len: usize, signature: *const u8) -> i32;

    // Permissions (Rust)
    fn permissions_grant(app_id: *const c_char, perms: u32) -> i32;

    // Manifest (C/Rust)
    fn manifest_parse_file(path: *const c_char, out: *mut c_void) -> i32;
    fn manifest_is_compatible(manifest: *const c_void, current_arch: *const c_char) -> bool;
    fn manifest_path_from_elf(elf_path: *const c_char, out: *mut c_char, out_size: usize);

    // Syscall table
    fn syscall_resolve(name: *const c_char) -> *mut c_void;
    fn syscall_table_count() -> usize;
    fn syscall_set_current_app(id: *const c_char);

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
    load_addr: usize,
    load_size: usize,
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
            load_addr: 0,
            load_size: 0,
            slot: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct ElfLoaderState {
    apps: Option<Box<[ElfAppHandle; MAX_LOADED_APPS]>>,
}

impl ElfLoaderState {
    const fn new() -> Self {
        ElfLoaderState {
            apps: None,
        }
    }

    fn ensure_apps(&mut self) -> &mut [ElfAppHandle; MAX_LOADED_APPS] {
        if self.apps.is_none() {
            self.apps = Some(Box::new([
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
                ElfAppHandle::empty(), ElfAppHandle::empty(),
            ]));
        }
        self.apps.as_mut().unwrap()
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
        let apps = state.ensure_apps();
        for (i, app) in apps.iter_mut().enumerate() {
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
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return ESP_FAIL,
        };
        let apps = state.ensure_apps();
        match apps.iter().position(|a| !a.loaded) {
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

    // Update the app slot with the load address and size
    {
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => { free(buf); return ESP_FAIL; }
        };
        let app = &mut state.ensure_apps()[slot_idx];
        app.load_addr = buf as usize;
        app.load_size = size;
    }

    // 3. Parse manifest FIRST to get the app ID for permission identity
    let mut app_id_buf = [0u8; 64];
    let mut has_manifest_id = false;
    {
        let mut manifest_path_buf = [0u8; 280];
        manifest_path_from_elf(path, manifest_path_buf.as_mut_ptr() as *mut c_char, manifest_path_buf.len());
        let manifest_buf = heap_caps_malloc(512, MALLOC_CAP_SPIRAM);
        if !manifest_buf.is_null() {
            if manifest_parse_file(manifest_path_buf.as_ptr() as *const c_char, manifest_buf) == ESP_OK {
                // Extract the "id" field from the parsed manifest
                let id_ptr = (manifest_buf as *const u8).add(1); // type is first byte, id starts at offset 1
                // The CManifest struct has id at offset 1 (after u8 type), as [u8; 64]
                let id_bytes = std::slice::from_raw_parts(id_ptr, 64);
                let id_len = id_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                if id_len > 0 {
                    app_id_buf[..id_len].copy_from_slice(&id_bytes[..id_len]);
                    has_manifest_id = true;
                }
            }
            free(manifest_buf);
        }
    }

    // Use manifest ID for permissions if available, otherwise fall back to path
    let perm_id = if has_manifest_id {
        app_id_buf.as_ptr() as *const c_char
    } else {
        path
    };

    // 4. Verify signature on in-memory ELF data (avoids TOCTOU — no re-read from disk)
    let sig_ret = {
        let sig_path = format!("{}.sig", path_str);
        match std::fs::read(&sig_path) {
            Ok(sig_bytes) if sig_bytes.len() == 64 => {
                signing_verify(buf as *const u8, size, sig_bytes.as_ptr())
            }
            Ok(_) => ESP_ERR_INVALID_SIZE,
            Err(_) => ESP_ERR_NOT_FOUND,
        }
    };
    if sig_ret == ESP_OK {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"ELF signature verified: %s\0".as_ptr(), path);
        permissions_grant(perm_id, PERM_ALL);
    } else if sig_ret == ESP_ERR_NOT_FOUND {
        // Unsigned ELF — refuse in production, allow with zero permissions in debug
        #[cfg(not(debug_assertions))]
        {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ELF unsigned: %s (REFUSED - production)\0".as_ptr(), path);
            free(buf);
            return ESP_ERR_INVALID_CRC;
        }
        #[cfg(debug_assertions)]
        {
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"ELF unsigned: %s (dev mode, no permissions)\0".as_ptr(), path);
            // Zero permissions — unsigned apps cannot access any resources
        }
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

    let app = &mut state.ensure_apps()[slot_idx];
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

    syscall_set_current_app(perm_id);
    let ret = esp_elf_relocate(elf_ptr, buf as *const u8);
    syscall_set_current_app(std::ptr::null());

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
                if !manifest_is_compatible(manifest_buf as *const c_void, CURRENT_ARCH.as_ptr() as *const c_char) {
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

    let rc = xTaskCreatePinnedToCore(
        elf_app_task,
        b"elf_app\0".as_ptr() as *const c_char,
        ELF_APP_TASK_STACK,
        handle as *mut c_void,
        ELF_APP_TASK_PRIO,
        &mut app.task,
        -1,  // tskNO_AFFINITY
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
        let apps = state.ensure_apps();
        apps[slot] = ElfAppHandle::empty();
        apps[slot].slot = slot;
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

// ---------------------------------------------------------------------------
// Scan & Register — wires ELF apps into the app manager
// ---------------------------------------------------------------------------

/// Static storage for app registration metadata.
/// The app manager holds raw pointers into these, so they must be 'static.
struct ElfAppRegistration {
    manifest: CAppManifest,
    entry: CAppEntry,
    handle: *mut ElfAppHandle,
    used: bool,
}

impl ElfAppRegistration {
    const fn empty() -> Self {
        ElfAppRegistration {
            manifest: CAppManifest {
                id:               std::ptr::null(),
                name:             std::ptr::null(),
                version:          b"0.0.0\0".as_ptr() as *const c_char,
                allow_background: false,
                min_memory_kb:    0,
            },
            entry: CAppEntry {
                on_create:  None,
                on_start:   None,
                on_pause:   None,
                on_resume:  None,
                on_destroy: None,
                manifest:   std::ptr::null(),
            },
            handle: std::ptr::null_mut(),
            used: false,
        }
    }
}

// SAFETY: Only accessed under REG_MUTEX.
unsafe impl Send for ElfAppRegistration {}

static REG_MUTEX: Mutex<Option<Box<[ElfAppRegistration; MAX_LOADED_APPS]>>> = Mutex::new(None);

fn ensure_regs(guard: &mut Option<Box<[ElfAppRegistration; MAX_LOADED_APPS]>>) -> &mut [ElfAppRegistration; MAX_LOADED_APPS] {
    if guard.is_none() {
        *guard = Some(Box::new([
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
            ElfAppRegistration::empty(), ElfAppRegistration::empty(),
        ]));
    }
    guard.as_mut().unwrap()
}

/// Static string storage for manifest id/name fields.
/// Each registration slot gets its own id and name buffer that lives forever.
struct RegStrings {
    id:   [u8; 64],
    name: [u8; 32],
}

impl RegStrings {
    const fn empty() -> Self {
        RegStrings {
            id:   [0u8; 64],
            name: [0u8; 32],
        }
    }
}

// SAFETY: Only accessed under REG_STR_MUTEX.
unsafe impl Send for RegStrings {}

static REG_STR_MUTEX: Mutex<Option<Box<[RegStrings; MAX_LOADED_APPS]>>> = Mutex::new(None);

fn ensure_reg_strings(guard: &mut Option<Box<[RegStrings; MAX_LOADED_APPS]>>) -> &mut [RegStrings; MAX_LOADED_APPS] {
    if guard.is_none() {
        *guard = Some(Box::new([
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
            RegStrings::empty(), RegStrings::empty(),
        ]));
    }
    guard.as_mut().unwrap()
}

/// Per-slot on_create callbacks. We generate one per slot so the app manager
/// can invoke it without arguments and we know which ELF handle to start.
macro_rules! make_on_create {
    ($fn_name:ident, $slot:expr) => {
        unsafe extern "C" fn $fn_name() -> i32 {
            let handle = {
                match REG_MUTEX.lock() {
                    Ok(regs) => {
                        match regs.as_ref() {
                            Some(r) => r[$slot].handle,
                            None => return ESP_ERR_INVALID_STATE,
                        }
                    }
                    Err(_) => return ESP_FAIL,
                }
            };
            if handle.is_null() {
                return ESP_ERR_INVALID_STATE;
            }
            elf_app_start(handle)
        }
    };
}

make_on_create!(on_create_slot_0,  0);
make_on_create!(on_create_slot_1,  1);
make_on_create!(on_create_slot_2,  2);
make_on_create!(on_create_slot_3,  3);
make_on_create!(on_create_slot_4,  4);
make_on_create!(on_create_slot_5,  5);
make_on_create!(on_create_slot_6,  6);
make_on_create!(on_create_slot_7,  7);
make_on_create!(on_create_slot_8,  8);
make_on_create!(on_create_slot_9,  9);
make_on_create!(on_create_slot_10, 10);
make_on_create!(on_create_slot_11, 11);
make_on_create!(on_create_slot_12, 12);
make_on_create!(on_create_slot_13, 13);
make_on_create!(on_create_slot_14, 14);
make_on_create!(on_create_slot_15, 15);

/// Per-slot on_destroy callbacks — unload the ELF when the app is killed.
macro_rules! make_on_destroy {
    ($fn_name:ident, $slot:expr) => {
        unsafe extern "C" fn $fn_name() {
            let handle = {
                match REG_MUTEX.lock() {
                    Ok(regs) => {
                        match regs.as_ref() {
                            Some(r) => r[$slot].handle,
                            None => return,
                        }
                    }
                    Err(_) => return,
                }
            };
            if !handle.is_null() {
                elf_app_unload(handle);
            }
        }
    };
}

make_on_destroy!(on_destroy_slot_0,  0);
make_on_destroy!(on_destroy_slot_1,  1);
make_on_destroy!(on_destroy_slot_2,  2);
make_on_destroy!(on_destroy_slot_3,  3);
make_on_destroy!(on_destroy_slot_4,  4);
make_on_destroy!(on_destroy_slot_5,  5);
make_on_destroy!(on_destroy_slot_6,  6);
make_on_destroy!(on_destroy_slot_7,  7);
make_on_destroy!(on_destroy_slot_8,  8);
make_on_destroy!(on_destroy_slot_9,  9);
make_on_destroy!(on_destroy_slot_10, 10);
make_on_destroy!(on_destroy_slot_11, 11);
make_on_destroy!(on_destroy_slot_12, 12);
make_on_destroy!(on_destroy_slot_13, 13);
make_on_destroy!(on_destroy_slot_14, 14);
make_on_destroy!(on_destroy_slot_15, 15);

const ON_CREATE_TABLE: [unsafe extern "C" fn() -> i32; MAX_LOADED_APPS] = [
    on_create_slot_0,  on_create_slot_1,  on_create_slot_2,  on_create_slot_3,
    on_create_slot_4,  on_create_slot_5,  on_create_slot_6,  on_create_slot_7,
    on_create_slot_8,  on_create_slot_9,  on_create_slot_10, on_create_slot_11,
    on_create_slot_12, on_create_slot_13, on_create_slot_14, on_create_slot_15,
];

const ON_DESTROY_TABLE: [unsafe extern "C" fn(); MAX_LOADED_APPS] = [
    on_destroy_slot_0,  on_destroy_slot_1,  on_destroy_slot_2,  on_destroy_slot_3,
    on_destroy_slot_4,  on_destroy_slot_5,  on_destroy_slot_6,  on_destroy_slot_7,
    on_destroy_slot_8,  on_destroy_slot_9,  on_destroy_slot_10, on_destroy_slot_11,
    on_destroy_slot_12, on_destroy_slot_13, on_destroy_slot_14, on_destroy_slot_15,
];

/// Scan `/spiffs/apps/` and `/sdcard/apps/` for `*.app.elf` files, load each
/// one, and register it with the app manager so it appears in the launcher.
///
/// # Safety
/// May be called from C. Accesses the filesystem and global state.
#[no_mangle]
pub unsafe extern "C" fn elf_app_scan_and_register() -> c_int {
    let dirs: [&str; 2] = ["/spiffs/apps", "/sdcard/apps"];
    let mut registered: i32 = 0;

    for dir in &dirs {
        let read_dir = match std::fs::read_dir(dir) {
            Ok(d) => d,
            Err(_) => {
                esp_log_write(
                    ESP_LOG_DEBUG,
                    TAG.as_ptr(),
                    b"No apps directory: %s\0".as_ptr(),
                    dir.as_ptr(),
                );
                continue;
            }
        };

        for dir_entry in read_dir.flatten() {
            let name = dir_entry.file_name();
            let name_str = name.to_string_lossy();

            if !name_str.ends_with(".app.elf") {
                continue;
            }

            let full_path = format!("{}/{}\0", dir, name_str);
            let full_path_ptr = full_path.as_ptr() as *const c_char;

            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Found app ELF: %s\0".as_ptr(),
                full_path_ptr,
            );

            // 1. Load the ELF
            let mut handle: *mut ElfAppHandle = std::ptr::null_mut();
            let ret = elf_app_load(full_path_ptr, &mut handle);
            if ret != ESP_OK {
                esp_log_write(
                    ESP_LOG_WARN,
                    TAG.as_ptr(),
                    b"Failed to load app ELF '%s': 0x%x\0".as_ptr(),
                    full_path_ptr,
                    ret,
                );
                continue;
            }

            // 2. Parse manifest to get app metadata
            let mut manifest_path_buf = [0u8; 280];
            manifest_path_from_elf(
                full_path_ptr,
                manifest_path_buf.as_mut_ptr() as *mut c_char,
                manifest_path_buf.len(),
            );

            let mut c_manifest = std::mem::zeroed::<CManifest>();
            let manifest_ret = manifest_parse_file(
                manifest_path_buf.as_ptr() as *const c_char,
                &mut c_manifest as *mut CManifest as *mut c_void,
            );

            if manifest_ret != ESP_OK {
                esp_log_write(
                    ESP_LOG_WARN,
                    TAG.as_ptr(),
                    b"No manifest for app ELF '%s', skipping registration\0".as_ptr(),
                    full_path_ptr,
                );
                continue;
            }

            // Extract id and name from the parsed manifest
            let id_len = c_manifest.id.iter().position(|&b| b == 0).unwrap_or(c_manifest.id.len());
            let name_len = c_manifest.name.iter().position(|&b| b == 0).unwrap_or(c_manifest.name.len());

            if id_len == 0 {
                esp_log_write(
                    ESP_LOG_WARN,
                    TAG.as_ptr(),
                    b"App ELF '%s' has empty manifest id, skipping\0".as_ptr(),
                    full_path_ptr,
                );
                continue;
            }

            // 3. Find a free registration slot
            let reg_slot = {
                let mut regs = match REG_MUTEX.lock() {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let r = ensure_regs(&mut regs);
                match r.iter().position(|r| !r.used) {
                    Some(i) => i,
                    None => {
                        esp_log_write(
                            ESP_LOG_ERROR,
                            TAG.as_ptr(),
                            b"No free registration slots for app '%s'\0".as_ptr(),
                            full_path_ptr,
                        );
                        continue;
                    }
                }
            };

            // 4. Copy id/name into static string storage so pointers remain valid
            {
                let mut strings = match REG_STR_MUTEX.lock() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let s = &mut ensure_reg_strings(&mut strings)[reg_slot];
                let copy_id = id_len.min(s.id.len() - 1);
                s.id[..copy_id].copy_from_slice(&c_manifest.id[..copy_id]);
                s.id[copy_id] = 0;
                let copy_name = name_len.min(s.name.len() - 1);
                s.name[..copy_name].copy_from_slice(&c_manifest.name[..copy_name]);
                s.name[copy_name] = 0;
            }

            // 5. Build registration entry with static pointers
            //    We must get pointers into the static string storage.
            let (id_ptr, name_ptr) = {
                let mut strings = match REG_STR_MUTEX.lock() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let strs = ensure_reg_strings(&mut strings);
                (
                    strs[reg_slot].id.as_ptr() as *const c_char,
                    strs[reg_slot].name.as_ptr() as *const c_char,
                )
            };

            {
                let mut regs = match REG_MUTEX.lock() {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let reg = &mut ensure_regs(&mut regs)[reg_slot];

                reg.manifest = CAppManifest {
                    id:               id_ptr,
                    name:             name_ptr,
                    version:          b"0.0.0\0".as_ptr() as *const c_char,
                    allow_background: c_manifest.background,
                    min_memory_kb:    c_manifest.min_memory_kb,
                };
                reg.handle = handle;
                reg.entry = CAppEntry {
                    on_create:  Some(ON_CREATE_TABLE[reg_slot]),
                    on_start:   None,
                    on_pause:   None,
                    on_resume:  None,
                    on_destroy: Some(ON_DESTROY_TABLE[reg_slot]),
                    manifest:   &reg.manifest as *const CAppManifest,
                };
                reg.used = true;
            }

            // 6. Register with the app manager
            let entry_ptr = {
                let mut regs = match REG_MUTEX.lock() {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                &ensure_regs(&mut regs)[reg_slot].entry as *const CAppEntry
            };

            let reg_ret = crate::app_manager::register(entry_ptr);
            if reg_ret != ESP_OK {
                esp_log_write(
                    ESP_LOG_WARN,
                    TAG.as_ptr(),
                    b"Failed to register app '%s' with app manager: 0x%x\0".as_ptr(),
                    full_path_ptr,
                    reg_ret,
                );
                // Roll back the slot
                if let Ok(mut regs) = REG_MUTEX.lock() {
                    ensure_regs(&mut regs)[reg_slot] = ElfAppRegistration::empty();
                }
                continue;
            }

            // 7. Grant permissions from manifest
            let perms = c_manifest.permissions;
            if perms != 0 {
                permissions_grant(id_ptr, perms);
            }

            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Registered ELF app: %s (perms=0x%x)\0".as_ptr(),
                id_ptr,
                perms,
            );
            registered += 1;
        }
    }

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"App scan complete: %d app(s) registered\0".as_ptr(),
        registered,
    );

    registered
}
