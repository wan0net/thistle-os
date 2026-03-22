// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SD card SPI driver (Rust)
//
// Rust port of components/drv_sdcard/src/drv_sdcard.c.
//
// Wraps ESP-IDF's esp_vfs_fat_sdspi_mount() to expose an SD card over SPI as a
// HAL storage driver.  On non-ESP32 targets every ESP-IDF call is a no-op stub
// so that host tests and the SDL2 simulator can link without ESP-IDF headers.
//
// Calling convention note: hal_registry_start_all() passes storage_configs[i]
// (the SdcardConfig pointer) cast to *const c_char as the mount_point argument
// to mount().  We therefore always use the mount_point stored in cfg (set during
// init) and ignore the runtime argument, matching the effective C behaviour.

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalStorageDriver, HalStorageType};

// ── ESP error codes ───────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Default constants (mirror hal/sdcard_path.h THISTLE_SDCARD) ───────────────

const DEFAULT_MOUNT_POINT: &[u8] = b"/sdcard\0";
const DEFAULT_MAX_FILES: i32 = 5;

// ── Config struct (must match sdcard_config_t in drv_sdcard.h) ───────────────

/// C-compatible configuration struct.  Must match `sdcard_config_t`.
#[repr(C)]
pub struct SdcardConfig {
    /// spi_host_device_t — SPI peripheral index (e.g. SPI2_HOST = 1)
    pub spi_host: i32,
    /// gpio_num_t — chip-select GPIO
    pub pin_cs: i32,
    /// Mount point string, e.g. "/sdcard".  NULL → default "/sdcard".
    pub mount_point: *const c_char,
    /// Maximum simultaneously open files.  ≤0 → default 5.
    pub max_files: i32,
}

// SAFETY: SdcardConfig holds raw C pointers.  The driver's single-init
// contract means these are only accessed from the HAL init path.
unsafe impl Send for SdcardConfig {}
unsafe impl Sync for SdcardConfig {}

// ── ESP-IDF FFI ───────────────────────────────────────────────────────────────

/// ESP-IDF bindings — only compiled when targeting ESP-IDF.
#[cfg(target_os = "espidf")]
mod esp_ffi {
    use std::os::raw::{c_char, c_void};

    extern "C" {
        /// Mount an SD card over SPI as a FAT VFS.
        pub fn esp_vfs_fat_sdspi_mount(
            mount_point: *const c_char,
            host: *const c_void,        // const sdmmc_host_t *
            slot_cfg: *const c_void,    // const sdspi_device_config_t *
            mount_cfg: *const c_void,   // const esp_vfs_fat_sdmmc_mount_config_t *
            card: *mut *mut c_void,     // sdmmc_card_t **
        ) -> i32;

        /// Unmount an SD card previously mounted with esp_vfs_fat_sdspi_mount.
        pub fn esp_vfs_fat_sdcard_unmount(
            mount_point: *const c_char,
            card: *mut c_void, // sdmmc_card_t *
        ) -> i32;

        /// Query FAT filesystem usage statistics.
        pub fn esp_vfs_fat_info(
            mount_point: *const c_char,
            total: *mut u64,
            free_bytes: *mut u64,
        ) -> i32;
    }
}

/// No-op stubs for non-ESP32 targets (host tests, SDL2 simulator).
#[cfg(not(target_os = "espidf"))]
mod esp_ffi {
    use std::os::raw::{c_char, c_void};

    pub unsafe fn esp_vfs_fat_sdspi_mount(
        _mount_point: *const c_char,
        _host: *const c_void,
        _slot_cfg: *const c_void,
        _mount_cfg: *const c_void,
        _card: *mut *mut c_void,
    ) -> i32 {
        0 // ESP_OK
    }

    pub unsafe fn esp_vfs_fat_sdcard_unmount(
        _mount_point: *const c_char,
        _card: *mut c_void,
    ) -> i32 {
        0 // ESP_OK
    }

    pub unsafe fn esp_vfs_fat_info(
        _mount_point: *const c_char,
        _total: *mut u64,
        _free_bytes: *mut u64,
    ) -> i32 {
        0 // ESP_OK — totals remain at caller-supplied initial values
    }
}

// ── Driver state ──────────────────────────────────────────────────────────────

struct SdcardState {
    cfg: SdcardConfig,
    /// sdmmc_card_t* — opaque ESP-IDF card handle, non-null when mounted.
    card: *mut c_void,
    mounted: bool,
    initialized: bool,
}

