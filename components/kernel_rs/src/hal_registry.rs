// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — HAL Registry
//
// Pure Rust replacement for components/thistle_hal/src/hal_registry.c.
// Exports the same C-compatible symbols so C board-init code and drivers
// can register hardware without modification.
//
// Layout mirrors the C structs in components/thistle_hal/include/hal/ exactly
// so that pointers passed from C are safe to dereference.

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_void};

// ── ESP error codes ──────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;

// ── Registry capacity constants (must match board.h) ─────────────────

pub const HAL_MAX_INPUT_DRIVERS: usize = 4;
pub const HAL_MAX_STORAGE_DRIVERS: usize = 2;

// ── Display HAL types (hal/display.h) ────────────────────────────────

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HalDisplayType {
    Lcd = 0,
    Epaper = 1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HalDisplayRefreshMode {
    Full = 0,
    Partial = 1,
    Fast = 2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct HalArea {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

#[repr(C)]
pub struct HalDisplayDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub flush: Option<unsafe extern "C" fn(area: *const HalArea, color_data: *const u8) -> i32>,
    pub refresh: Option<unsafe extern "C" fn() -> i32>,
    pub set_brightness: Option<unsafe extern "C" fn(percent: u8) -> i32>,
    pub sleep: Option<unsafe extern "C" fn(enter: bool) -> i32>,
    pub set_refresh_mode: Option<unsafe extern "C" fn(mode: HalDisplayRefreshMode) -> i32>,
    pub width: u16,
    pub height: u16,
    pub display_type: HalDisplayType,
    pub name: *const c_char,
}

// SAFETY: HalDisplayDriver contains only raw pointers and fn pointers.
unsafe impl Send for HalDisplayDriver {}
unsafe impl Sync for HalDisplayDriver {}

// ── Input HAL types (hal/input.h) ─────────────────────────────────────

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HalInputEventType {
    KeyDown = 0,
    KeyUp = 1,
    TouchDown = 2,
    TouchUp = 3,
    TouchMove = 4,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalInputKeyData {
    pub keycode: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalInputTouchData {
    pub x: u16,
    pub y: u16,
}

#[repr(C)]
pub union HalInputEventData {
    pub key: HalInputKeyData,
    pub touch: HalInputTouchData,
}

#[repr(C)]
pub struct HalInputEvent {
    pub event_type: HalInputEventType,
    pub timestamp: u32,
    pub data: HalInputEventData,
}

pub type HalInputCb =
    Option<unsafe extern "C" fn(event: *const HalInputEvent, user_data: *mut c_void)>;

#[repr(C)]
pub struct HalInputDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub register_callback: Option<unsafe extern "C" fn(cb: HalInputCb, user_data: *mut c_void) -> i32>,
    pub poll: Option<unsafe extern "C" fn() -> i32>,
    pub name: *const c_char,
    pub is_touch: bool,
}

unsafe impl Send for HalInputDriver {}
unsafe impl Sync for HalInputDriver {}

// ── Radio HAL types (hal/radio.h) ─────────────────────────────────────

pub type HalRadioRxCb = Option<
    unsafe extern "C" fn(data: *const u8, len: usize, rssi: i32, user_data: *mut c_void),
>;

#[repr(C)]
pub struct HalRadioDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub set_frequency: Option<unsafe extern "C" fn(freq_hz: u32) -> i32>,
    pub set_tx_power: Option<unsafe extern "C" fn(dbm: i8) -> i32>,
    pub set_bandwidth: Option<unsafe extern "C" fn(bw_hz: u32) -> i32>,
    pub set_spreading_factor: Option<unsafe extern "C" fn(sf: u8) -> i32>,
    pub send: Option<unsafe extern "C" fn(data: *const u8, len: usize) -> i32>,
    pub start_receive: Option<unsafe extern "C" fn(cb: HalRadioRxCb, user_data: *mut c_void) -> i32>,
    pub stop_receive: Option<unsafe extern "C" fn() -> i32>,
    pub get_rssi: Option<unsafe extern "C" fn() -> i32>,
    pub sleep: Option<unsafe extern "C" fn(enter: bool) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalRadioDriver {}
unsafe impl Sync for HalRadioDriver {}

// ── GPS HAL types (hal/gps.h) ──────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalGpsPosition {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: f32,
    pub speed_kmh: f32,
    pub heading_deg: f32,
    pub satellites: u8,
    pub fix_valid: bool,
    pub timestamp: u32,
}

pub type HalGpsCb = Option<
    unsafe extern "C" fn(pos: *const HalGpsPosition, user_data: *mut c_void),
>;

#[repr(C)]
pub struct HalGpsDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub enable: Option<unsafe extern "C" fn() -> i32>,
    pub disable: Option<unsafe extern "C" fn() -> i32>,
    pub get_position: Option<unsafe extern "C" fn(pos: *mut HalGpsPosition) -> i32>,
    pub register_callback: Option<unsafe extern "C" fn(cb: HalGpsCb, user_data: *mut c_void) -> i32>,
    pub sleep: Option<unsafe extern "C" fn(enter: bool) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalGpsDriver {}
unsafe impl Sync for HalGpsDriver {}

// ── Audio HAL types (hal/audio.h) ─────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalAudioConfig {
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
}

#[repr(C)]
pub struct HalAudioDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub play: Option<unsafe extern "C" fn(data: *const u8, len: usize) -> i32>,
    pub stop: Option<unsafe extern "C" fn() -> i32>,
    pub set_volume: Option<unsafe extern "C" fn(percent: u8) -> i32>,
    pub configure: Option<unsafe extern "C" fn(cfg: *const HalAudioConfig) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalAudioDriver {}
unsafe impl Sync for HalAudioDriver {}

// ── Power HAL types (hal/power.h) ──────────────────────────────────────

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HalPowerState {
    Discharging = 0,
    Charging = 1,
    Charged = 2,
    NoBattery = 3,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalPowerInfo {
    pub voltage_mv: u16,
    pub percent: u8,
    pub state: HalPowerState,
}

#[repr(C)]
pub struct HalPowerDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_info: Option<unsafe extern "C" fn(info: *mut HalPowerInfo) -> i32>,
    pub get_battery_mv: Option<unsafe extern "C" fn() -> u16>,
    pub get_battery_percent: Option<unsafe extern "C" fn() -> u8>,
    pub is_charging: Option<unsafe extern "C" fn() -> bool>,
    pub sleep: Option<unsafe extern "C" fn(enter: bool) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalPowerDriver {}
unsafe impl Sync for HalPowerDriver {}

// ── IMU HAL types (hal/imu.h) ──────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HalImuData {
    pub accel_x: f32,
    pub accel_y: f32,
    pub accel_z: f32,
    pub gyro_x: f32,
    pub gyro_y: f32,
    pub gyro_z: f32,
    pub mag_x: f32,
    pub mag_y: f32,
    pub mag_z: f32,
}

pub type HalImuCb = Option<
    unsafe extern "C" fn(data: *const HalImuData, user_data: *mut c_void),
>;

#[repr(C)]
pub struct HalImuDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_data: Option<unsafe extern "C" fn(data: *mut HalImuData) -> i32>,
    pub register_callback: Option<unsafe extern "C" fn(cb: HalImuCb, user_data: *mut c_void) -> i32>,
    pub set_sample_rate: Option<unsafe extern "C" fn(hz: u16) -> i32>,
    pub sleep: Option<unsafe extern "C" fn(enter: bool) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalImuDriver {}
unsafe impl Sync for HalImuDriver {}

// ── Storage HAL types (hal/storage.h) ─────────────────────────────────

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HalStorageType {
    Sd = 0,
    Internal = 1,
}

#[repr(C)]
pub struct HalStorageDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub mount: Option<unsafe extern "C" fn(mount_point: *const c_char) -> i32>,
    pub unmount: Option<unsafe extern "C" fn() -> i32>,
    pub is_mounted: Option<unsafe extern "C" fn() -> bool>,
    pub get_total_bytes: Option<unsafe extern "C" fn() -> u64>,
    pub get_free_bytes: Option<unsafe extern "C" fn() -> u64>,
    pub storage_type: HalStorageType,
    pub name: *const c_char,
}

