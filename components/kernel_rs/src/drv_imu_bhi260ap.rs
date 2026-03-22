// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Bosch BHI260AP IMU driver (Rust stub)
//
// Rust port of components/drv_imu_bhi260ap/src/drv_imu_bhi260ap.c.
//
// This is a stub driver — all hardware operations return ESP_ERR_NOT_SUPPORTED
// with TODO markers for the real implementation.  The register_callback vtable
// entry stores the callback and user-data pointer and returns ESP_OK, matching
// the C stub's behaviour exactly.

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalImuCb, HalImuData, HalImuDriver};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible configuration for the BHI260AP driver.
///
/// Must match `imu_bhi260ap_config_t` in `drv_imu_bhi260ap.h`.
///
/// - `i2c_bus`:  opaque `i2c_master_bus_handle_t` from the board HAL.
/// - `i2c_addr`: 7-bit I2C address; default 0x28.
/// - `pin_int`:  GPIO number for the BHI260AP interrupt line (`gpio_num_t`).
#[repr(C)]
pub struct ImuBhi260apConfig {
    /// Opaque ESP-IDF `i2c_master_bus_handle_t`.
    pub i2c_bus: *mut c_void,
    /// 7-bit I2C device address (default 0x28).
    pub i2c_addr: u8,
    /// Interrupt GPIO number (`gpio_num_t`).
    pub pin_int: i32,
}

// SAFETY: Config holds only primitive integers and an opaque pointer.
// Accessed only from single-threaded board-init, mirroring the C driver.
unsafe impl Send for ImuBhi260apConfig {}
unsafe impl Sync for ImuBhi260apConfig {}

// ── Driver state ─────────────────────────────────────────────────────────────

struct ImuState {
    cfg: ImuBhi260apConfig,
    cb: Option<unsafe extern "C" fn(*const HalImuData, *mut c_void)>,
    cb_data: *mut c_void,
}

// SAFETY: The driver state is only mutated during single-threaded board-init
// and from the single interrupt/task context, mirroring the C static pattern.
unsafe impl Send for ImuState {}
unsafe impl Sync for ImuState {}

impl ImuState {
    const fn new() -> Self {
        ImuState {
            cfg: ImuBhi260apConfig {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x28,
                pin_int: -1,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
        }
    }
}

static mut S_IMU: ImuState = ImuState::new();

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the BHI260AP.
///
/// TODO: Add I2C device to bus, check chip ID register (0x2B == BHI260AP),
///       upload firmware blob if required, configure virtual sensor list,
///       install ISR on pin_int.
///
/// # Safety
/// `config` must point to a valid `ImuBhi260apConfig` or be null.
unsafe extern "C" fn bhi260ap_init(config: *const c_void) -> i32 {
    if !config.is_null() {
        let src = &*(config as *const ImuBhi260apConfig);
        let imu = &mut *(&raw mut S_IMU);
        imu.cfg.i2c_bus  = src.i2c_bus;
        imu.cfg.i2c_addr = src.i2c_addr;
        imu.cfg.pin_int  = src.pin_int;
    }
    ESP_ERR_NOT_SUPPORTED
}

/// De-initialise the BHI260AP.
///
/// TODO: Remove ISR, remove I2C device, clear state.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn bhi260ap_deinit() {
    // TODO: Remove ISR, remove I2C device, clear state.
    let imu = &mut *(&raw mut S_IMU);
    imu.cfg.i2c_bus  = std::ptr::null_mut();
    imu.cfg.i2c_addr = 0x28;
    imu.cfg.pin_int  = -1;
    imu.cb      = None;
    imu.cb_data = std::ptr::null_mut();
}

/// Read IMU data.
///
/// TODO: Read FIFO or status registers; parse accel, gyro, mag virtual
///       sensor output packets.
///
/// # Safety
/// `data` must be a writable `HalImuData` or null.
unsafe extern "C" fn bhi260ap_get_data(_data: *mut HalImuData) -> i32 {
    // TODO: Read FIFO or status registers; parse accel, gyro, mag virtual
    //       sensor output packets.
    ESP_ERR_NOT_SUPPORTED
}

/// Register a callback invoked on each new IMU data packet.
///
/// Stores `cb` and `user_data` unconditionally and returns `ESP_OK`,
/// matching the C stub behaviour exactly.
///
/// # Safety
/// `cb` must remain valid until replaced or the driver is de-initialised.
unsafe extern "C" fn bhi260ap_register_callback(cb: HalImuCb, user_data: *mut c_void) -> i32 {
    let imu = &mut *(&raw mut S_IMU);
    imu.cb      = cb;
    imu.cb_data = user_data;
    ESP_OK
}

/// Configure the virtual sensor sample rate.
///
/// TODO: Configure virtual sensor sample rate via BHY2 host interface.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn bhi260ap_set_sample_rate(_hz: u16) -> i32 {
    // TODO: Configure virtual sensor sample rate via BHY2 host interface.
    ESP_ERR_NOT_SUPPORTED
}

/// Enter or leave low-power sleep mode.
///
/// TODO: Issue sleep/wakeup command via host interface.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn bhi260ap_sleep(_enter: bool) -> i32 {
    // TODO: Issue sleep/wakeup command via host interface.
    ESP_ERR_NOT_SUPPORTED
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL IMU driver vtable for the BHI260AP.
///
/// Pass to `hal_imu_register()`.  Returned by `drv_imu_bhi260ap_get()`.
static IMU_DRIVER: HalImuDriver = HalImuDriver {
    init:              Some(bhi260ap_init),
    deinit:            Some(bhi260ap_deinit),
    get_data:          Some(bhi260ap_get_data),
    register_callback: Some(bhi260ap_register_callback),
    set_sample_rate:   Some(bhi260ap_set_sample_rate),
    sleep:             Some(bhi260ap_sleep),
    name:              b"BHI260AP\0".as_ptr() as *const c_char,
};

