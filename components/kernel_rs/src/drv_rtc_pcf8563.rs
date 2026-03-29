// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — NXP PCF8563 I2C Real-Time Clock driver (Rust)
//
// The PCF8563 is a battery-backed RTC found on the LilyGo T-Deck Pro at I2C
// address 0x55 (some revisions use 0x51).  All time registers are BCD-encoded.
// The Voltage Low (VL) flag in the seconds register indicates that a power-loss
// event may have corrupted the stored time; callers should treat the time as
// unreliable until it has been explicitly set.

#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalDateTime, HalRtcDriver};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── PCF8563 register map ─────────────────────────────────────────────────────

/// Control/Status 1 — STOP bit at [5]
const PCF8563_REG_CTRL1: u8 = 0x00;
/// Control/Status 2 — alarm/timer flags
const PCF8563_REG_CTRL2: u8 = 0x01;
/// Seconds (BCD).  Bit 7 = VL (Voltage Low / clock integrity flag)
const PCF8563_REG_SECONDS: u8 = 0x02;
/// Minutes (BCD)
const PCF8563_REG_MINUTES: u8 = 0x03;
/// Hours (BCD, 24-hour)
const PCF8563_REG_HOURS: u8 = 0x04;
/// Days (BCD, 1-31)
const PCF8563_REG_DAYS: u8 = 0x05;
/// Weekdays (0-6)
const PCF8563_REG_WEEKDAYS: u8 = 0x06;
/// Months/century (BCD months in [4:0], century in [7])
const PCF8563_REG_MONTHS: u8 = 0x07;
/// Years (BCD, 00-99)
const PCF8563_REG_YEARS: u8 = 0x08;

/// STOP bit in Control/Status 1 — set to stop the clock oscillator
const PCF8563_CTRL1_STOP: u8 = 1 << 5;

/// VL (Voltage Low) flag in the seconds register — indicates possible time loss
const PCF8563_VL_FLAG: u8 = 1 << 7;

/// Mask for BCD seconds value (bits 6:0)
const PCF8563_SECONDS_MASK: u8 = 0x7F;
/// Mask for BCD minutes value (bits 6:0)
const PCF8563_MINUTES_MASK: u8 = 0x7F;
/// Mask for BCD hours value (bits 5:0)
const PCF8563_HOURS_MASK: u8 = 0x3F;
/// Mask for BCD days value (bits 5:0)
const PCF8563_DAYS_MASK: u8 = 0x3F;
/// Mask for weekdays value (bits 2:0)
const PCF8563_WEEKDAYS_MASK: u8 = 0x07;
/// Mask for BCD months value (bits 4:0)
const PCF8563_MONTHS_MASK: u8 = 0x1F;
/// Century bit in the months register — set = year 2000+
const PCF8563_CENTURY_BIT: u8 = 1 << 7;

// ── BCD conversion ───────────────────────────────────────────────────────────

/// Convert a BCD-encoded byte to a plain binary integer.
///
/// # Examples
/// ```
/// assert_eq!(drv_rtc_pcf8563::bcd_to_bin(0x59), 59);
/// assert_eq!(drv_rtc_pcf8563::bcd_to_bin(0x00), 0);
/// ```
#[inline]
pub fn bcd_to_bin(bcd: u8) -> u8 {
    (bcd >> 4) * 10 + (bcd & 0x0F)
}

/// Convert a plain binary integer to BCD encoding.
///
/// # Examples
/// ```
/// assert_eq!(drv_rtc_pcf8563::bin_to_bcd(59), 0x59);
/// assert_eq!(drv_rtc_pcf8563::bin_to_bcd(0), 0x00);
/// ```
#[inline]
pub fn bin_to_bcd(bin: u8) -> u8 {
    ((bin / 10) << 4) | (bin % 10)
}

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible config struct for the PCF8563 driver.
///
/// Must be cast from `const void *` in the vtable `init` call.
#[repr(C)]
pub struct RtcPcf8563Config {
    /// i2c_master_bus_handle_t — shared bus handle from the HAL registry.
    pub i2c_bus: *mut c_void,
    /// 7-bit I2C address.  T-Deck Pro uses 0x55; some boards use 0x51.
    pub i2c_addr: u8,
}