unsafe impl Send for HalStorageDriver {}
unsafe impl Sync for HalStorageDriver {}

// ── Crypto HAL types (hal/crypto.h) ───────────────────────────────────

#[repr(C)]
pub struct HalCryptoDriver {
    pub sha256: Option<unsafe extern "C" fn(data: *const u8, len: usize, hash_out: *mut u8) -> i32>,
    pub aes256_cbc_encrypt: Option<
        unsafe extern "C" fn(
            key: *const u8,
            iv: *const u8,
            plaintext: *const u8,
            len: usize,
            ciphertext_out: *mut u8,
        ) -> i32,
    >,
    pub aes256_cbc_decrypt: Option<
        unsafe extern "C" fn(
            key: *const u8,
            iv: *const u8,
            ciphertext: *const u8,
            len: usize,
            plaintext_out: *mut u8,
        ) -> i32,
    >,
    pub hmac_sha256: Option<
        unsafe extern "C" fn(
            key: *const u8,
            key_len: usize,
            data: *const u8,
            data_len: usize,
            mac_out: *mut u8,
        ) -> i32,
    >,
    pub random: Option<unsafe extern "C" fn(buf: *mut u8, len: usize) -> i32>,
    pub name: *const c_char,
}

unsafe impl Send for HalCryptoDriver {}
unsafe impl Sync for HalCryptoDriver {}

