// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — driver_loader module
//
// Port of components/kernel/src/driver_loader.c
// Loads ELF driver files from the SD card into PSRAM using esp_elf,
// verifies signatures, parses manifests, and calls driver_init().

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_INVALID_CRC: i32 = 0x109;
const ESP_FAIL: i32 = -1;

const MAX_LOADED_DRVS: usize = 8;
const MAX_DRV_SIZE: usize = 512 * 1024; // 512 KB

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

    // Signing (Rust)
    fn signing_verify_file(path: *const c_char) -> i32;

    // Manifest (C/Rust shims)
    fn manifest_parse_file(path: *const c_char, out: *mut c_void) -> i32;
    fn manifest_is_compatible(manifest: *const c_void) -> bool;
    fn manifest_path_from_elf(elf_path: *const c_char, out: *mut c_char, out_size: usize);

    // Syscall table (C)
    fn syscall_resolve(name: *const c_char) -> *mut c_void;

    // Logging
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

// MALLOC_CAP_SPIRAM = BIT(9) = 0x200
const MALLOC_CAP_SPIRAM: u32 = 1 << 9;

// ESP log levels
const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;
const ESP_LOG_ERROR: i32 = 1;
const ESP_LOG_DEBUG: i32 = 4;

static TAG: &[u8] = b"drv_loader\0";

// Size of esp_elf_t opaque struct — must be large enough to hold the C struct.
// We use a byte array as an opaque storage blob.
// esp_elf_t is typically ~128 bytes; use 256 for safety.
const ESP_ELF_T_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct LoadedDriver {
    elf_storage: [u8; ESP_ELF_T_SIZE],
    path: [u8; 128],
    loaded: bool,
}

impl LoadedDriver {
    const fn empty() -> Self {
        LoadedDriver {
            elf_storage: [0u8; ESP_ELF_T_SIZE],
            path: [0u8; 128],
            loaded: false,
        }
    }
}

// SAFETY: LoadedDriver only mutated under a Mutex.
unsafe impl Send for LoadedDriver {}

struct DriverLoaderState {
    drivers: [LoadedDriver; MAX_LOADED_DRVS],
    count: usize,
    current_config: *const c_char,
}

impl DriverLoaderState {
    const fn new() -> Self {
        DriverLoaderState {
            drivers: [
                LoadedDriver::empty(), LoadedDriver::empty(),
                LoadedDriver::empty(), LoadedDriver::empty(),
                LoadedDriver::empty(), LoadedDriver::empty(),
                LoadedDriver::empty(), LoadedDriver::empty(),
            ],
            count: 0,
            current_config: EMPTY_CONFIG.as_ptr() as *const c_char,
        }
    }
}

// SAFETY: Only mutated through Mutex.
unsafe impl Send for DriverLoaderState {}

static EMPTY_CONFIG: &[u8] = b"{}\0";
static STATE: Mutex<DriverLoaderState> = Mutex::new(DriverLoaderState::new());

// ---------------------------------------------------------------------------
// Symbol resolver — delegates to the kernel syscall table
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_symbol_resolver(sym_name: *const c_char) -> usize {
    let addr = syscall_resolve(sym_name);
    if addr.is_null() {
        esp_log_write(
            ESP_LOG_WARN,
            TAG.as_ptr(),
            b"Unresolved driver symbol: %s\0".as_ptr(),
            sym_name,
        );
        0
    } else {
        addr as usize
    }
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the driver loader state.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn driver_loader_init() -> i32 {
    if let Ok(mut state) = STATE.lock() {
        for d in state.drivers.iter_mut() {
            *d = LoadedDriver::empty();
        }
        state.count = 0;
        unsafe {
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Driver loader initialized (max %d drivers)\0".as_ptr(),
                MAX_LOADED_DRVS as c_int,
            );
        }
    }
    ESP_OK
}

/// Return the number of loaded drivers.
#[no_mangle]
pub extern "C" fn driver_loader_get_count() -> c_int {
    STATE.lock().map(|s| s.count as c_int).unwrap_or(0)
}

/// Return the current driver config JSON (set during driver_loader_load_with_config).
///
/// # Safety
/// Returns a pointer to static storage. Do not free.
#[no_mangle]
pub extern "C" fn driver_loader_get_config() -> *const c_char {
    STATE
        .lock()
        .map(|s| s.current_config)
        .unwrap_or(EMPTY_CONFIG.as_ptr() as *const c_char)
}

