// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — QST QMI8658C 6-axis IMU driver (accelerometer + gyroscope)
//
// Communicates with the QMI8658C via I2C at 0x6A or 0x6B.
// Detected at 0x6B on the LilyGo T-Deck Pro.
//
// Accelerometer:  ±8 g full scale, 100 Hz ODR → m/s²
// Gyroscope:      ±2048 dps full scale, 100 Hz ODR → deg/s
// No magnetometer; mag fields in HalImuData are always 0.0.

#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalImuCb, HalImuData, HalImuDriver};

// ── ESP error codes ───────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_FAIL: i32 = -1;

// ── QMI8658C register map ─────────────────────────────────────────────────────

pub const QMI8658C_REG_WHO_AM_I:  u8 = 0x00;
pub const QMI8658C_REG_CTRL1:     u8 = 0x02;
pub const QMI8658C_REG_CTRL2:     u8 = 0x03;
pub const QMI8658C_REG_CTRL3:     u8 = 0x04;
pub const QMI8658C_REG_CTRL5:     u8 = 0x06;
pub const QMI8658C_REG_CTRL7:     u8 = 0x08;
pub const QMI8658C_REG_STATUSINT: u8 = 0x0D;
pub const QMI8658C_REG_STATUS0:   u8 = 0x0E;
pub const QMI8658C_REG_AX_L:      u8 = 0x35;

/// Expected WHO_AM_I value for QMI8658C.
pub const QMI8658C_CHIP_ID: u8 = 0x05;

/// I2C timeout in milliseconds.
const I2C_TIMEOUT_MS: i32 = 50;

// ── Scaling constants ─────────────────────────────────────────────────────────
//
// Accel ±8 g:       sensitivity = (8.0 * 9.80665) / 32768.0  m/s² per LSB
// Gyro  ±2048 dps:  sensitivity = 2048.0 / 32768.0           dps per LSB

pub const ACCEL_SCALE: f32 = 8.0 * 9.80665 / 32768.0;
pub const GYRO_SCALE:  f32 = 2048.0 / 32768.0;

// ── ODR encoding ─────────────────────────────────────────────────────────────
//
// CTRL2 / CTRL3 bits [6:4] select the output data rate.

pub const ODR_8000:  u8 = 0x00;
pub const ODR_4000:  u8 = 0x01;
pub const ODR_2000:  u8 = 0x02;
pub const ODR_1000:  u8 = 0x03;
pub const ODR_500:   u8 = 0x04;
pub const ODR_250:   u8 = 0x05;
pub const ODR_125:   u8 = 0x06;
pub const ODR_62_5:  u8 = 0x07;
pub const ODR_31_25: u8 = 0x08;

/// Map a sample-rate in Hz to a QMI8658C ODR nibble.
///
/// Selects the nearest supported ODR at or above `hz` where possible.
/// Falls to the lowest available rate (31.25 Hz) when `hz` is very small.
pub fn hz_to_odr(hz: u16) -> u8 {
    match hz {
        h if h >= 8000 => ODR_8000,
        h if h >= 4000 => ODR_4000,
        h if h >= 2000 => ODR_2000,
        h if h >= 1000 => ODR_1000,
        h if h >= 500  => ODR_500,
        h if h >= 250  => ODR_250,
        h if h >= 125  => ODR_125,
        h if h >= 63   => ODR_62_5,
        _              => ODR_31_25,
    }
}

// ── Configuration struct ──────────────────────────────────────────────────────

/// C-compatible configuration for the QMI8658C driver.
///
/// - `i2c_bus`:  opaque `i2c_master_bus_handle_t` from the board HAL.
/// - `i2c_addr`: 7-bit I2C address; 0x6A or 0x6B (T-Deck Pro = 0x6B).
/// - `pin_int`:  GPIO number for INT1; -1 disables interrupt-driven mode.
#[repr(C)]
pub struct AccelQmi8658Config {
    pub i2c_bus:  *mut c_void,
    pub i2c_addr: u8,
    pub pin_int:  i32,
}