// ── RTC HAL types ──────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct HalDateTime {
    pub year: u16,   // e.g., 2026
    pub month: u8,   // 1-12
    pub day: u8,     // 1-31
    pub weekday: u8, // 0=Sunday, 6=Saturday
    pub hour: u8,    // 0-23
    pub minute: u8,  // 0-59
    pub second: u8,  // 0-59
}

#[repr(C)]
pub struct HalRtcDriver {
    pub init: Option<unsafe extern "C" fn(config: *const c_void) -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_time: Option<unsafe extern "C" fn(dt: *mut HalDateTime) -> i32>,
    pub set_time: Option<unsafe extern "C" fn(dt: *const HalDateTime) -> i32>,
    pub is_valid: Option<unsafe extern "C" fn() -> bool>,
    pub name: *const c_char,
}

unsafe impl Send for HalRtcDriver {}
unsafe impl Sync for HalRtcDriver {}

// ── HAL Registry struct (hal/board.h: hal_registry_t) ─────────────────

#[repr(C)]
pub struct HalRegistry {
    pub display: *const HalDisplayDriver,
    pub display_config: *const c_void,
    pub inputs: [*const HalInputDriver; HAL_MAX_INPUT_DRIVERS],
    pub input_configs: [*const c_void; HAL_MAX_INPUT_DRIVERS],
    pub input_count: u8,
    pub radio: *const HalRadioDriver,
    pub radio_config: *const c_void,
    pub gps: *const HalGpsDriver,
    pub gps_config: *const c_void,
    pub audio: *const HalAudioDriver,
    pub audio_config: *const c_void,
    pub power: *const HalPowerDriver,
    pub power_config: *const c_void,
    pub imu: *const HalImuDriver,
    pub imu_config: *const c_void,
    pub storage: [*const HalStorageDriver; HAL_MAX_STORAGE_DRIVERS],
    pub storage_configs: [*const c_void; HAL_MAX_STORAGE_DRIVERS],
    pub storage_count: u8,
    pub spi_bus: [*mut c_void; 2],
    pub spi_bus_count: u8,
    pub i2c_bus: [*mut c_void; 2],
    pub i2c_bus_count: u8,
    pub crypto: *const HalCryptoDriver,
    pub rtc: *const HalRtcDriver,
    pub board_name: *const c_char,
}

unsafe impl Send for HalRegistry {}
unsafe impl Sync for HalRegistry {}

impl HalRegistry {
    pub const fn new() -> Self {
        HalRegistry {
            display: std::ptr::null(),
            display_config: std::ptr::null(),
            inputs: [std::ptr::null(); HAL_MAX_INPUT_DRIVERS],
            input_configs: [std::ptr::null(); HAL_MAX_INPUT_DRIVERS],
            input_count: 0,
            radio: std::ptr::null(),
            radio_config: std::ptr::null(),
            gps: std::ptr::null(),
            gps_config: std::ptr::null(),
            audio: std::ptr::null(),
            audio_config: std::ptr::null(),
            power: std::ptr::null(),
            power_config: std::ptr::null(),
            imu: std::ptr::null(),
            imu_config: std::ptr::null(),
            storage: [std::ptr::null(); HAL_MAX_STORAGE_DRIVERS],
            storage_configs: [std::ptr::null(); HAL_MAX_STORAGE_DRIVERS],
            storage_count: 0,
            spi_bus: [std::ptr::null_mut(); 2],
            spi_bus_count: 0,
            i2c_bus: [std::ptr::null_mut(); 2],
            i2c_bus_count: 0,
            crypto: std::ptr::null(),
            rtc: std::ptr::null(),
            board_name: std::ptr::null(),
        }
    }
}

// ── Global static registry ─────────────────────────────────────────────
//
// UnsafeCell rather than Mutex: C callers retrieve a raw pointer via
// hal_get_registry() and read fields directly, so the pointer must be stable.
// Mutation only occurs during single-threaded board-init, mirroring the C
// pattern of a plain file-scope static.

struct GlobalRegistry {
    inner: UnsafeCell<HalRegistry>,
}

// SAFETY: Only mutated during single-threaded board init.
unsafe impl Sync for GlobalRegistry {}

static REGISTRY: GlobalRegistry = GlobalRegistry {
    inner: UnsafeCell::new(HalRegistry::new()),
};

#[inline]
pub fn registry() -> &'static HalRegistry {
    unsafe { &*REGISTRY.inner.get() }
}

#[inline]
pub fn registry_mut() -> &'static mut HalRegistry {
    unsafe { &mut *REGISTRY.inner.get() }
}

/// Return a pointer to the crypto driver, or NULL if none is registered.
///
/// Replaces the C `hal_crypto_get()` shim.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn hal_crypto_get() -> *const HalCryptoDriver {
    registry().crypto
}

// ── Logging helpers ────────────────────────────────────────────────────

#[cfg(not(test))]
extern "C" {
    fn esp_log_write(level: u32, tag: *const u8, format: *const u8, ...);
}