/// Load a driver ELF from `path`.
///
/// Steps: verify signature, parse manifest, read ELF into PSRAM, relocate,
/// call driver_init() entry point.
///
/// # Safety
/// `path` must be a valid null-terminated C string. May be called from C.
#[no_mangle]
pub unsafe extern "C" fn driver_loader_load(path: *const c_char) -> i32 {
    if path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    // Check slot availability
    let count = STATE.lock().map(|s| s.count).unwrap_or(MAX_LOADED_DRVS);
    if count >= MAX_LOADED_DRVS {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"No free driver slots (max %d)\0".as_ptr(),
            MAX_LOADED_DRVS as c_int,
        );
        return ESP_ERR_NO_MEM;
    }

    // 1. Verify signature
    let sig_ret = signing_verify_file(path);
    if sig_ret == ESP_ERR_INVALID_CRC {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"Driver signature INVALID: %s\0".as_ptr(),
            path,
        );
        return ESP_ERR_INVALID_CRC;
    } else if sig_ret == ESP_ERR_NOT_FOUND {
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"Driver unsigned (dev mode): %s\0".as_ptr(), path);
    } else if sig_ret == ESP_OK {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Driver signature verified: %s\0".as_ptr(), path);
    }

    // 2. Parse manifest (optional)
    {
        let mut manifest_path_buf = [0u8; 280];
        manifest_path_from_elf(
            path,
            manifest_path_buf.as_mut_ptr() as *mut c_char,
            manifest_path_buf.len(),
        );

        // Use a heap-allocated opaque blob for the manifest struct
        let manifest_buf = heap_caps_malloc(512, MALLOC_CAP_SPIRAM);
        if !manifest_buf.is_null() {
            if manifest_parse_file(manifest_path_buf.as_ptr() as *const c_char, manifest_buf) == ESP_OK {
                if !manifest_is_compatible(manifest_buf as *const c_void) {
                    esp_log_write(
                        ESP_LOG_ERROR,
                        TAG.as_ptr(),
                        b"Driver incompatible: %s\0".as_ptr(),
                        path,
                    );
                    free(manifest_buf);
                    return ESP_ERR_NOT_SUPPORTED;
                }
                esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Driver manifest OK: %s\0".as_ptr(), path);
            }
            free(manifest_buf);
        }
    }

    // 3. Read ELF file into PSRAM
    let file_data = match std::fs::read(path_str) {
        Ok(d) => d,
        Err(_) => {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Cannot open driver ELF: %s\0".as_ptr(), path);
            return ESP_ERR_NOT_FOUND;
        }
    };

    let size = file_data.len();
    if size == 0 || size > MAX_DRV_SIZE {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"Rejecting driver size %d: %s\0".as_ptr(),
            size as c_int,
            path,
        );
        return ESP_ERR_INVALID_SIZE;
    }

    let buf = heap_caps_malloc(size, MALLOC_CAP_SPIRAM);
    if buf.is_null() {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"PSRAM alloc failed for driver: %s\0".as_ptr(), path);
        return ESP_ERR_NO_MEM;
    }
    std::ptr::copy_nonoverlapping(file_data.as_ptr(), buf as *mut u8, size);
    drop(file_data); // Release Rust-side buffer

    // 4. Initialise esp_elf context using per-slot storage
    let slot_idx = STATE.lock().map(|s| s.count).unwrap_or(0);

    let ret = {
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => { free(buf); return ESP_FAIL; }
        };
        let drv = &mut state.drivers[slot_idx];
        let elf_ptr = drv.elf_storage.as_mut_ptr() as *mut c_void;

        let r = esp_elf_init(elf_ptr);
        if r != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_elf_init failed: %s\0".as_ptr(), path);
            free(buf);
            return r;
        }

        // 5. Set symbol resolver and relocate
        elf_set_symbol_resolver(driver_symbol_resolver);

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Loading driver: %s (%d bytes)\0".as_ptr(),
            path,
            size as c_int,
        );

        let r = esp_elf_relocate(elf_ptr, buf as *const u8);
        free(buf);

        if r != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_elf_relocate failed: %s\0".as_ptr(), path);
            esp_elf_deinit(elf_ptr);
            return r;
        }

        // 6. Call driver_init() entry point
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Calling driver_init() for: %s\0".as_ptr(), path);
        let init_ret = esp_elf_request(elf_ptr, 0, 0, std::ptr::null_mut());
        if init_ret != 0 {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"driver_init() failed for '%s': ret=%d\0".as_ptr(),
                path,
                init_ret,
            );
            esp_elf_deinit(elf_ptr);
            return ESP_FAIL;
        }

        // 7. Record loaded driver
        let path_bytes = CStr::from_ptr(path).to_bytes_with_nul();
        let copy_len = path_bytes.len().min(drv.path.len() - 1);
        drv.path[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
        drv.path[copy_len] = 0;
        drv.loaded = true;
        state.count += 1;

        ESP_OK
    };

    if ret == ESP_OK {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Driver loaded successfully: %s\0".as_ptr(), path);
    }

    ret
}

/// Scan the drivers directory on the SD card and load all `.drv.elf` files.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub unsafe extern "C" fn driver_loader_scan_and_load() -> c_int {
    // THISTLE_SDCARD is typically "/sdcard"
    let drivers_dir = "/sdcard/drivers";

    let read_dir = match std::fs::read_dir(drivers_dir) {
        Ok(d) => d,
        Err(_) => {
            esp_log_write(
                ESP_LOG_DEBUG,
                TAG.as_ptr(),
                b"No drivers directory: %s\0".as_ptr(),
                drivers_dir.as_ptr(),
            );
            return 0;
        }
    };

    let mut loaded = 0i32;

    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.ends_with(".drv.elf") {
            continue;
        }

        let full_path = format!("{}/{}\0", drivers_dir, name_str);
        let ret = driver_loader_load(full_path.as_ptr() as *const c_char);
        if ret == ESP_OK {
            loaded += 1;
        } else {
            esp_log_write(
                ESP_LOG_WARN,
                TAG.as_ptr(),
                b"Failed to load driver '%s': %d\0".as_ptr(),
                full_path.as_ptr(),
                ret,
            );
        }
    }

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Scanned drivers dir: %d driver(s) loaded\0".as_ptr(),
        loaded,
    );

    loaded
}

/// Load a driver with an explicit JSON config string.
///
/// The config is available to the driver during init via `driver_loader_get_config()`.
///
/// # Safety
/// `path` and `config_json` must be valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn driver_loader_load_with_config(
    path: *const c_char,
    config_json: *const c_char,
) -> i32 {
    let cfg = if config_json.is_null() {
        EMPTY_CONFIG.as_ptr() as *const c_char
    } else {
        config_json
    };

    if let Ok(mut state) = STATE.lock() {
        state.current_config = cfg;
    }

    let ret = driver_loader_load(path);

    if let Ok(mut state) = STATE.lock() {
        state.current_config = EMPTY_CONFIG.as_ptr() as *const c_char;
    }

    ret
}