// SAFETY: Driver state is accessed only from the HAL init/mount/unmount path
// which is single-threaded by the HAL registry contract.
unsafe impl Send for SdcardState {}
unsafe impl Sync for SdcardState {}

impl SdcardState {
    const fn new() -> Self {
        SdcardState {
            cfg: SdcardConfig {
                spi_host: 0,
                pin_cs: 0,
                mount_point: std::ptr::null(),
                max_files: 0,
            },
            card: std::ptr::null_mut(),
            mounted: false,
            initialized: false,
        }
    }
}

static mut S_SD: SdcardState = SdcardState::new();

// ── vtable implementations ────────────────────────────────────────────────────

/// Initialise the SD card driver.
///
/// Copies the supplied config, applies defaults for NULL/zero fields, and marks
/// the driver as ready to mount.  Idempotent: a second call is a no-op.
///
/// # Safety
/// `config` must be null or point to a valid `SdcardConfig`.
unsafe extern "C" fn sdcard_init(config: *const c_void) -> i32 {
    let sd = &mut *(&raw mut S_SD);

    if sd.initialized {
        return ESP_OK;
    }

    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let src = &*(config as *const SdcardConfig);
    sd.cfg.spi_host = src.spi_host;
    sd.cfg.pin_cs = src.pin_cs;
    sd.cfg.mount_point = src.mount_point;
    sd.cfg.max_files = src.max_files;

    // Apply defaults
    if sd.cfg.mount_point.is_null() {
        sd.cfg.mount_point = DEFAULT_MOUNT_POINT.as_ptr() as *const c_char;
    }
    if sd.cfg.max_files <= 0 {
        sd.cfg.max_files = DEFAULT_MAX_FILES;
    }

    sd.mounted = false;
    sd.initialized = true;
    ESP_OK
}

/// De-initialise the SD card driver.
///
/// Unmounts the card if currently mounted, then resets all state.
///
/// # Safety
/// Must only be called from the HAL registry tear-down path.
unsafe extern "C" fn sdcard_deinit() {
    let sd = &mut *(&raw mut S_SD);

    if !sd.initialized {
        return;
    }

    if sd.mounted {
        esp_ffi::esp_vfs_fat_sdcard_unmount(sd.cfg.mount_point, sd.card);
        sd.mounted = false;
        sd.card = std::ptr::null_mut();
    }

    sd.initialized = false;
}

/// Mount the SD card as a FAT VFS.
///
/// The `_mount_point` argument is ignored — the HAL registry passes the raw
/// config pointer cast to `*const c_char`, so we always use the mount point
/// stored in `cfg` (validated during `sdcard_init`).
///
/// On ESP-IDF we forward through `drv_sdcard_spi_mount_shim` (a thin C helper
/// that invokes `SDSPI_HOST_DEFAULT()` and related macros) rather than
/// duplicating the version-sensitive struct layout in Rust.  On all other
/// targets the no-op stub is used and mount always succeeds.
///
/// # Safety
/// Driver must have been successfully initialised first.
unsafe extern "C" fn sdcard_mount(_mount_point: *const c_char) -> i32 {
    let sd = &mut *(&raw mut S_SD);

    if !sd.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if sd.mounted {
        return ESP_OK;
    }

    let mp = sd.cfg.mount_point;

    #[cfg(target_os = "espidf")]
    {
        // Thin C shim that calls SDSPI_HOST_DEFAULT() and builds the structs
        // whose layout depends on ESP-IDF version.
        // Signature (drv_sdcard_shim.c):
        //   esp_err_t drv_sdcard_spi_mount_shim(
        //       const char *mount_point, int spi_host, int pin_cs,
        //       int max_files, sdmmc_card_t **card_out);
        extern "C" {
            fn drv_sdcard_spi_mount_shim(
                mount_point: *const c_char,
                spi_host: i32,
                pin_cs: i32,
                max_files: i32,
                card_out: *mut *mut c_void,
            ) -> i32;
        }

        let mut card: *mut c_void = std::ptr::null_mut();
        let ret = drv_sdcard_spi_mount_shim(
            mp,
            sd.cfg.spi_host,
            sd.cfg.pin_cs,
            sd.cfg.max_files,
            &mut card,
        );
        if ret != ESP_OK {
            return ret;
        }
        sd.card = card;
        sd.mounted = true;
        ESP_OK
    }

    #[cfg(not(target_os = "espidf"))]
    {
        // Simulator / host tests: call the no-op stub.
        let dummy_host = [0u8; 64];
        let dummy_slot = [0u8; 32];
        // esp_vfs_fat_sdmmc_mount_config_t (3 × i32)
        let mount_cfg: [i32; 3] = [0, sd.cfg.max_files, 16 * 1024];
        let mut card: *mut c_void = std::ptr::null_mut();
        let ret = esp_ffi::esp_vfs_fat_sdspi_mount(
            mp,
            dummy_host.as_ptr() as *const c_void,
            dummy_slot.as_ptr() as *const c_void,
            mount_cfg.as_ptr() as *const c_void,
            &mut card,
        );
        if ret != ESP_OK {
            return ret;
        }
        // Stub returns null card; use a non-null sentinel so callers can
        // distinguish "mounted with valid handle" from "not yet mounted".
        sd.card = 1usize as *mut c_void;
        sd.mounted = true;
        ESP_OK
    }
}