#[cfg(not(test))]
const ESP_LOG_ERROR: u32 = 1;
#[cfg(not(test))]
const ESP_LOG_WARN: u32 = 2;
#[cfg(not(test))]
const ESP_LOG_INFO: u32 = 3;

static TAG: &[u8] = b"hal\0";

/// Return the driver name string from a raw pointer, or "(unnamed)" if null.
///
/// # Safety
/// `name_ptr` must be null or a valid null-terminated C string.
unsafe fn driver_name(name_ptr: *const c_char) -> &'static str {
    if name_ptr.is_null() {
        return "(unnamed)";
    }
    std::ffi::CStr::from_ptr(name_ptr)
        .to_str()
        .unwrap_or("(unnamed)")
}

#[cfg(not(test))]
macro_rules! hal_logi {
    ($fmt:expr, $($arg:expr),*) => {
        unsafe { esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), $fmt.as_ptr(), $($arg),*); }
    };
    ($fmt:expr) => {
        unsafe { esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), $fmt.as_ptr()); }
    };
}

#[cfg(not(test))]
macro_rules! hal_logw {
    ($fmt:expr, $($arg:expr),*) => {
        unsafe { esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), $fmt.as_ptr(), $($arg),*); }
    };
    ($fmt:expr) => {
        unsafe { esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), $fmt.as_ptr()); }
    };
}

#[cfg(not(test))]
macro_rules! hal_loge {
    ($fmt:expr, $($arg:expr),*) => {
        unsafe { esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), $fmt.as_ptr(), $($arg),*); }
    };
    ($fmt:expr) => {
        unsafe { esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), $fmt.as_ptr()); }
    };
}

// In test builds, suppress log output (no esp_log_write available on host).
#[cfg(test)]
macro_rules! hal_logi {
    ($fmt:expr $(, $arg:expr)*) => { let _ = ($($arg,)*); };
}
#[cfg(test)]
macro_rules! hal_logw {
    ($fmt:expr $(, $arg:expr)*) => { let _ = ($($arg,)*); };
}
#[cfg(test)]
macro_rules! hal_loge {
    ($fmt:expr $(, $arg:expr)*) => { let _ = ($($arg,)*); };
}

// ── Exported C functions ───────────────────────────────────────────────

/// Return a pointer to the global HAL registry.
///
/// The returned pointer is valid for the lifetime of the program and is
/// the same value on every call. C callers read fields directly.
#[no_mangle]
pub extern "C" fn hal_get_registry() -> *const HalRegistry {
    registry() as *const HalRegistry
}

/// Register the display driver.
///
/// # Safety
/// `driver` must be null or a valid `hal_display_driver_t`-compatible struct.
#[no_mangle]
pub unsafe extern "C" fn hal_display_register(
    driver: *const HalDisplayDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.display = driver;
    reg.display_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"display driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register an input driver (keyboard, touch, encoder, etc.).
///
/// # Safety
/// `driver` must be null or a valid `hal_input_driver_t`-compatible struct.
#[no_mangle]
pub unsafe extern "C" fn hal_input_register(
    driver: *const HalInputDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    if reg.input_count as usize >= HAL_MAX_INPUT_DRIVERS {
        hal_loge!(
            b"input driver registration failed: max %d drivers already registered\0",
            HAL_MAX_INPUT_DRIVERS as i32
        );
        return ESP_ERR_NO_MEM;
    }
    let idx = reg.input_count as usize;
    reg.inputs[idx] = driver;
    reg.input_configs[idx] = config;
    reg.input_count += 1;
    let name = driver_name((*driver).name);
    hal_logi!(b"input driver registered: %s (slot %d)\0", name.as_ptr(), idx as i32);
    ESP_OK
}

/// Register the radio (LoRa / FSK) driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_radio_register(
    driver: *const HalRadioDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.radio = driver;
    reg.radio_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"radio driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register the GPS driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_gps_register(
    driver: *const HalGpsDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.gps = driver;
    reg.gps_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"GPS driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register the audio driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_audio_register(
    driver: *const HalAudioDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.audio = driver;
    reg.audio_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"audio driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register the power / battery driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_power_register(
    driver: *const HalPowerDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.power = driver;
    reg.power_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"power driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register the IMU driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_imu_register(
    driver: *const HalImuDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.imu = driver;
    reg.imu_config = config;
    let name = driver_name((*driver).name);
    hal_logi!(b"IMU driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register a storage driver (SD card or internal flash).
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_storage_register(
    driver: *const HalStorageDriver,
    config: *const c_void,
) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    if reg.storage_count as usize >= HAL_MAX_STORAGE_DRIVERS {
        hal_loge!(
            b"storage driver registration failed: max %d drivers already registered\0",
            HAL_MAX_STORAGE_DRIVERS as i32
        );
        return ESP_ERR_NO_MEM;
    }
    let idx = reg.storage_count as usize;
    reg.storage[idx] = driver;
    reg.storage_configs[idx] = config;
    reg.storage_count += 1;
    let name = driver_name((*driver).name);
    hal_logi!(b"storage driver registered: %s (slot %d)\0", name.as_ptr(), idx as i32);
    ESP_OK
}

/// Register the hardware crypto accelerator driver.
///
/// # Safety
/// `driver` must be null or a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn hal_crypto_register(driver: *const HalCryptoDriver) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.crypto = driver;
    let name = driver_name((*driver).name);
    hal_logi!(b"crypto driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Register the RTC driver.
///
/// # Safety
/// `driver` must be null or a valid `hal_rtc_driver_t`-compatible struct.
#[no_mangle]
pub unsafe extern "C" fn hal_rtc_register(driver: *const HalRtcDriver) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    reg.rtc = driver;
    let name = driver_name((*driver).name);
    hal_logi!(b"RTC driver registered: %s\0", name.as_ptr());
    ESP_OK
}

/// Return a pointer to the RTC driver, or NULL if none is registered.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn hal_rtc_get() -> *const HalRtcDriver {
    registry().rtc
}