// SAFETY: Only primitive integers plus an opaque C pointer.
// Accessed from single-threaded board-init, mirroring the C driver model.
unsafe impl Send for AccelQmi8658Config {}
unsafe impl Sync for AccelQmi8658Config {}

// ── ESP-IDF FFI ───────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod esp_ffi {
    use std::os::raw::c_void;

    /// Partial `i2c_device_config_t`.  Must match the ESP-IDF struct layout.
    #[repr(C)]
    pub struct I2cDeviceConfig {
        /// I2C_ADDR_BIT_LEN_7 = 0
        pub dev_addr_length: u32,
        pub device_address:  u16,
        pub scl_speed_hz:    u32,
        pub scl_wait_us:     u32,
        pub flags:           u32,
    }

    extern "C" {
        pub fn i2c_master_bus_add_device(
            bus:    *mut c_void,
            cfg:    *const I2cDeviceConfig,
            handle: *mut *mut c_void,
        ) -> i32;
        pub fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
        pub fn i2c_master_transmit_receive(
            handle:     *mut c_void,
            write_data: *const u8,
            write_size: usize,
            read_data:  *mut u8,
            read_size:  usize,
            timeout_ms: i32,
        ) -> i32;
        pub fn i2c_master_transmit(
            handle:     *mut c_void,
            data:       *const u8,
            len:        usize,
            timeout_ms: i32,
        ) -> i32;
    }
}

// ── Simulator / host stubs ────────────────────────────────────────────────────

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
        pub device_address:  u16,
        pub scl_speed_hz:    u32,
        pub scl_wait_us:     u32,
        pub flags:           u32,
    }

    /// Returns a non-null sentinel so the driver can tell the add succeeded.
    pub unsafe fn i2c_master_bus_add_device(
        _bus:   *mut c_void,
        _cfg:   *const I2cDeviceConfig,
        handle: *mut *mut c_void,
    ) -> i32 {
        *handle = 1usize as *mut c_void;
        0
    }

    pub unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 { 0 }

    /// Simulate a successful I2C read.
    ///
    /// For WHO_AM_I probes (write=[0x00], read_size=1) returns 0x05 so the
    /// init sequence passes.  All other reads return zeroes (idle sensor).
    pub unsafe fn i2c_master_transmit_receive(
        _handle:     *mut c_void,
        write_data:  *const u8,
        write_size:  usize,
        read_data:   *mut u8,
        read_size:   usize,
        _timeout_ms: i32,
    ) -> i32 {
        if read_size == 0 { return 0; }
        let who_am_i_probe = write_size == 1
            && !write_data.is_null()
            && *write_data == super::QMI8658C_REG_WHO_AM_I
            && read_size == 1;
        if who_am_i_probe {
            *read_data = super::QMI8658C_CHIP_ID;
        } else {
            std::ptr::write_bytes(read_data, 0, read_size);
        }
        0
    }

    pub unsafe fn i2c_master_transmit(
        _handle:     *mut c_void,
        _data:       *const u8,
        _len:        usize,
        _timeout_ms: i32,
    ) -> i32 { 0 }
}

// ── Driver state ──────────────────────────────────────────────────────────────

struct Qmi8658State {
    cfg:         AccelQmi8658Config,
    /// Opaque `i2c_master_dev_handle_t`; null until init.
    dev:         *mut c_void,
    cb:          Option<unsafe extern "C" fn(*const HalImuData, *mut c_void)>,
    cb_data:     *mut c_void,
    initialized: bool,
}

// SAFETY: Accessed only from single-threaded board-init / HAL call path.
unsafe impl Send for Qmi8658State {}
unsafe impl Sync for Qmi8658State {}