/// Return the BHI260AP IMU driver vtable.
///
/// Drop-in replacement for the C `drv_imu_bhi260ap_get()`.
///
/// # Safety
/// Returns a pointer to a program-lifetime static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_imu_bhi260ap_get() -> *const HalImuDriver {
    &IMU_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_IMU) = ImuState::new();
    }

    // ── vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_stable() {
        // Same pointer returned on every call — pointer identity, not just non-null.
        let p1 = drv_imu_bhi260ap_get();
        let p2 = drv_imu_bhi260ap_get();
        assert!(!p1.is_null());
        assert_eq!(p1, p2, "vtable pointer must be stable across calls");
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_imu_bhi260ap_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.get_data.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.set_sample_rate.is_some());
        assert!(drv.sleep.is_some());
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_vtable_name_is_bhi260ap() {
        let drv = unsafe { &*drv_imu_bhi260ap_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "BHI260AP");
    }

    // ── init ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_init_returns_not_supported() {
        unsafe {
            reset_state();
            let cfg = ImuBhi260apConfig {
                i2c_bus: 0x1usize as *mut c_void,
                i2c_addr: 0x28,
                pin_int: 5,
            };
            assert_eq!(
                bhi260ap_init(&cfg as *const ImuBhi260apConfig as *const c_void),
                ESP_ERR_NOT_SUPPORTED,
            );
        }
    }

    #[test]
    fn test_init_null_config_returns_not_supported() {
        unsafe {
            reset_state();
            // Null config is tolerated (skips copy) and still returns NOT_SUPPORTED.
            assert_eq!(bhi260ap_init(std::ptr::null()), ESP_ERR_NOT_SUPPORTED);
        }
    }

    #[test]
    fn test_init_copies_config() {
        unsafe {
            reset_state();
            let bus_sentinel = 0xDEAD_BEEFusize as *mut c_void;
            let cfg = ImuBhi260apConfig {
                i2c_bus:  bus_sentinel,
                i2c_addr: 0x29,
                pin_int:  42,
            };
            bhi260ap_init(&cfg as *const ImuBhi260apConfig as *const c_void);
            let imu = &*(&raw const S_IMU);
            assert_eq!(imu.cfg.i2c_bus,  bus_sentinel);
            assert_eq!(imu.cfg.i2c_addr, 0x29);
            assert_eq!(imu.cfg.pin_int,  42);
        }
    }

    // ── get_data ──────────────────────────────────────────────────────────────

    #[test]
    fn test_get_data_returns_not_supported() {
        unsafe {
            reset_state();
            let mut data = HalImuData {
                accel_x: 0.0, accel_y: 0.0, accel_z: 0.0,
                gyro_x:  0.0, gyro_y:  0.0, gyro_z:  0.0,
                mag_x:   0.0, mag_y:   0.0, mag_z:   0.0,
            };
            assert_eq!(
                bhi260ap_get_data(&mut data as *mut HalImuData),
                ESP_ERR_NOT_SUPPORTED,
            );
        }
    }

    // ── register_callback ─────────────────────────────────────────────────────

    #[test]
    fn test_register_callback_returns_ok() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            let sentinel = 0xCAFE_BABEusize as *mut c_void;
            assert_eq!(bhi260ap_register_callback(Some(dummy_cb), sentinel), ESP_OK);
        }
    }

    #[test]
    fn test_register_callback_stores_values() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            let sentinel = 0xCAFE_BABEusize as *mut c_void;
            bhi260ap_register_callback(Some(dummy_cb), sentinel);
            let imu = &*(&raw const S_IMU);
            assert!(imu.cb.is_some());
            assert_eq!(imu.cb_data, sentinel);
        }
    }

    #[test]
    fn test_register_callback_null_clears_cb() {
        unsafe {
            reset_state();
            unsafe extern "C" fn dummy_cb(_d: *const HalImuData, _ud: *mut c_void) {}
            bhi260ap_register_callback(Some(dummy_cb), 0x1usize as *mut c_void);
            bhi260ap_register_callback(None, std::ptr::null_mut());
            let imu = &*(&raw const S_IMU);
            assert!(imu.cb.is_none());
            assert!(imu.cb_data.is_null());
        }
    }

    // ── set_sample_rate ───────────────────────────────────────────────────────

    #[test]
    fn test_set_sample_rate_returns_not_supported() {
        unsafe {
            reset_state();
            assert_eq!(bhi260ap_set_sample_rate(100), ESP_ERR_NOT_SUPPORTED);
            assert_eq!(bhi260ap_set_sample_rate(0),   ESP_ERR_NOT_SUPPORTED);
        }
    }

    // ── sleep ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sleep_returns_not_supported() {
        unsafe {
            reset_state();
            assert_eq!(bhi260ap_sleep(true),  ESP_ERR_NOT_SUPPORTED);
            assert_eq!(bhi260ap_sleep(false), ESP_ERR_NOT_SUPPORTED);
        }
    }

    // ── deinit ────────────────────────────────────────────────────────────────

    #[test]
    fn test_deinit_clears_state() {
        unsafe {
            reset_state();
            let cfg = ImuBhi260apConfig {
                i2c_bus:  0x1usize as *mut c_void,
                i2c_addr: 0x28,
                pin_int:  5,
            };
            bhi260ap_init(&cfg as *const ImuBhi260apConfig as *const c_void);
            bhi260ap_deinit();
            let imu = &*(&raw const S_IMU);
            assert!(imu.cfg.i2c_bus.is_null());
            assert!(imu.cb.is_none());
            assert!(imu.cb_data.is_null());
        }
    }
}