/// Set the board name string (e.g. "T-Deck Pro").
///
/// # Safety
/// `name` must be null or a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn hal_set_board_name(name: *const c_char) -> i32 {
    if name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    registry_mut().board_name = name;
    let s = driver_name(name);
    hal_logi!(b"board: %s\0", s.as_ptr());
    ESP_OK
}

/// Register a shared SPI bus handle.
///
/// # Safety
/// `bus_handle` must be a valid handle or null (returns ESP_ERR_INVALID_ARG).
#[no_mangle]
pub unsafe extern "C" fn hal_bus_register_spi(host_id: i32, bus_handle: *mut c_void) -> i32 {
    if bus_handle.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    if reg.spi_bus_count >= 2 {
        hal_loge!(b"SPI bus registration failed: max 2 buses\0");
        return ESP_ERR_NO_MEM;
    }
    let idx = reg.spi_bus_count as usize;
    reg.spi_bus[idx] = bus_handle;
    reg.spi_bus_count += 1;
    hal_logi!(b"SPI bus %d registered (host %d)\0", idx as i32, host_id);
    ESP_OK
}

/// Register a shared I2C bus handle.
///
/// # Safety
/// `bus_handle` must be a valid handle or null (returns ESP_ERR_INVALID_ARG).
#[no_mangle]
pub unsafe extern "C" fn hal_bus_register_i2c(port: i32, bus_handle: *mut c_void) -> i32 {
    if bus_handle.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let reg = registry_mut();
    if reg.i2c_bus_count >= 2 {
        hal_loge!(b"I2C bus registration failed: max 2 buses\0");
        return ESP_ERR_NO_MEM;
    }
    let idx = reg.i2c_bus_count as usize;
    reg.i2c_bus[idx] = bus_handle;
    reg.i2c_bus_count += 1;
    hal_logi!(b"I2C bus %d registered (port %d)\0", idx as i32, port);
    ESP_OK
}

/// Get a shared SPI bus handle by slot index.
///
/// Returns null if the index is out of range.
#[no_mangle]
pub extern "C" fn hal_bus_get_spi(index: i32) -> *mut c_void {
    let reg = registry();
    if index < 0 || index as u8 >= reg.spi_bus_count {
        return std::ptr::null_mut();
    }
    reg.spi_bus[index as usize]
}

/// Get a shared I2C bus handle by slot index.
///
/// Returns null if the index is out of range.
#[no_mangle]
pub extern "C" fn hal_bus_get_i2c(index: i32) -> *mut c_void {
    let reg = registry();
    if index < 0 || index as u8 >= reg.i2c_bus_count {
        return std::ptr::null_mut();
    }
    reg.i2c_bus[index as usize]
}