// SAFETY: Config holds an opaque C bus handle; mutation only during single-
// threaded driver init.
unsafe impl Send for RtcPcf8563Config {}
unsafe impl Sync for RtcPcf8563Config {}

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod esp_ffi {
    use std::os::raw::c_void;

    /// i2c_device_config_t — partial layout matching ESP-IDF v5.x on Xtensa.
    #[repr(C)]
    pub struct I2cDeviceConfig {
        /// dev_addr_length: I2C_ADDR_BIT_LEN_7 = 0
        pub dev_addr_length: u32,
        pub device_address: u16,
        pub scl_speed_hz: u32,
        pub scl_wait_us: u32,
        pub flags: u32,
    }

    extern "C" {
        pub fn i2c_master_bus_add_device(
            bus: *mut c_void,
            cfg: *const I2cDeviceConfig,
            handle: *mut *mut c_void,
        ) -> i32;
        pub fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
        pub fn i2c_master_transmit_receive(
            handle: *mut c_void,
            write_data: *const u8,
            write_size: usize,
            read_data: *mut u8,
            read_size: usize,
            timeout_ms: i32,
        ) -> i32;
        pub fn i2c_master_transmit(
            handle: *mut c_void,
            data: *const u8,
            len: usize,
            timeout_ms: i32,
        ) -> i32;
    }
}

// ── Stub impls for host tests / simulator ────────────────────────────────────

#[cfg(all(not(target_os = "espidf"), feature = "sim-bus"))]
mod esp_ffi {
    use std::os::raw::c_void;

    #[repr(C)]
    pub struct I2cDeviceConfig {
        pub dev_addr_length: u32,
        pub device_address: u16,
        pub scl_speed_hz: u32,
        pub scl_wait_us: u32,
        pub flags: u32,
    }

    extern "C" {
        pub fn i2c_master_bus_add_device(
            bus: *mut c_void,
            cfg: *const I2cDeviceConfig,
            handle: *mut *mut c_void,
        ) -> i32;
        pub fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
        pub fn i2c_master_transmit_receive(
            handle: *mut c_void,
            write_data: *const u8,
            write_size: usize,
            read_data: *mut u8,
            read_size: usize,
            timeout_ms: i32,
        ) -> i32;
        pub fn i2c_master_transmit(
            handle: *mut c_void,
            data: *const u8,
            len: usize,
            timeout_ms: i32,
        ) -> i32;
    }
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
mod esp_ffi {
    use std::os::raw::c_void;

    #[repr(C)]
    pub struct I2cDeviceConfig {
        pub dev_addr_length: u32,
        pub device_address: u16,
        pub scl_speed_hz: u32,
        pub scl_wait_us: u32,
        pub flags: u32,
    }

    pub unsafe fn i2c_master_bus_add_device(
        _bus: *mut c_void,
        _cfg: *const I2cDeviceConfig,
        handle: *mut *mut c_void,
    ) -> i32 {
        // Return a non-null sentinel so the driver can tell "success".
        *handle = 1usize as *mut c_void;
        0
    }
    pub unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 {
        0
    }
    pub unsafe fn i2c_master_transmit_receive(
        _handle: *mut c_void,
        _write: *const u8,
        _wsz: usize,
        read: *mut u8,
        rsz: usize,
        _timeout: i32,
    ) -> i32 {
        // Return all-zeros: VL clear, clock not stopped, time = 00:00:00 2000-01-01
        unsafe { std::ptr::write_bytes(read, 0, rsz) };
        0
    }
    pub unsafe fn i2c_master_transmit(
        _handle: *mut c_void,
        _data: *const u8,
        _len: usize,
        _timeout: i32,
    ) -> i32 {
        0
    }
}

// ── Driver state ─────────────────────────────────────────────────────────────

struct RtcState {
    /// I2C device handle (i2c_master_dev_handle_t)
    dev: *mut c_void,
    cfg: RtcPcf8563Config,
    initialized: bool,
}

// SAFETY: The driver state is accessed only from single-threaded HAL init/deinit
// paths, mirroring the C driver pattern.
unsafe impl Send for RtcState {}
unsafe impl Sync for RtcState {}

impl RtcState {
    const fn new() -> Self {
        RtcState {
            dev: std::ptr::null_mut(),
            cfg: RtcPcf8563Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x55,
            },
            initialized: false,
        }
    }
}

