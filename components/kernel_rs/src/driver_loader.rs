// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — driver_loader module
//
// Port of components/kernel/src/driver_loader.c
// Loads ELF driver files from the SD card into PSRAM using esp_elf,
// verifies signatures, parses manifests, and calls driver_init().

use std::ffi::CStr;
use std::io::Read;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

use crate::ffi::CManifest;

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

    // Manifest (C/Rust shims)
    fn manifest_parse_file(path: *const c_char, out: *mut CManifest) -> i32;
    fn manifest_path_from_elf(elf_path: *const c_char, out: *mut c_char, out_size: usize);

    // Syscall table (C)
    fn syscall_resolve(name: *const c_char) -> *mut c_void;

    // Logging
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

// ---------------------------------------------------------------------------
// Simulator: dlopen-based driver loading (replaces esp_elf)
// ---------------------------------------------------------------------------

#[cfg(all(not(target_os = "espidf"), feature = "sim-bus"))]
mod sim_loader {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_void};

    extern "C" {
        fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
        fn driver_loader_get_config() -> *const c_char;
    }

    const ESP_LOG_INFO: i32 = 3;
    const ESP_LOG_ERROR: i32 = 1;
    const ESP_LOG_WARN: i32 = 2;
    static TAG: &[u8] = b"drv_loader\0";

    /// Convert a .drv.elf path to a .drv.dylib (macOS) or .drv.so (Linux) path.
    pub fn elf_path_to_host_lib(elf_path: &str) -> Option<String> {
        if let Some(base) = elf_path.strip_suffix(".drv.elf") {
            #[cfg(target_os = "macos")]
            return Some(format!("{}.drv.dylib", base));
            #[cfg(not(target_os = "macos"))]
            return Some(format!("{}.drv.so", base));
        }
        if let Some(base) = elf_path.strip_suffix(".app.elf") {
            #[cfg(target_os = "macos")]
            return Some(format!("{}.app.dylib", base));
            #[cfg(not(target_os = "macos"))]
            return Some(format!("{}.app.so", base));
        }
        None
    }

    /// Load a driver shared library via dlopen and call its driver_init().
    ///
    /// Returns ESP_OK on success, error code on failure.
    pub unsafe fn load_driver_dylib(path: &str) -> i32 {
        let lib_path = match elf_path_to_host_lib(path) {
            Some(p) => p,
            None => {
                esp_log_write(
                    ESP_LOG_ERROR,
                    TAG.as_ptr(),
                    b"Cannot convert to host lib: %s\0".as_ptr(),
                    path.as_ptr(),
                );
                return -1; // ESP_FAIL
            }
        };

        // Check if the host library exists
        if !std::path::Path::new(&lib_path).exists() {
            esp_log_write(
                ESP_LOG_WARN,
                TAG.as_ptr(),
                b"No host library found (skipping): %s\0".as_ptr(),
                format!("{}\0", lib_path).as_ptr(),
            );
            return 0x105; // ESP_ERR_NOT_FOUND — non-fatal, driver just not available
        }

        let c_lib_path = format!("{}\0", lib_path);

        // dlopen the shared library
        let handle = libc::dlopen(
            c_lib_path.as_ptr() as *const i8,
            libc::RTLD_NOW | libc::RTLD_LOCAL,
        );

        if handle.is_null() {
            let err = libc::dlerror();
            let err_str = if err.is_null() {
                "unknown error"
            } else {
                CStr::from_ptr(err).to_str().unwrap_or("unknown error")
            };
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"dlopen failed: %s\0".as_ptr(),
                format!("{}\0", err_str).as_ptr(),
            );
            return -1;
        }

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Loaded host library: %s\0".as_ptr(),
            c_lib_path.as_ptr(),
        );

        // Look up driver_init symbol
        let init_sym = libc::dlsym(handle, b"driver_init\0".as_ptr() as *const i8);
        if init_sym.is_null() {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"driver_init not found in %s\0".as_ptr(),
                c_lib_path.as_ptr(),
            );
            libc::dlclose(handle);
            return -1;
        }

        // Call driver_init(config_json)
        let driver_init: unsafe extern "C" fn(*const c_char) -> c_int =
            std::mem::transmute(init_sym);

        let config = driver_loader_get_config();
        let ret = driver_init(config);

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"driver_init() returned %d for %s\0".as_ptr(),
            ret as c_int,
            c_lib_path.as_ptr(),
        );

        // Don't dlclose — driver code is still running
        // Leak the handle intentionally (driver stays loaded)

        if ret == 0 {
            0
        } else {
            -1
        }
    }
}