impl Qmi8658State {
    const fn new() -> Self {
        Qmi8658State {
            cfg: AccelQmi8658Config {
                i2c_bus:  std::ptr::null_mut(),
                i2c_addr: 0x6B,
                pin_int:  -1,
            },
            dev:         std::ptr::null_mut(),
            cb:          None,
            cb_data:     std::ptr::null_mut(),
            initialized: false,
        }
    }
}

static mut S_QMI: Qmi8658State = Qmi8658State::new();

// ── I2C helpers ───────────────────────────────────────────────────────────────

/// Read `len` bytes starting at `reg` into `buf`.
unsafe fn i2c_read_regs(dev: *mut c_void, reg: u8, buf: *mut u8, len: usize) -> i32 {
    esp_ffi::i2c_master_transmit_receive(dev, &reg, 1, buf, len, I2C_TIMEOUT_MS)
}

/// Write a single register.
unsafe fn i2c_write_reg(dev: *mut c_void, reg: u8, val: u8) -> i32 {
    let buf = [reg, val];
    esp_ffi::i2c_master_transmit(dev, buf.as_ptr(), 2, I2C_TIMEOUT_MS)
}

// ── vtable implementations ────────────────────────────────────────────────────

/// Initialise the QMI8658C.
///
/// Steps:
/// 1. Copy config.
/// 2. Add I2C device to the bus at `config.i2c_addr`.
/// 3. Read WHO_AM_I (0x00) — expect 0x05.
/// 4. Configure CTRL2: accel ±8 g, 100 Hz ODR.
/// 5. Configure CTRL3: gyro ±2048 dps, 100 Hz ODR.
/// 6. Configure CTRL7: enable accel (bit 0) + gyro (bit 1) = 0x03.
///
/// # Safety
/// `config` must point to a valid `AccelQmi8658Config` or be null.
unsafe extern "C" fn qmi8658_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let src = &*(config as *const AccelQmi8658Config);
    let qmi = &mut *(&raw mut S_QMI);

    qmi.cfg.i2c_bus  = src.i2c_bus;
    qmi.cfg.i2c_addr = src.i2c_addr;
    qmi.cfg.pin_int  = src.pin_int;

    if qmi.cfg.i2c_bus.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    // ── Add I2C device ────────────────────────────────────────────────────────
    let dev_cfg = esp_ffi::I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address:  qmi.cfg.i2c_addr as u16,
        scl_speed_hz:    400_000,
        scl_wait_us:     0,
        flags:           0,
    };
    let ret = esp_ffi::i2c_master_bus_add_device(qmi.cfg.i2c_bus, &dev_cfg, &mut qmi.dev);
    if ret != ESP_OK {
        return ret;
    }

    // ── WHO_AM_I check ────────────────────────────────────────────────────────
    let mut who: u8 = 0;
    let ret = i2c_read_regs(qmi.dev, QMI8658C_REG_WHO_AM_I, &mut who, 1);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
        qmi.dev = std::ptr::null_mut();
        return ret;
    }
    if who != QMI8658C_CHIP_ID {
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
        qmi.dev = std::ptr::null_mut();
        return ESP_FAIL;
    }

    // ── CTRL2: accel ODR=100 Hz nominal, ±8 g full scale ─────────────────────
    // CTRL2 bits [6:4] = ODR code, bits [3:1] = FS code.
    // ±8 g → FS = 0b010 = 0x02
    let accel_odr = hz_to_odr(100);
    let accel_fs  = 0x02u8;
    let ctrl2 = (accel_odr << 4) | (accel_fs << 1);
    let ret = i2c_write_reg(qmi.dev, QMI8658C_REG_CTRL2, ctrl2);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
        qmi.dev = std::ptr::null_mut();
        return ret;
    }

    // ── CTRL3: gyro ODR=100 Hz nominal, ±2048 dps full scale ─────────────────
    // CTRL3 bits [6:4] = ODR code, bits [3:1] = FS code.
    // ±2048 dps → FS = 0b111 = 0x07
    let gyro_odr = hz_to_odr(100);
    let gyro_fs  = 0x07u8;
    let ctrl3 = (gyro_odr << 4) | (gyro_fs << 1);
    let ret = i2c_write_reg(qmi.dev, QMI8658C_REG_CTRL3, ctrl3);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
        qmi.dev = std::ptr::null_mut();
        return ret;
    }

    // ── CTRL7: enable accel (bit 0) + gyro (bit 1) ───────────────────────────
    let ret = i2c_write_reg(qmi.dev, QMI8658C_REG_CTRL7, 0x03);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
        qmi.dev = std::ptr::null_mut();
        return ret;
    }

    qmi.initialized = true;
    ESP_OK
}