/// Initialise all registered drivers.
///
/// Display and input initialisation is fatal — a failure stops boot.
/// Radio, GPS, audio, power, and IMU failures are non-fatal (logged as warnings).
/// Storage is init'd then mounted; mount is skipped if init failed.
#[no_mangle]
pub extern "C" fn hal_registry_start_all() -> i32 {
    let reg = registry();

    // Display — fatal
    if !reg.display.is_null() {
        let drv = unsafe { &*reg.display };
        if let Some(init_fn) = drv.init {
            let ret = unsafe { init_fn(reg.display_config) };
            if ret != ESP_OK {
                return ret;
            }
        }
    }

    // Inputs — fatal
    for i in 0..(reg.input_count as usize) {
        if !reg.inputs[i].is_null() {
            let drv = unsafe { &*reg.inputs[i] };
            if let Some(init_fn) = drv.init {
                let ret = unsafe { init_fn(reg.input_configs[i]) };
                if ret != ESP_OK {
                    return ret;
                }
            }
        }
    }

    // Radio — non-fatal
    if !reg.radio.is_null() {
        let drv = unsafe { &*reg.radio };
        if let Some(init_fn) = drv.init {
            hal_logi!(b"Starting radio...\0");
            if unsafe { init_fn(reg.radio_config) } != ESP_OK {
                hal_logw!(b"Radio init failed (non-fatal)\0");
            }
        }
    }

    // GPS — non-fatal
    if !reg.gps.is_null() {
        let drv = unsafe { &*reg.gps };
        if let Some(init_fn) = drv.init {
            hal_logi!(b"Starting GPS...\0");
            if unsafe { init_fn(reg.gps_config) } != ESP_OK {
                hal_logw!(b"GPS init failed (non-fatal)\0");
            }
        }
    }

    // Audio — non-fatal
    if !reg.audio.is_null() {
        let drv = unsafe { &*reg.audio };
        if let Some(init_fn) = drv.init {
            hal_logi!(b"Starting audio...\0");
            if unsafe { init_fn(reg.audio_config) } != ESP_OK {
                hal_logw!(b"Audio init failed (non-fatal)\0");
            }
        }
    }

    // Power — non-fatal
    if !reg.power.is_null() {
        let drv = unsafe { &*reg.power };
        if let Some(init_fn) = drv.init {
            hal_logi!(b"Starting power...\0");
            if unsafe { init_fn(reg.power_config) } != ESP_OK {
                hal_logw!(b"Power init failed (non-fatal)\0");
            }
        }
    }

    // IMU — non-fatal
    if !reg.imu.is_null() {
        let drv = unsafe { &*reg.imu };
        if let Some(init_fn) = drv.init {
            hal_logi!(b"Starting IMU...\0");
            if unsafe { init_fn(reg.imu_config) } != ESP_OK {
                hal_logw!(b"IMU init failed (non-fatal)\0");
            }
        }
    }

    // Storage — non-fatal; skip mount if init fails
    for i in 0..(reg.storage_count as usize) {
        if !reg.storage[i].is_null() {
            let drv = unsafe { &*reg.storage[i] };
            hal_logi!(b"Starting storage[%d]...\0", i as i32);
            if let Some(init_fn) = drv.init {
                if unsafe { init_fn(reg.storage_configs[i]) } != ESP_OK {
                    hal_logw!(b"Storage[%d] init failed (non-fatal)\0", i as i32);
                    continue;
                }
            }
            if let Some(mount_fn) = drv.mount {
                hal_logi!(b"Mounting storage[%d]...\0", i as i32);
                // Mirror C: pass config pointer as mount_point string.
                unsafe { mount_fn(reg.storage_configs[i] as *const c_char) };
            }
        }
    }

    ESP_OK
}