// MALLOC_CAP_SPIRAM = BIT(9) = 0x200
const MALLOC_CAP_SPIRAM: u32 = 1 << 9;

// ESP log levels
const ESP_LOG_INFO: i32 = 3;
const ESP_LOG_WARN: i32 = 2;
const ESP_LOG_ERROR: i32 = 1;
const ESP_LOG_DEBUG: i32 = 4;

static TAG: &[u8] = b"drv_loader\0";

// Size of esp_elf_t opaque struct — must be large enough to hold the C struct.
// We use a byte array as an opaque storage blob.
// esp_elf_t is typically ~128 bytes; use 256 for safety.
const ESP_ELF_T_SIZE: usize = 256;

fn manifest_is_compatible_for_arch(manifest: &CManifest, current_arch: &CStr) -> bool {
    unsafe { crate::ffi::manifest_is_compatible(manifest, current_arch.as_ptr()) }
}

fn manifest_is_compatible_for_current_arch(manifest: &CManifest) -> bool {
    manifest_is_compatible_for_arch(manifest, crate::manifest::current_arch_cstr())
}

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
    drivers: Option<Box<[LoadedDriver; MAX_LOADED_DRVS]>>,
    count: usize,
    current_config: *const c_char,
}

impl DriverLoaderState {
    const fn new() -> Self {
        DriverLoaderState {
            drivers: None,
            count: 0,
            current_config: EMPTY_CONFIG.as_ptr() as *const c_char,
        }
    }

    fn ensure_drivers(&mut self) -> &mut [LoadedDriver; MAX_LOADED_DRVS] {
        if self.drivers.is_none() {
            self.drivers = Some(Box::new([
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
                LoadedDriver::empty(),
            ]));
        }
        self.drivers.as_mut().unwrap()
    }
}

// SAFETY: Only mutated through Mutex.
unsafe impl Send for DriverLoaderState {}

static EMPTY_CONFIG: &[u8] = b"{}\0";
static STATE: Mutex<DriverLoaderState> = Mutex::new(DriverLoaderState::new());

/// Return whether signature policy permits loading a driver. Production
/// accepts only a verified signature. Debug builds retain the deliberate
/// development exception for an absent `.sig`, but malformed signatures and
/// all verifier failures remain fatal.
fn driver_signature_allows_load(signature_result: i32) -> Result<bool, i32> {
    if signature_result == ESP_OK {
        return Ok(true);
    }

    if signature_result == ESP_ERR_NOT_FOUND {
        #[cfg(debug_assertions)]
        return Ok(false);
    }

    Err(signature_result)
}

#[derive(Debug, PartialEq, Eq)]
enum PrepareDriverImageError {
    Read(i32),
    InvalidSize,
    Verify(i32),
}

fn prepare_driver_image_with(
    path: &str,
    read_image: impl FnOnce(&str) -> Result<Vec<u8>, i32>,
    verify_image: impl FnOnce(&str, &[u8]) -> i32,
) -> Result<(Vec<u8>, bool), PrepareDriverImageError> {
    let image = read_image(path).map_err(PrepareDriverImageError::Read)?;
    if image.is_empty() || image.len() > MAX_DRV_SIZE {
        return Err(PrepareDriverImageError::InvalidSize);
    }

    let signature_verified = driver_signature_allows_load(verify_image(path, &image))
        .map_err(PrepareDriverImageError::Verify)?;
    Ok((image, signature_verified))
}