static mut S_RTC: RtcState = RtcState::new();

// ── I2C helpers ──────────────────────────────────────────────────────────────

/// Write a single register on the PCF8563.
///
/// # Safety
/// `S_RTC.dev` must be a valid I2C device handle.
unsafe fn pcf8563_write_reg(reg: u8, val: u8) -> i32 {
    let buf: [u8; 2] = [reg, val];
    esp_ffi::i2c_master_transmit(S_RTC.dev, buf.as_ptr(), 2, 50)
}

/// Read `count` consecutive registers starting at `reg` into `buf`.
///
/// # Safety
/// `S_RTC.dev` must be valid; `buf` must have room for `count` bytes.
unsafe fn pcf8563_read_regs(reg: u8, buf: &mut [u8]) -> i32 {
    esp_ffi::i2c_master_transmit_receive(
        S_RTC.dev,
        &reg as *const u8,
        1,
        buf.as_mut_ptr(),
        buf.len(),
        50,
    )
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the PCF8563 driver.
///
/// Adds an I2C device, then verifies that the oscillator is running
/// (STOP bit in Control/Status 1 must be clear).
///
/// # Safety
/// `config` must point to a valid `RtcPcf8563Config`.
unsafe extern "C" fn pcf8563_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let rtc = &mut *(&raw mut S_RTC);

    if rtc.initialized {
        return ESP_OK; // idempotent
    }

    let src = &*(config as *const RtcPcf8563Config);
    if src.i2c_bus.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    rtc.cfg.i2c_bus = src.i2c_bus;
    rtc.cfg.i2c_addr = src.i2c_addr;

    // Add the I2C device at 400 kHz (PCF8563 supports up to 400 kHz)
    let dev_cfg = esp_ffi::I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: rtc.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
        scl_wait_us: 0,
        flags: 0,
    };
    let ret = esp_ffi::i2c_master_bus_add_device(rtc.cfg.i2c_bus, &dev_cfg, &mut rtc.dev);
    if ret != ESP_OK {
        return ret;
    }

    // Read Control/Status 1 and ensure the STOP bit is clear.
    // If the clock was stopped (e.g. first power-on), start it.
    let mut ctrl1: u8 = 0;
    let ret = esp_ffi::i2c_master_transmit_receive(
        rtc.dev,
        &PCF8563_REG_CTRL1 as *const u8,
        1,
        &mut ctrl1,
        1,
        50,
    );
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(rtc.dev);
        rtc.dev = std::ptr::null_mut();
        return ret;
    }

    if (ctrl1 & PCF8563_CTRL1_STOP) != 0 {
        // Clock was stopped — clear the STOP bit to start it running.
        let ret = pcf8563_write_reg(PCF8563_REG_CTRL1, ctrl1 & !PCF8563_CTRL1_STOP);
        if ret != ESP_OK {
            esp_ffi::i2c_master_bus_rm_device(rtc.dev);
            rtc.dev = std::ptr::null_mut();
            return ret;
        }
    }

    rtc.initialized = true;
    ESP_OK
}

/// De-initialise the PCF8563 driver and release the I2C device handle.
unsafe extern "C" fn pcf8563_deinit() {
    let rtc = &mut *(&raw mut S_RTC);
    if !rtc.initialized {
        return;
    }

    if !rtc.dev.is_null() {
        esp_ffi::i2c_master_bus_rm_device(rtc.dev);
        rtc.dev = std::ptr::null_mut();
    }

    rtc.initialized = false;
}