/// De-initialise the QMI8658C: disable sensors, remove I2C device, clear state.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn qmi8658_deinit() {
    let qmi = &mut *(&raw mut S_QMI);
    if qmi.initialized && !qmi.dev.is_null() {
        // Disable both sensors before removing the I2C device.
        let _ = i2c_write_reg(qmi.dev, QMI8658C_REG_CTRL7, 0x00);
        esp_ffi::i2c_master_bus_rm_device(qmi.dev);
    }
    *(&raw mut S_QMI) = Qmi8658State::new();
}

/// Read accelerometer and gyroscope data.
///
/// Reads 12 consecutive bytes starting at AX_L (0x35):
///   AX_L, AX_H, AY_L, AY_H, AZ_L, AZ_H,
///   GX_L, GX_H, GY_L, GY_H, GZ_L, GZ_H
///
/// Converts raw 16-bit signed little-endian values:
///   - Accel: raw * ACCEL_SCALE → m/s²
///   - Gyro:  raw * GYRO_SCALE  → deg/s
///
/// Magnetometer fields are always 0.0 (QMI8658C has no magnetometer).
///
/// # Safety
/// `data` must be a writable `HalImuData` or null (returns ESP_ERR_INVALID_ARG).
unsafe extern "C" fn qmi8658_get_data(data: *mut HalImuData) -> i32 {
    if data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let qmi = &*(&raw const S_QMI);
    if !qmi.initialized || qmi.dev.is_null() {
        return ESP_ERR_INVALID_STATE;
    }

    // Read 12 bytes: AX_L(0x35)..GZ_H(0x40)
    let mut raw = [0u8; 12];
    let ret = i2c_read_regs(qmi.dev, QMI8658C_REG_AX_L, raw.as_mut_ptr(), 12);
    if ret != ESP_OK {
        return ret;
    }

    let as_i16 = |lo: u8, hi: u8| -> i16 { i16::from_le_bytes([lo, hi]) };

    let ax_raw = as_i16(raw[0],  raw[1]);
    let ay_raw = as_i16(raw[2],  raw[3]);
    let az_raw = as_i16(raw[4],  raw[5]);
    let gx_raw = as_i16(raw[6],  raw[7]);
    let gy_raw = as_i16(raw[8],  raw[9]);
    let gz_raw = as_i16(raw[10], raw[11]);

    let out = &mut *data;
    out.accel_x = ax_raw as f32 * ACCEL_SCALE;
    out.accel_y = ay_raw as f32 * ACCEL_SCALE;
    out.accel_z = az_raw as f32 * ACCEL_SCALE;
    out.gyro_x  = gx_raw as f32 * GYRO_SCALE;
    out.gyro_y  = gy_raw as f32 * GYRO_SCALE;
    out.gyro_z  = gz_raw as f32 * GYRO_SCALE;
    // No magnetometer on QMI8658C
    out.mag_x   = 0.0;
    out.mag_y   = 0.0;
    out.mag_z   = 0.0;

    ESP_OK
}

/// Register a callback invoked on each new IMU data packet.
///
/// Stores `cb` and `user_data` and returns `ESP_OK` unconditionally.
///
/// # Safety
/// `cb` must remain valid until replaced or the driver is de-initialised.
unsafe extern "C" fn qmi8658_register_callback(cb: HalImuCb, user_data: *mut c_void) -> i32 {
    let qmi = &mut *(&raw mut S_QMI);
    qmi.cb      = cb;
    qmi.cb_data = user_data;
    ESP_OK
}