fn prepare_driver_image(path: &str) -> Result<(Vec<u8>, bool), PrepareDriverImageError> {
    prepare_driver_image_with(
        path,
        |path| {
            let file = std::fs::File::open(path).map_err(|_| ESP_ERR_NOT_FOUND)?;
            let mut image = Vec::new();
            file.take((MAX_DRV_SIZE + 1) as u64)
                .read_to_end(&mut image)
                .map_err(|_| ESP_FAIL)?;
            Ok(image)
        },
        crate::signing::verify_file_bytes,
    )
}

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
        let drivers = state.ensure_drivers();
        for d in drivers.iter_mut() {
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
/// Steps: read and verify one ELF snapshot, parse manifest, copy to PSRAM, relocate,
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

    // Simulator: use dlopen instead of esp_elf
    #[cfg(all(not(target_os = "espidf"), feature = "sim-bus"))]
    {
        let ret = sim_loader::load_driver_dylib(path_str);
        if ret == ESP_OK {
            if let Ok(mut state) = STATE.lock() {
                let drv = &mut state.ensure_drivers()[count];
                let path_bytes = CStr::from_ptr(path).to_bytes_with_nul();
                let copy_len = path_bytes.len().min(drv.path.len() - 1);
                drv.path[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
                drv.path[copy_len] = 0;
                drv.loaded = true;
                state.count += 1;
            }
        }
        return ret;
    }

    // 1. Read once and verify the exact byte snapshot that will be relocated.
    let (file_data, signature_verified) = match prepare_driver_image(path_str) {
        Ok(prepared) => prepared,
        Err(PrepareDriverImageError::Read(err)) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Cannot open driver ELF: %s\0".as_ptr(),
                path,
            );
            return err;
        }
        Err(PrepareDriverImageError::InvalidSize) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Rejecting driver size: %s\0".as_ptr(),
                path,
            );
            return ESP_ERR_INVALID_SIZE;
        }
        Err(PrepareDriverImageError::Verify(err)) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Driver signature verification failed: %d for %s\0".as_ptr(),
                err,
                path,
            );
            return err;
        }
    };

    if signature_verified {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Driver signature verified: %s\0".as_ptr(),
            path,
        );
    } else {
        esp_log_write(
            ESP_LOG_WARN,
            TAG.as_ptr(),
            b"Driver unsigned (dev mode): %s\0".as_ptr(),
            path,
        );
    }

    // 2. Parse manifest (optional)
    {
        let mut manifest_path_buf = [0u8; 280];
        manifest_path_from_elf(
            path,
            manifest_path_buf.as_mut_ptr() as *mut c_char,
            manifest_path_buf.len(),
        );

        let mut manifest = std::mem::MaybeUninit::<CManifest>::uninit();
        if manifest_parse_file(
            manifest_path_buf.as_ptr() as *const c_char,
            manifest.as_mut_ptr(),
        ) == ESP_OK
        {
            let manifest = manifest.assume_init();
            if !manifest_is_compatible_for_current_arch(&manifest) {
                esp_log_write(
                    ESP_LOG_ERROR,
                    TAG.as_ptr(),
                    b"Driver incompatible: %s\0".as_ptr(),
                    path,
                );
                return ESP_ERR_NOT_SUPPORTED;
            }
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"Driver manifest OK: %s\0".as_ptr(),
                path,
            );
        }
    }

    // 3. Copy the already-verified snapshot into PSRAM.
    let size = file_data.len();
    let buf = heap_caps_malloc(size, MALLOC_CAP_SPIRAM);
    if buf.is_null() {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"PSRAM alloc failed for driver: %s\0".as_ptr(),
            path,
        );
        return ESP_ERR_NO_MEM;
    }
    std::ptr::copy_nonoverlapping(file_data.as_ptr(), buf as *mut u8, size);
    drop(file_data); // Release Rust-side buffer

    // 4. Initialise esp_elf context using per-slot storage
    let slot_idx = STATE.lock().map(|s| s.count).unwrap_or(0);

    let ret = {
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => {
                free(buf);
                return ESP_FAIL;
            }
        };
        let drv = &mut state.ensure_drivers()[slot_idx];
        let elf_ptr = drv.elf_storage.as_mut_ptr() as *mut c_void;

        let r = esp_elf_init(elf_ptr);
        if r != ESP_OK {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"esp_elf_init failed: %s\0".as_ptr(),
                path,
            );
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
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"esp_elf_relocate failed: %s\0".as_ptr(),
                path,
            );
            esp_elf_deinit(elf_ptr);
            return r;
        }

        // 6. Call driver_init() entry point
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Calling driver_init() for: %s\0".as_ptr(),
            path,
        );
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
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Driver loaded successfully: %s\0".as_ptr(),
            path,
        );
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

        #[cfg(all(not(target_os = "espidf"), feature = "sim-bus"))]
        let valid_ext = {
            #[cfg(target_os = "macos")]
            {
                name_str.ends_with(".drv.dylib") || name_str.ends_with(".drv.elf")
            }
            #[cfg(not(target_os = "macos"))]
            {
                name_str.ends_with(".drv.so") || name_str.ends_with(".drv.elf")
            }
        };
        #[cfg(not(all(not(target_os = "espidf"), feature = "sim-bus")))]
        let valid_ext = name_str.ends_with(".drv.elf");

        if !valid_ext {
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

// ---------------------------------------------------------------------------
// Tests
//
// driver_loader_init(), driver_loader_load(), and driver_loader_scan_and_load()
// all call esp_log_write and are not safe on aarch64-apple-darwin.
//
// driver_loader_get_count() and driver_loader_get_config() are pure Rust and
// are tested here using direct state manipulation under the Mutex.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::{CStr, CString};
    use std::rc::Rc;

    fn reset_state() {
        if let Ok(mut s) = STATE.lock() {
            let drivers = s.ensure_drivers();
            for d in drivers.iter_mut() {
                *d = LoadedDriver::empty();
            }
            s.count = 0;
            s.current_config = EMPTY_CONFIG.as_ptr() as *const c_char;
        }
    }

    // -----------------------------------------------------------------------
    // test_get_count_is_zero_initially
    // Mirrors test_driver_loader.c: count is 0 after init (no drivers loaded).
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_count_is_zero_initially() {
        reset_state();
        assert_eq!(
            driver_loader_get_count(),
            0,
            "driver count must be 0 after reset"
        );
    }

    // -----------------------------------------------------------------------
    // test_get_config_returns_default
    // Mirrors test_driver_loader.c: default config is the "{}" empty JSON.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_config_returns_default() {
        reset_state();
        let ptr = driver_loader_get_config();
        assert!(
            !ptr.is_null(),
            "driver_loader_get_config() must not return NULL"
        );
        let s = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert_eq!(s, "{}", "default config must be \"{{}}\"");
    }

    // -----------------------------------------------------------------------
    // test_get_count_after_manual_increment
    // Direct state manipulation verifies the count accessor is wired correctly.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_count_after_manual_increment() {
        reset_state();
        {
            let mut s = STATE.lock().unwrap();
            s.ensure_drivers()[0].loaded = true;
            s.count = 1;
        }
        assert_eq!(
            driver_loader_get_count(),
            1,
            "driver count must reflect manually-set value"
        );
        reset_state();
    }

    // -----------------------------------------------------------------------
    // test_max_loaded_drvs_constant
    // The capacity constant must match what tests assume.
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_loaded_drvs_constant() {
        assert_eq!(MAX_LOADED_DRVS, 8, "MAX_LOADED_DRVS must be 8");
    }

    #[test]
    fn driver_loader_accepts_matching_canonical_architectures_only() {
        let universal = CManifest::from(&crate::manifest::Manifest::default());
        for package_arch in ["esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6"] {
            let manifest = CManifest::from(&crate::manifest::Manifest {
                arch: package_arch.into(),
                ..Default::default()
            });
            let matching = CString::new(package_arch).unwrap();
            assert!(manifest_is_compatible_for_arch(&universal, &matching));
            assert!(manifest_is_compatible_for_arch(&manifest, &matching));

            let mismatch = if package_arch == "esp32c3" {
                CString::new("esp32s3").unwrap()
            } else {
                CString::new("esp32c3").unwrap()
            };
            assert!(!manifest_is_compatible_for_arch(&manifest, &mismatch));
        }
    }

    #[test]
    fn test_driver_signature_gate_rejects_verifier_errors() {
        assert_eq!(driver_signature_allows_load(ESP_OK), Ok(true));

        #[cfg(debug_assertions)]
        assert_eq!(driver_signature_allows_load(ESP_ERR_NOT_FOUND), Ok(false));
        #[cfg(not(debug_assertions))]
        assert_eq!(
            driver_signature_allows_load(ESP_ERR_NOT_FOUND),
            Err(ESP_ERR_NOT_FOUND)
        );

        for error in [
            ESP_ERR_INVALID_CRC,
            ESP_ERR_INVALID_SIZE,
            0x103, // ESP_ERR_INVALID_STATE
            ESP_ERR_INVALID_ARG,
        ] {
            assert_eq!(driver_signature_allows_load(error), Err(error));
        }
    }

    #[test]
    fn test_prepared_driver_image_is_the_exact_verified_snapshot() {
        let trusted = b"trusted driver bytes".to_vec();
        let attacker = b"swapped attacker bytes".to_vec();
        let on_disk = Rc::new(RefCell::new(trusted.clone()));
        let verified = Rc::new(RefCell::new(Vec::new()));

        let read_disk = Rc::clone(&on_disk);
        let verify_disk = Rc::clone(&on_disk);
        let verify_capture = Rc::clone(&verified);
        let prepared = prepare_driver_image_with(
            "/sdcard/drivers/test.drv.elf",
            move |_| Ok(read_disk.borrow().clone()),
            move |_, image| {
                verify_capture.borrow_mut().extend_from_slice(image);
                *verify_disk.borrow_mut() = attacker;
                ESP_OK
            },
        )
        .expect("the trusted snapshot should pass verification");

        assert_eq!(*verified.borrow(), trusted);
        assert_eq!(
            prepared.0, trusted,
            "relocation must receive verified bytes"
        );
        assert_ne!(
            prepared.0,
            *on_disk.borrow(),
            "a path swap must be irrelevant"
        );
    }

    #[test]
    fn test_prepared_driver_image_rejects_before_relocation_on_verifier_failure() {
        let result = prepare_driver_image_with(
            "/sdcard/drivers/test.drv.elf",
            |_| Ok(b"untrusted driver bytes".to_vec()),
            |_, _| ESP_ERR_INVALID_CRC,
        );

        assert_eq!(
            result,
            Err(PrepareDriverImageError::Verify(ESP_ERR_INVALID_CRC))
        );
    }

    #[test]
    fn test_prepared_driver_image_rejects_oversize_before_verification() {
        let verifier_called = Rc::new(RefCell::new(false));
        let called = Rc::clone(&verifier_called);
        let result = prepare_driver_image_with(
            "/sdcard/drivers/oversized.drv.elf",
            |_| Ok(vec![0u8; MAX_DRV_SIZE + 1]),
            move |_, _| {
                *called.borrow_mut() = true;
                ESP_OK
            },
        );

        assert_eq!(result, Err(PrepareDriverImageError::InvalidSize));
        assert!(!*verifier_called.borrow());
    }
}