/// Read the current date and time from the PCF8563.
///
/// Reads registers 0x02–0x08 (7 bytes) in a single I2C transaction,
/// converts from BCD to binary, and applies the century bit.
///
/// # Safety
/// `dt` must be a valid non-null pointer to a `HalDateTime`.
unsafe extern "C" fn pcf8563_get_time(dt: *mut HalDateTime) -> i32 {
    if dt.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let rtc = &*(&raw const S_RTC);
    if !rtc.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Read 7 bytes starting at register 0x02 (seconds) through 0x08 (years).
    let mut regs = [0u8; 7];
    let ret = pcf8563_read_regs(PCF8563_REG_SECONDS, &mut regs);
    if ret != ESP_OK {
        return ret;
    }

    // regs[0] = seconds (VL in bit 7)
    // regs[1] = minutes
    // regs[2] = hours
    // regs[3] = days
    // regs[4] = weekdays
    // regs[5] = months/century
    // regs[6] = years (BCD 00-99)

    let out = &mut *dt;
    out.second  = bcd_to_bin(regs[0] & PCF8563_SECONDS_MASK);
    out.minute  = bcd_to_bin(regs[1] & PCF8563_MINUTES_MASK);
    out.hour    = bcd_to_bin(regs[2] & PCF8563_HOURS_MASK);
    out.day     = bcd_to_bin(regs[3] & PCF8563_DAYS_MASK);
    out.weekday = regs[4] & PCF8563_WEEKDAYS_MASK;
    out.month   = bcd_to_bin(regs[5] & PCF8563_MONTHS_MASK);

    // Years register holds BCD 00-99; the century bit in the months register
    // indicates which century: clear = 1900s, set = 2000s.
    let year_bcd = regs[6];
    let century: u16 = if (regs[5] & PCF8563_CENTURY_BIT) != 0 {
        2000
    } else {
        1900
    };
    out.year = century + bcd_to_bin(year_bcd) as u16;

    ESP_OK
}

/// Set the PCF8563 to the given date and time.
///
/// Converts binary values to BCD and writes registers 0x02–0x08.
/// Clears the VL flag in the seconds register to mark the time as valid.
///
/// # Safety
/// `dt` must be a valid non-null pointer to a `HalDateTime`.
unsafe extern "C" fn pcf8563_set_time(dt: *const HalDateTime) -> i32 {
    if dt.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let rtc = &*(&raw const S_RTC);
    if !rtc.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let t = &*dt;

    // Determine century bit: years 2000-2099 → set, 1900-1999 → clear.
    let century_bit: u8 = if t.year >= 2000 {
        PCF8563_CENTURY_BIT
    } else {
        0
    };
    let year_2digit = (t.year % 100) as u8;

    // Build 8-byte write: register address followed by 7 data bytes.
    let buf: [u8; 8] = [
        PCF8563_REG_SECONDS,
        // Bit 7 (VL) = 0: clear the voltage-low flag to mark time as valid.
        bin_to_bcd(t.second) & PCF8563_SECONDS_MASK,
        bin_to_bcd(t.minute) & PCF8563_MINUTES_MASK,
        bin_to_bcd(t.hour)   & PCF8563_HOURS_MASK,
        bin_to_bcd(t.day)    & PCF8563_DAYS_MASK,
        t.weekday & PCF8563_WEEKDAYS_MASK,
        (bin_to_bcd(t.month) & PCF8563_MONTHS_MASK) | century_bit,
        bin_to_bcd(year_2digit),
    ];

    esp_ffi::i2c_master_transmit(S_RTC.dev, buf.as_ptr(), buf.len(), 50)
}