/// Set the output data rate for both accel and gyro.
///
/// Maps `hz` to the nearest supported QMI8658C ODR code and writes it to
/// both CTRL2 (accel) and CTRL3 (gyro), preserving the FS bits.
///
/// Returns `ESP_ERR_INVALID_STATE` if the driver is not initialised.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn qmi8658_set_sample_rate(hz: u16) -> i32 {
    let qmi = &*(&raw const S_QMI);
    if !qmi.initialized || qmi.dev.is_null() {
        return ESP_ERR_INVALID_STATE;
    }

    let odr = hz_to_odr(hz);
    // Preserve FS: accel=±8 g (0x02), gyro=±2048 dps (0x07)
    let ctrl2 = (odr << 4) | (0x02u8 << 1);
    let ctrl3 = (odr << 4) | (0x07u8 << 1);

    let dev = qmi.dev;
    let r2 = i2c_write_reg(dev, QMI8658C_REG_CTRL2, ctrl2);
    if r2 != ESP_OK { return r2; }
    i2c_write_reg(dev, QMI8658C_REG_CTRL3, ctrl3)
}

/// Enter or leave low-power sleep mode.
///
/// - `enter = true`:  write CTRL7 = 0x00 (disable accel + gyro).
/// - `enter = false`: write CTRL7 = 0x03 (re-enable both).
///
/// Returns `ESP_ERR_INVALID_STATE` if the driver is not initialised.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn qmi8658_sleep(enter: bool) -> i32 {
    let qmi = &*(&raw const S_QMI);
    if !qmi.initialized || qmi.dev.is_null() {
        return ESP_ERR_INVALID_STATE;
    }
    let ctrl7 = if enter { 0x00u8 } else { 0x03u8 };
    i2c_write_reg(qmi.dev, QMI8658C_REG_CTRL7, ctrl7)
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL IMU driver vtable for the QMI8658C.
///
/// Pass to `hal_imu_register()`.  Returned by `drv_accel_qmi8658_get()`.
static ACCEL_DRIVER: HalImuDriver = HalImuDriver {
    init:              Some(qmi8658_init),
    deinit:            Some(qmi8658_deinit),
    get_data:          Some(qmi8658_get_data),
    register_callback: Some(qmi8658_register_callback),
    set_sample_rate:   Some(qmi8658_set_sample_rate),
    sleep:             Some(qmi8658_sleep),
    name:              b"QMI8658C\0".as_ptr() as *const c_char,
};