/// Unmount the SD card.
///
/// # Safety
/// Must only be called from the HAL registry tear-down path.
unsafe extern "C" fn sdcard_unmount() -> i32 {
    let sd = &mut *(&raw mut S_SD);

    if !sd.mounted {
        return ESP_OK;
    }

    let ret = esp_ffi::esp_vfs_fat_sdcard_unmount(sd.cfg.mount_point, sd.card);
    if ret == ESP_OK {
        sd.mounted = false;
        sd.card = std::ptr::null_mut();
    }
    ret
}

/// Return true if the SD card is currently mounted.
///
/// # Safety
/// Safe to call at any time after driver creation.
unsafe extern "C" fn sdcard_is_mounted() -> bool {
    (*(&raw const S_SD)).mounted
}

/// Return the total capacity of the SD card in bytes, or 0 on error.
///
/// # Safety
/// Should only be called while the card is mounted.
unsafe extern "C" fn sdcard_get_total_bytes() -> u64 {
    let sd = &*(&raw const S_SD);
    if !sd.mounted {
        return 0;
    }
    let mut total: u64 = 0;
    let mut free_bytes: u64 = 0;
    if esp_ffi::esp_vfs_fat_info(sd.cfg.mount_point, &mut total, &mut free_bytes) != ESP_OK {
        return 0;
    }
    total
}

/// Return the free space on the SD card in bytes, or 0 on error.
///
/// # Safety
/// Should only be called while the card is mounted.
unsafe extern "C" fn sdcard_get_free_bytes() -> u64 {
    let sd = &*(&raw const S_SD);
    if !sd.mounted {
        return 0;
    }
    let mut total: u64 = 0;
    let mut free_bytes: u64 = 0;
    if esp_ffi::esp_vfs_fat_info(sd.cfg.mount_point, &mut total, &mut free_bytes) != ESP_OK {
        return 0;
    }
    free_bytes
}

// ── HAL vtable ─────────────────────────────────────────────────────────────────

/// Static HAL storage driver vtable for the SPI SD card.
///
/// Returned by `drv_sdcard_get()` and passed to `hal_storage_register()`.
static SDCARD_DRIVER: HalStorageDriver = HalStorageDriver {
    init: Some(sdcard_init),
    deinit: Some(sdcard_deinit),
    mount: Some(sdcard_mount),
    unmount: Some(sdcard_unmount),
    is_mounted: Some(sdcard_is_mounted),
    get_total_bytes: Some(sdcard_get_total_bytes),
    get_free_bytes: Some(sdcard_get_free_bytes),
    storage_type: HalStorageType::Sd,
    name: b"SD Card\0".as_ptr() as *const c_char,
};