/// Check whether the stored time is trustworthy.
///
/// Reads the seconds register and checks the VL (Voltage Low) bit.
/// Returns `true` when the time is valid, `false` when it may have been lost.
unsafe extern "C" fn pcf8563_is_valid() -> bool {
    let rtc = &*(&raw const S_RTC);
    if !rtc.initialized {
        return false;
    }

    let mut seconds: u8 = 0;
    let ret = esp_ffi::i2c_master_transmit_receive(
        rtc.dev,
        &PCF8563_REG_SECONDS as *const u8,
        1,
        &mut seconds,
        1,
        50,
    );
    if ret != ESP_OK {
        return false;
    }

    // VL = 0 → clock integrity guaranteed; VL = 1 → time may be unreliable.
    (seconds & PCF8563_VL_FLAG) == 0
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL RTC driver vtable for the PCF8563.
static RTC_DRIVER: HalRtcDriver = HalRtcDriver {
    init: Some(pcf8563_init),
    deinit: Some(pcf8563_deinit),
    get_time: Some(pcf8563_get_time),
    set_time: Some(pcf8563_set_time),
    is_valid: Some(pcf8563_is_valid),
    name: b"PCF8563\0".as_ptr() as *const c_char,
};

/// Return the PCF8563 driver vtable pointer.
///
/// Drop-in replacement for a hypothetical C `drv_rtc_pcf8563_get()`.
///
/// # Safety
/// Returns a pointer to a program-lifetime static — always safe to call from C.
#[no_mangle]
pub extern "C" fn drv_rtc_pcf8563_get() -> *const HalRtcDriver {
    &RTC_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn reset_state() {
        *(&raw mut S_RTC) = RtcState::new();
    }

    // ── BCD conversion ────────────────────────────────────────────────────────

    #[test]
    fn test_bcd_to_bin_known_values() {
        assert_eq!(bcd_to_bin(0x59), 59);
        assert_eq!(bcd_to_bin(0x00), 0);
        assert_eq!(bcd_to_bin(0x12), 12);
        assert_eq!(bcd_to_bin(0x99), 99);
        assert_eq!(bcd_to_bin(0x23), 23);
    }

    #[test]
    fn test_bin_to_bcd_known_values() {
        assert_eq!(bin_to_bcd(59), 0x59);
        assert_eq!(bin_to_bcd(0), 0x00);
        assert_eq!(bin_to_bcd(12), 0x12);
        assert_eq!(bin_to_bcd(99), 0x99);
        assert_eq!(bin_to_bcd(23), 0x23);
    }

    #[test]
    fn test_bcd_bin_roundtrip() {
        for v in 0u8..=99u8 {
            assert_eq!(bcd_to_bin(bin_to_bcd(v)), v, "roundtrip failed for {}", v);
        }
    }

    #[test]
    fn test_bin_bcd_roundtrip() {
        // All valid BCD bytes: tens digit 0-9, units digit 0-9
        for tens in 0u8..10 {
            for units in 0u8..10 {
                let bcd = (tens << 4) | units;
                assert_eq!(bin_to_bcd(bcd_to_bin(bcd)), bcd,
                    "roundtrip failed for BCD 0x{:02X}", bcd);
            }
        }
    }

    // ── Vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_non_null() {
        let p = drv_rtc_pcf8563_get();
        assert!(!p.is_null());
    }

    #[test]
    fn test_vtable_pointer_stable() {
        let p1 = drv_rtc_pcf8563_get();
        let p2 = drv_rtc_pcf8563_get();
        assert_eq!(p1, p2, "vtable pointer must be stable across calls");
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_rtc_pcf8563_get() };
        assert!(drv.init.is_some(), "init must be set");
        assert!(drv.deinit.is_some(), "deinit must be set");
        assert!(drv.get_time.is_some(), "get_time must be set");
        assert!(drv.set_time.is_some(), "set_time must be set");
        assert!(drv.is_valid.is_some(), "is_valid must be set");
        assert!(!drv.name.is_null(), "name must not be null");
    }

    #[test]
    fn test_vtable_name_is_pcf8563() {
        let drv = unsafe { &*drv_rtc_pcf8563_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name).to_str().unwrap() };
        assert_eq!(name, "PCF8563");
    }

    // ── Init / deinit lifecycle ───────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            let ret = pcf8563_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_null_bus_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x55,
            };
            let ret = pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void, // non-null sentinel
                i2c_addr: 0x55,
            };
            let ret = pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_RTC)).initialized);

            pcf8563_deinit();
            assert!(!(*(&raw const S_RTC)).initialized);
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            let p = &cfg as *const RtcPcf8563Config as *const c_void;
            assert_eq!(pcf8563_init(p), ESP_OK);
            assert_eq!(pcf8563_init(p), ESP_OK); // second call must succeed too
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            pcf8563_deinit(); // must not panic
            assert!(!(*(&raw const S_RTC)).initialized);
        }
    }

    // ── get_time / set_time ───────────────────────────────────────────────────

    #[test]
    fn test_get_time_null_dt_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            let ret = pcf8563_get_time(std::ptr::null_mut());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            pcf8563_deinit();
        }
    }

    #[test]
    fn test_get_time_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let mut dt = HalDateTime {
                year: 0, month: 0, day: 0, weekday: 0,
                hour: 0, minute: 0, second: 0,
            };
            let ret = pcf8563_get_time(&mut dt as *mut HalDateTime);
            assert_eq!(ret, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_set_time_null_dt_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            let ret = pcf8563_set_time(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            pcf8563_deinit();
        }
    }

    #[test]
    fn test_set_time_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let dt = HalDateTime {
                year: 2026, month: 3, day: 22, weekday: 0,
                hour: 12, minute: 0, second: 0,
            };
            let ret = pcf8563_set_time(&dt as *const HalDateTime);
            assert_eq!(ret, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_get_time_returns_ok_after_init() {
        // Stub returns all-zeros: seconds=0x00 (VL=0), month/year = 0 BCD.
        // After decoding: 00:00:00, 1900-01-00 (month/day = bcd_to_bin(0) = 0).
        // We just check the call succeeds.
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            let mut dt = HalDateTime {
                year: 0, month: 0, day: 0, weekday: 0,
                hour: 0, minute: 0, second: 0,
            };
            let ret = pcf8563_get_time(&mut dt as *mut HalDateTime);
            assert_eq!(ret, ESP_OK);
            // Stub returns zeros; century bit absent → 1900 + 0 = 1900.
            assert_eq!(dt.year, 1900);
            assert_eq!(dt.second, 0);
            assert_eq!(dt.minute, 0);
            assert_eq!(dt.hour, 0);
            pcf8563_deinit();
        }
    }

    #[test]
    fn test_set_time_returns_ok_after_init() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            let dt = HalDateTime {
                year: 2026,
                month: 3,
                day: 22,
                weekday: 0,
                hour: 15,
                minute: 30,
                second: 45,
            };
            let ret = pcf8563_set_time(&dt as *const HalDateTime);
            assert_eq!(ret, ESP_OK);
            pcf8563_deinit();
        }
    }

    // ── is_valid ──────────────────────────────────────────────────────────────

    #[test]
    fn test_is_valid_before_init_returns_false() {
        unsafe {
            reset_state();
            assert!(!pcf8563_is_valid());
        }
    }

    #[test]
    fn test_is_valid_after_init_stub_returns_true() {
        // Stub i2c_master_transmit_receive fills with 0x00.
        // VL bit (bit 7) of seconds register = 0 → time is valid.
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x55,
            };
            pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            assert!(pcf8563_is_valid());
            pcf8563_deinit();
        }
    }

    // ── BCD encoding in set_time ──────────────────────────────────────────────

    #[test]
    fn test_century_bit_set_for_year_2000_plus() {
        // Verify bin_to_bcd and century logic used in set_time.
        let year: u16 = 2026;
        let century_bit: u8 = if year >= 2000 { PCF8563_CENTURY_BIT } else { 0 };
        assert_ne!(century_bit, 0, "century bit must be set for year 2026");
        assert_eq!(bin_to_bcd((year % 100) as u8), 0x26);
    }

    #[test]
    fn test_century_bit_clear_for_year_1999() {
        let year: u16 = 1999;
        let century_bit: u8 = if year >= 2000 { PCF8563_CENTURY_BIT } else { 0 };
        assert_eq!(century_bit, 0, "century bit must be clear for year 1999");
        assert_eq!(bin_to_bcd((year % 100) as u8), 0x99);
    }

    // ── alternate I2C address ─────────────────────────────────────────────────

    #[test]
    fn test_alternate_address_0x51() {
        unsafe {
            reset_state();
            let cfg = RtcPcf8563Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x51,
            };
            let ret = pcf8563_init(&cfg as *const RtcPcf8563Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert_eq!((*(&raw const S_RTC)).cfg.i2c_addr, 0x51);
            pcf8563_deinit();
        }
    }

    // ── HalDateTime struct ────────────────────────────────────────────────────

    #[test]
    fn test_hal_datetime_fields() {
        let dt = HalDateTime {
            year: 2026,
            month: 12,
            day: 31,
            weekday: 6,
            hour: 23,
            minute: 59,
            second: 59,
        };
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 12);
        assert_eq!(dt.day, 31);
        assert_eq!(dt.weekday, 6);
        assert_eq!(dt.hour, 23);
        assert_eq!(dt.minute, 59);
        assert_eq!(dt.second, 59);
    }
}