/// Return the QMI8658C IMU driver vtable.
///
/// # Safety
/// Returns a pointer to a program-lifetime static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_accel_qmi8658_get() -> *const HalImuDriver {
    &ACCEL_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_void;

    // ── helpers ───────────────────────────────────────────────────────────────

    unsafe fn reset_state() {
        *(&raw mut S_QMI) = Qmi8658State::new();
    }

    // ── WHO_AM_I constant ──────────────────────────────────────────────────────

    #[test]
    fn test_who_am_i_constant() {
        assert_eq!(QMI8658C_CHIP_ID, 0x05, "QMI8658C WHO_AM_I must be 0x05");
    }

    // ── Register map constants ─────────────────────────────────────────────────

    #[test]
    fn test_register_constants() {
        assert_eq!(QMI8658C_REG_WHO_AM_I, 0x00);
        assert_eq!(QMI8658C_REG_CTRL1,    0x02);
        assert_eq!(QMI8658C_REG_CTRL2,    0x03);
        assert_eq!(QMI8658C_REG_CTRL3,    0x04);
        assert_eq!(QMI8658C_REG_CTRL5,    0x06);
        assert_eq!(QMI8658C_REG_CTRL7,    0x08);
        assert_eq!(QMI8658C_REG_STATUSINT, 0x0D);
        assert_eq!(QMI8658C_REG_STATUS0,  0x0E);
        assert_eq!(QMI8658C_REG_AX_L,     0x35);
    }

    // ── Accel scaling ──────────────────────────────────────────────────────────

    #[test]
    fn test_accel_scale_nominal() {
        // 32768.0 * ACCEL_SCALE == 8.0 * 9.80665
        let result   = 32768.0f32 * ACCEL_SCALE;
        let expected = 8.0f32 * 9.80665f32;
        assert!(
            (result - expected).abs() < 1e-3,
            "32768 * ACCEL_SCALE should equal 8 * 9.80665: got {result}, expected {expected}"
        );
    }

    #[test]
    fn test_accel_scale_zero() {
        assert_eq!(0i16 as f32 * ACCEL_SCALE, 0.0);
    }

    #[test]
    fn test_accel_scale_negative_full() {
        let raw: i16 = -32768;
        let ms2 = raw as f32 * ACCEL_SCALE;
        let expected = -8.0f32 * 9.80665f32;
        assert!(
            (ms2 - expected).abs() < 1e-3,
            "negative full-scale: got {ms2}, expected {expected}"
        );
    }

    // ── Gyro scaling ───────────────────────────────────────────────────────────

    #[test]
    fn test_gyro_scale_nominal() {
        // 32768.0 * GYRO_SCALE == 2048.0
        let result   = 32768.0f32 * GYRO_SCALE;
        let expected = 2048.0f32;
        assert!(
            (result - expected).abs() < 1e-2,
            "32768 * GYRO_SCALE should equal 2048: got {result}, expected {expected}"
        );
    }

    #[test]
    fn test_gyro_scale_zero() {
        assert_eq!(0i16 as f32 * GYRO_SCALE, 0.0);
    }

    #[test]
    fn test_gyro_scale_negative_full() {
        let raw: i16 = -32768;
        let dps = raw as f32 * GYRO_SCALE;
        assert!(
            (dps - (-2048.0f32)).abs() < 1e-2,
            "negative gyro full-scale: got {dps}"
        );
    }

    // ── ODR mapping ───────────────────────────────────────────────────────────

    #[test]
    fn test_odr_mapping_exact() {
        assert_eq!(hz_to_odr(8000), ODR_8000,  "8000 Hz");
        assert_eq!(hz_to_odr(4000), ODR_4000,  "4000 Hz");
        assert_eq!(hz_to_odr(2000), ODR_2000,  "2000 Hz");
        assert_eq!(hz_to_odr(1000), ODR_1000,  "1000 Hz");
        assert_eq!(hz_to_odr(500),  ODR_500,   "500 Hz");
        assert_eq!(hz_to_odr(250),  ODR_250,   "250 Hz");
        assert_eq!(hz_to_odr(125),  ODR_125,   "125 Hz");
        assert_eq!(hz_to_odr(63),   ODR_62_5,  "63 Hz → 62.5 Hz bucket");
        assert_eq!(hz_to_odr(31),   ODR_31_25, "31 Hz");
        assert_eq!(hz_to_odr(0),    ODR_31_25, "0 Hz → lowest");
    }

    #[test]
    fn test_odr_8000_upper_boundary() {
        assert_eq!(hz_to_odr(9999), ODR_8000);
        assert_eq!(hz_to_odr(u16::MAX), ODR_8000);
    }

    #[test]
    fn test_odr_125_boundary() {
        assert_eq!(hz_to_odr(125), ODR_125);
        assert_eq!(hz_to_odr(126), ODR_125);
        assert_eq!(hz_to_odr(124), ODR_62_5);
    }

    // ── Vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_stable() {
        let p1 = drv_accel_qmi8658_get();
        let p2 = drv_accel_qmi8658_get();
        assert!(!p1.is_null());
        assert_eq!(p1, p2, "vtable pointer must be stable across calls");
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_accel_qmi8658_get() };
        assert!(drv.init.is_some(),              "init must be set");
        assert!(drv.deinit.is_some(),            "deinit must be set");
        assert!(drv.get_data.is_some(),          "get_data must be set");
        assert!(drv.register_callback.is_some(), "register_callback must be set");
        assert!(drv.set_sample_rate.is_some(),   "set_sample_rate must be set");
        assert!(drv.sleep.is_some(),             "sleep must be set");
        assert!(!drv.name.is_null(),             "name must not be null");
    }

    #[test]
    fn test_vtable_name_is_qmi8658c() {
        let drv = unsafe { &*drv_accel_qmi8658_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "QMI8658C");
    }

    // ── Init lifecycle ────────────────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(
                qmi8658_init(std::ptr::null()),
                ESP_ERR_INVALID_ARG,
                "null config must return ESP_ERR_INVALID_ARG"
            );
        }
    }

    #[test]
    fn test_init_null_bus_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = AccelQmi8658Config {
                i2c_bus:  std::ptr::null_mut(),
                i2c_addr: 0x6B,
                pin_int:  -1,
            };
            assert_eq!(
                qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void),
                ESP_ERR_INVALID_ARG,
            );
        }
    }

    #[test]
    fn test_init_valid_config_returns_ok() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            let ret = qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            assert_eq!(ret, ESP_OK, "valid init should return ESP_OK");
            let qmi = &*(&raw const S_QMI);
            assert!(qmi.initialized);
            assert_eq!(qmi.cfg.i2c_addr, 0x6B);
        }
    }

    #[test]
    fn test_init_copies_config_fields() {
        unsafe {
            reset_state();
            let bus_sentinel = 0xDEAD_BEEFusize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: bus_sentinel, i2c_addr: 0x6A, pin_int: 42 };
            qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            let qmi = &*(&raw const S_QMI);
            assert_eq!(qmi.cfg.i2c_bus,  bus_sentinel);
            assert_eq!(qmi.cfg.i2c_addr, 0x6A);
            assert_eq!(qmi.cfg.pin_int,  42);
        }
    }

    #[test]
    fn test_init_alternate_address_0x6a() {
        unsafe {
            reset_state();
            let fake_bus = 0x2usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6A, pin_int: -1 };
            let ret = qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            assert_eq!(ret, ESP_OK, "init at alternate address 0x6A should succeed");
            let qmi = &*(&raw const S_QMI);
            assert_eq!(qmi.cfg.i2c_addr, 0x6A);
        }
    }

    // ── Deinit lifecycle ──────────────────────────────────────────────────────

    #[test]
    fn test_deinit_clears_state() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            let ret = qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((&*(&raw const S_QMI)).initialized);
            qmi8658_deinit();
            let qmi = &*(&raw const S_QMI);
            assert!(!qmi.initialized,          "initialized must be false after deinit");
            assert!(qmi.cfg.i2c_bus.is_null(), "i2c_bus must be null after deinit");
            assert!(qmi.dev.is_null(),          "dev must be null after deinit");
            assert!(qmi.cb.is_none(),           "callback must be cleared after deinit");
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            qmi8658_deinit(); // must not panic
            let qmi = &*(&raw const S_QMI);
            assert!(!qmi.initialized);
        }
    }

    // ── get_data ──────────────────────────────────────────────────────────────

    #[test]
    fn test_get_data_null_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(qmi8658_get_data(std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_get_data_not_initialized_returns_invalid_state() {
        unsafe {
            reset_state();
            let mut data = HalImuData {
                accel_x: 0.0, accel_y: 0.0, accel_z: 0.0,
                gyro_x:  0.0, gyro_y:  0.0, gyro_z:  0.0,
                mag_x:   0.0, mag_y:   0.0, mag_z:   0.0,
            };
            assert_eq!(
                qmi8658_get_data(&mut data as *mut HalImuData),
                ESP_ERR_INVALID_STATE,
            );
        }
    }

    #[test]
    fn test_get_data_after_init_returns_ok() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            assert_eq!(
                qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void),
                ESP_OK
            );
            let mut data = HalImuData {
                accel_x: 0.0, accel_y: 0.0, accel_z: 0.0,
                gyro_x:  0.0, gyro_y:  0.0, gyro_z:  0.0,
                mag_x:   0.0, mag_y:   0.0, mag_z:   0.0,
            };
            assert_eq!(qmi8658_get_data(&mut data as *mut HalImuData), ESP_OK);
        }
    }

    #[test]
    fn test_get_data_mag_fields_always_zero() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            let mut data = HalImuData {
                accel_x: 1.0, accel_y: 2.0, accel_z: 3.0,
                gyro_x:  4.0, gyro_y:  5.0, gyro_z:  6.0,
                mag_x:   9.0, mag_y:   9.0, mag_z:   9.0,
            };
            qmi8658_get_data(&mut data as *mut HalImuData);
            assert_eq!(data.mag_x, 0.0, "mag_x must always be 0 (no magnetometer)");
            assert_eq!(data.mag_y, 0.0, "mag_y must always be 0");
            assert_eq!(data.mag_z, 0.0, "mag_z must always be 0");
        }
    }

    // ── register_callback ─────────────────────────────────────────────────────

    #[test]
    fn test_register_callback_returns_ok() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            let sentinel = 0xCAFE_BABEusize as *mut c_void;
            assert_eq!(qmi8658_register_callback(Some(dummy_cb), sentinel), ESP_OK);
        }
    }

    #[test]
    fn test_register_callback_stores_values() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            let sentinel = 0xCAFE_BABEusize as *mut c_void;
            qmi8658_register_callback(Some(dummy_cb), sentinel);
            let qmi = &*(&raw const S_QMI);
            assert!(qmi.cb.is_some());
            assert_eq!(qmi.cb_data, sentinel);
        }
    }

    #[test]
    fn test_register_callback_null_clears_cb() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            qmi8658_register_callback(Some(dummy_cb), 0x1usize as *mut c_void);
            qmi8658_register_callback(None, std::ptr::null_mut());
            let qmi = &*(&raw const S_QMI);
            assert!(qmi.cb.is_none());
            assert!(qmi.cb_data.is_null());
        }
    }

    // ── set_sample_rate ───────────────────────────────────────────────────────

    #[test]
    fn test_set_sample_rate_not_initialized() {
        unsafe {
            reset_state();
            assert_eq!(qmi8658_set_sample_rate(100), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_set_sample_rate_after_init() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            assert_eq!(qmi8658_set_sample_rate(500),  ESP_OK, "500 Hz");
            assert_eq!(qmi8658_set_sample_rate(100),  ESP_OK, "100 Hz");
            assert_eq!(qmi8658_set_sample_rate(8000), ESP_OK, "8000 Hz");
            assert_eq!(qmi8658_set_sample_rate(0),    ESP_OK, "0 Hz → lowest ODR");
        }
    }

    // ── sleep ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sleep_not_initialized() {
        unsafe {
            reset_state();
            assert_eq!(qmi8658_sleep(true),  ESP_ERR_INVALID_STATE);
            assert_eq!(qmi8658_sleep(false), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_sleep_enter_exit_after_init() {
        unsafe {
            reset_state();
            let fake_bus = 0x1usize as *mut c_void;
            let cfg = AccelQmi8658Config { i2c_bus: fake_bus, i2c_addr: 0x6B, pin_int: -1 };
            qmi8658_init(&cfg as *const AccelQmi8658Config as *const c_void);
            assert_eq!(qmi8658_sleep(true),  ESP_OK, "enter sleep must return ESP_OK");
            assert_eq!(qmi8658_sleep(false), ESP_OK, "exit sleep must return ESP_OK");
        }
    }
}