/// Return the SD card driver vtable.
///
/// Drop-in C-ABI replacement for `drv_sdcard_get()` in the original C driver.
///
/// # Safety
/// Returns a pointer to a `'static` value — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_sdcard_get() -> *const HalStorageDriver {
    &SDCARD_DRIVER
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the global driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_SD) = SdcardState::new();
    }

    // ── vtable sanity ─────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_is_non_null() {
        let p = drv_sdcard_get();
        assert!(!p.is_null());
    }

    #[test]
    fn test_vtable_fields_are_populated() {
        let drv = unsafe { &*drv_sdcard_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.mount.is_some());
        assert!(drv.unmount.is_some());
        assert!(drv.is_mounted.is_some());
        assert!(drv.get_total_bytes.is_some());
        assert!(drv.get_free_bytes.is_some());
        assert_eq!(drv.storage_type, HalStorageType::Sd);
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_vtable_name_is_sd_card() {
        let drv = unsafe { &*drv_sdcard_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "SD Card");
    }

    // ── initial state ─────────────────────────────────────────────────────────

    #[test]
    fn test_initial_state_not_mounted_not_initialized() {
        unsafe {
            reset_state();
            assert!(!sdcard_is_mounted());
            assert!(!(*(&raw const S_SD)).initialized);
        }
    }

    // ── init ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            let ret = sdcard_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            assert!(!(*(&raw const S_SD)).initialized);
        }
    }

    #[test]
    fn test_init_with_valid_config_succeeds() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: b"/sdcard\0".as_ptr() as *const c_char,
                max_files: 5,
            };
            let ret = sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_SD)).initialized);
        }
    }

    #[test]
    fn test_init_applies_default_mount_point_when_null() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            let ret = sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!(ret, ESP_OK);
            let mp = (*(&raw const S_SD)).cfg.mount_point;
            assert!(!mp.is_null());
            let s = std::ffi::CStr::from_ptr(mp).to_str().unwrap();
            assert_eq!(s, "/sdcard");
        }
    }

    #[test]
    fn test_init_applies_default_max_files_when_zero() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 0,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!((*(&raw const S_SD)).cfg.max_files, DEFAULT_MAX_FILES);
        }
    }

    #[test]
    fn test_init_applies_default_max_files_when_negative() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: -3,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!((*(&raw const S_SD)).cfg.max_files, DEFAULT_MAX_FILES);
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            let p = &cfg as *const SdcardConfig as *const c_void;
            assert_eq!(sdcard_init(p), ESP_OK);
            assert_eq!(sdcard_init(p), ESP_OK);
        }
    }

    // ── mount / unmount ───────────────────────────────────────────────────────

    #[test]
    fn test_mount_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let ret = sdcard_mount(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_mount_sets_is_mounted() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert!(!sdcard_is_mounted());
            let ret = sdcard_mount(std::ptr::null());
            assert_eq!(ret, ESP_OK);
            assert!(sdcard_is_mounted());
        }
    }

    #[test]
    fn test_double_mount_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!(sdcard_mount(std::ptr::null()), ESP_OK);
            assert_eq!(sdcard_mount(std::ptr::null()), ESP_OK);
            assert!(sdcard_is_mounted());
        }
    }

    #[test]
    fn test_unmount_when_not_mounted_returns_ok() {
        unsafe {
            reset_state();
            assert_eq!(sdcard_unmount(), ESP_OK);
        }
    }

    #[test]
    fn test_mount_unmount_cycle() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert_eq!(sdcard_mount(std::ptr::null()), ESP_OK);
            assert!(sdcard_is_mounted());
            assert_eq!(sdcard_unmount(), ESP_OK);
            assert!(!sdcard_is_mounted());
        }
    }

    // ── capacity queries ──────────────────────────────────────────────────────

    #[test]
    fn test_get_total_bytes_before_mount_returns_zero() {
        unsafe {
            reset_state();
            assert_eq!(sdcard_get_total_bytes(), 0);
        }
    }

    #[test]
    fn test_get_free_bytes_before_mount_returns_zero() {
        unsafe {
            reset_state();
            assert_eq!(sdcard_get_free_bytes(), 0);
        }
    }

    #[test]
    fn test_get_bytes_after_mount_does_not_panic() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            sdcard_mount(std::ptr::null());
            // Stubs return ESP_OK with totals = 0; just verify no panic.
            let _ = sdcard_get_total_bytes();
            let _ = sdcard_get_free_bytes();
        }
    }

    // ── deinit ────────────────────────────────────────────────────────────────

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            sdcard_deinit(); // must not panic
            assert!(!(*(&raw const S_SD)).initialized);
        }
    }

    #[test]
    fn test_deinit_clears_initialized_flag() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            assert!((*(&raw const S_SD)).initialized);
            sdcard_deinit();
            assert!(!(*(&raw const S_SD)).initialized);
        }
    }

    #[test]
    fn test_deinit_unmounts_if_mounted() {
        unsafe {
            reset_state();
            let cfg = SdcardConfig {
                spi_host: 1,
                pin_cs: 10,
                mount_point: std::ptr::null(),
                max_files: 5,
            };
            sdcard_init(&cfg as *const SdcardConfig as *const c_void);
            sdcard_mount(std::ptr::null());
            assert!(sdcard_is_mounted());
            sdcard_deinit();
            assert!(!sdcard_is_mounted());
            assert!(!(*(&raw const S_SD)).initialized);
        }
    }
}