/// De-initialise all registered drivers in reverse initialisation order.
#[no_mangle]
pub extern "C" fn hal_registry_stop_all() -> i32 {
    let reg = registry();

    // Storage — reverse order
    for i in (0..(reg.storage_count as usize)).rev() {
        if !reg.storage[i].is_null() {
            let drv = unsafe { &*reg.storage[i] };
            if let Some(deinit_fn) = drv.deinit {
                unsafe { deinit_fn() };
            }
        }
    }

    if !reg.imu.is_null() {
        if let Some(f) = unsafe { (*reg.imu).deinit } { unsafe { f() }; }
    }
    if !reg.power.is_null() {
        if let Some(f) = unsafe { (*reg.power).deinit } { unsafe { f() }; }
    }
    if !reg.audio.is_null() {
        if let Some(f) = unsafe { (*reg.audio).deinit } { unsafe { f() }; }
    }
    if !reg.gps.is_null() {
        if let Some(f) = unsafe { (*reg.gps).deinit } { unsafe { f() }; }
    }
    if !reg.radio.is_null() {
        if let Some(f) = unsafe { (*reg.radio).deinit } { unsafe { f() }; }
    }

    // Inputs — reverse order
    for i in (0..(reg.input_count as usize)).rev() {
        if !reg.inputs[i].is_null() {
            let drv = unsafe { &*reg.inputs[i] };
            if let Some(deinit_fn) = drv.deinit {
                unsafe { deinit_fn() };
            }
        }
    }

    if !reg.display.is_null() {
        if let Some(f) = unsafe { (*reg.display).deinit } { unsafe { f() }; }
    }

    ESP_OK
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_registry() {
        *registry_mut() = HalRegistry::new();
    }

    #[test]
    fn test_registry_initial_state() {
        reset_registry();
        let reg = registry();
        assert!(reg.display.is_null());
        assert_eq!(reg.input_count, 0);
        assert_eq!(reg.storage_count, 0);
        assert_eq!(reg.spi_bus_count, 0);
        assert_eq!(reg.i2c_bus_count, 0);
        assert!(reg.crypto.is_null());
        assert!(reg.rtc.is_null());
        assert!(reg.board_name.is_null());
    }

    #[test]
    fn test_null_registration_returns_invalid_arg() {
        reset_registry();
        unsafe {
            assert_eq!(hal_display_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_input_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_radio_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_gps_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_audio_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_power_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_imu_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_storage_register(std::ptr::null(), std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_crypto_register(std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_rtc_register(std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_set_board_name(std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_bus_register_spi(0, std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
            assert_eq!(hal_bus_register_i2c(0, std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_input_driver_overflow() {
        reset_registry();
        let dummy = 1usize as *const HalInputDriver;
        // Manually fill all input slots.
        {
            let reg = registry_mut();
            for i in 0..HAL_MAX_INPUT_DRIVERS {
                reg.inputs[i] = dummy;
            }
            reg.input_count = HAL_MAX_INPUT_DRIVERS as u8;
        }
        assert_eq!(
            unsafe { hal_input_register(dummy, std::ptr::null()) },
            ESP_ERR_NO_MEM
        );
    }

    #[test]
    fn test_storage_driver_overflow() {
        reset_registry();
        let dummy = 1usize as *const HalStorageDriver;
        {
            let reg = registry_mut();
            for i in 0..HAL_MAX_STORAGE_DRIVERS {
                reg.storage[i] = dummy;
            }
            reg.storage_count = HAL_MAX_STORAGE_DRIVERS as u8;
        }
        assert_eq!(
            unsafe { hal_storage_register(dummy, std::ptr::null()) },
            ESP_ERR_NO_MEM
        );
    }

    #[test]
    fn test_bus_registration_and_retrieval() {
        reset_registry();
        let mut a = 0x1234u32;
        let mut b = 0x5678u32;
        let ptr_a = &mut a as *mut u32 as *mut c_void;
        let ptr_b = &mut b as *mut u32 as *mut c_void;

        assert_eq!(unsafe { hal_bus_register_spi(0, ptr_a) }, ESP_OK);
        assert_eq!(unsafe { hal_bus_register_spi(1, ptr_b) }, ESP_OK);

        let mut c = 0u32;
        assert_eq!(
            unsafe { hal_bus_register_spi(2, &mut c as *mut u32 as *mut c_void) },
            ESP_ERR_NO_MEM
        );

        assert_eq!(hal_bus_get_spi(0), ptr_a);
        assert_eq!(hal_bus_get_spi(1), ptr_b);
        assert!(hal_bus_get_spi(2).is_null());
        assert!(hal_bus_get_spi(-1).is_null());
    }

    #[test]
    fn test_i2c_bus_registration_and_retrieval() {
        reset_registry();
        let mut a = 0xABCDu32;
        let ptr_a = &mut a as *mut u32 as *mut c_void;
        assert_eq!(unsafe { hal_bus_register_i2c(0, ptr_a) }, ESP_OK);
        assert_eq!(hal_bus_get_i2c(0), ptr_a);
        assert!(hal_bus_get_i2c(1).is_null());
        assert!(hal_bus_get_i2c(-1).is_null());
    }

    #[test]
    fn test_hal_get_registry_pointer_stable() {
        reset_registry();
        let p1 = hal_get_registry();
        let p2 = hal_get_registry();
        assert_eq!(p1, p2);
        assert!(!p1.is_null());
    }

    #[test]
    fn test_start_stop_empty_registry() {
        reset_registry();
        assert_eq!(hal_registry_start_all(), ESP_OK);
        assert_eq!(hal_registry_stop_all(), ESP_OK);
    }

    // -----------------------------------------------------------------------
    // test_display_registration_with_metadata
    // Mirrors test_hal_registry.c: display type, dimensions, and name stored.
    // -----------------------------------------------------------------------

    #[test]
    fn test_display_registration_with_metadata() {
        reset_registry();

        let drv = HalDisplayDriver {
            init: None,
            deinit: None,
            flush: None,
            refresh: None,
            set_brightness: None,
            sleep: None,
            set_refresh_mode: None,
            width: 960,
            height: 540,
            display_type: HalDisplayType::Epaper,
            name: b"test-display\0".as_ptr() as *const c_char,
        };

        let rc = unsafe { hal_display_register(&drv as *const HalDisplayDriver, std::ptr::null()) };
        assert_eq!(rc, ESP_OK, "display registration must succeed");

        let reg = registry();
        assert!(!reg.display.is_null(), "display pointer must be stored");
        let stored = unsafe { &*reg.display };
        assert_eq!(stored.width, 960, "width must be preserved");
        assert_eq!(stored.height, 540, "height must be preserved");
        assert_eq!(stored.display_type, HalDisplayType::Epaper, "display_type must be preserved");
    }

    // -----------------------------------------------------------------------
    // test_board_name_stored
    // Mirrors test_hal_registry.c: hal_set_board_name stores the pointer.
    // -----------------------------------------------------------------------

    #[test]
    fn test_board_name_stored() {
        reset_registry();

        let name: &[u8] = b"ThistleBoard v1\0";
        let rc = unsafe { hal_set_board_name(name.as_ptr() as *const c_char) };
        assert_eq!(rc, ESP_OK, "hal_set_board_name must return ESP_OK");

        let reg = registry();
        assert!(!reg.board_name.is_null(), "board_name must not be null after set");
        let stored = unsafe { std::ffi::CStr::from_ptr(reg.board_name).to_str().unwrap() };
        assert_eq!(stored, "ThistleBoard v1", "board name must match what was set");
    }

    // -----------------------------------------------------------------------
    // test_display_config_pointer_identity
    // The config void* passed to hal_display_register must be stored as-is.
    // -----------------------------------------------------------------------

    #[test]
    fn test_display_config_pointer_identity() {
        reset_registry();

        let drv = HalDisplayDriver {
            init: None,
            deinit: None,
            flush: None,
            refresh: None,
            set_brightness: None,
            sleep: None,
            set_refresh_mode: None,
            width: 128,
            height: 64,
            display_type: HalDisplayType::Epaper,
            name: b"cfg-check\0".as_ptr() as *const c_char,
        };

        let config_word: u32 = 0xDEADBEEF;
        let config_ptr: *const c_void = &config_word as *const u32 as *const c_void;

        unsafe { hal_display_register(&drv as *const HalDisplayDriver, config_ptr) };

        let reg = registry();
        assert_eq!(
            reg.display_config, config_ptr,
            "display_config must be the pointer that was passed in"
        );
    }

    // -----------------------------------------------------------------------
    // test_radio_registration
    // Mirrors test_hal_registry.c: radio pointer stored after registration.
    // -----------------------------------------------------------------------

    #[test]
    fn test_radio_registration() {
        reset_registry();

        let drv = HalRadioDriver {
            init: None,
            deinit: None,
            set_frequency: None,
            set_tx_power: None,
            set_bandwidth: None,
            set_spreading_factor: None,
            send: None,
            start_receive: None,
            stop_receive: None,
            get_rssi: None,
            sleep: None,
            name: b"test-radio\0".as_ptr() as *const c_char,
        };

        let rc = unsafe { hal_radio_register(&drv as *const HalRadioDriver, std::ptr::null()) };
        assert_eq!(rc, ESP_OK, "radio registration must succeed");
        assert!(!registry().radio.is_null(), "radio pointer must be stored");
    }

    // -----------------------------------------------------------------------
    // test_gps_registration
    // -----------------------------------------------------------------------

    #[test]
    fn test_gps_registration() {
        reset_registry();

        let drv = HalGpsDriver {
            init: None,
            deinit: None,
            enable: None,
            disable: None,
            get_position: None,
            register_callback: None,
            sleep: None,
            name: b"test-gps\0".as_ptr() as *const c_char,
        };

        let rc = unsafe { hal_gps_register(&drv as *const HalGpsDriver, std::ptr::null()) };
        assert_eq!(rc, ESP_OK, "GPS registration must succeed");
        assert!(!registry().gps.is_null(), "gps pointer must be stored");
    }

    // -----------------------------------------------------------------------
    // test_power_registration
    // -----------------------------------------------------------------------

    #[test]
    fn test_power_registration() {
        reset_registry();

        let drv = HalPowerDriver {
            init: None,
            deinit: None,
            get_info: None,
            get_battery_mv: None,
            get_battery_percent: None,
            is_charging: None,
            sleep: None,
            name: b"test-power\0".as_ptr() as *const c_char,
        };

        let rc = unsafe { hal_power_register(&drv as *const HalPowerDriver, std::ptr::null()) };
        assert_eq!(rc, ESP_OK, "power registration must succeed");
        assert!(!registry().power.is_null(), "power pointer must be stored");
    }

    // -----------------------------------------------------------------------
    // test_multi_input_registration
    // Mirrors test_hal_registry.c: multiple inputs fill sequential slots.
    // -----------------------------------------------------------------------

    #[test]
    fn test_multi_input_registration() {
        reset_registry();

        let drv0 = HalInputDriver {
            init: None,
            deinit: None,
            register_callback: None,
            poll: None,
            name: b"kbd\0".as_ptr() as *const c_char,
            is_touch: false,
        };
        let drv1 = HalInputDriver {
            init: None,
            deinit: None,
            register_callback: None,
            poll: None,
            name: b"touch\0".as_ptr() as *const c_char,
            is_touch: true,
        };

        assert_eq!(
            unsafe { hal_input_register(&drv0 as *const HalInputDriver, std::ptr::null()) },
            ESP_OK,
            "first input registration must succeed"
        );
        assert_eq!(
            unsafe { hal_input_register(&drv1 as *const HalInputDriver, std::ptr::null()) },
            ESP_OK,
            "second input registration must succeed"
        );

        let reg = registry();
        assert_eq!(reg.input_count, 2, "input_count must be 2 after two registrations");
        assert!(!reg.inputs[0].is_null(), "inputs[0] must be set");
        assert!(!reg.inputs[1].is_null(), "inputs[1] must be set");
    }
}
