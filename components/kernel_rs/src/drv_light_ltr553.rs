// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Liteon LTR-553ALS light/proximity sensor driver (Rust stub)
//
// Rust port of components/drv_light_ltr553/src/drv_light_ltr553.c.
//
// This driver does NOT use the standard HAL vtable system.  It exports a
// custom three-function public API that exactly mirrors the C API:
//
//   drv_ltr553_init(config) → stores config, returns ESP_ERR_NOT_SUPPORTED
//   drv_ltr553_deinit()     → clears driver state
//   drv_ltr553_read(data)   → zeroes *data, returns ESP_ERR_NOT_SUPPORTED
//
// All hardware operations are stubs with TODO markers for the real
// implementation.

use std::os::raw::c_void;

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// ── LTR-553ALS I2C register map (partial) ───────────────────────────────────

/// ALS measurement rate / gain control.
pub const LTR553_REG_ALS_CONTR: u8 = 0x80;
/// PS measurement rate / gain / LED current.
pub const LTR553_REG_PS_CONTR: u8 = 0x81;
/// PS LED pulse frequency, duty cycle, and peak current.
pub const LTR553_REG_PS_LED: u8 = 0x82;
/// Part / revision ID register.  Expected value: 0x92 (LTR-553ALS).
pub const LTR553_REG_PART_ID: u8 = 0x86;
/// ALS channel 1 data low byte (first register of the CH1/CH0 block).
pub const LTR553_REG_ALS_DATA_CH1_L: u8 = 0x88;
/// PS data low byte.
pub const LTR553_REG_PS_DATA_L: u8 = 0x8D;

// ── Public data struct ───────────────────────────────────────────────────────

/// Sensor reading returned by `drv_ltr553_read`.
///
/// Must match `ltr553_data_t` in `drv_light_ltr553.h`.
///
/// - `als_lux`:      ambient light level in lux (uint16_t in C header).
/// - `ps_proximity`: proximity ADC count (0–2047, 11-bit PS register).
#[repr(C)]
pub struct Ltr553Data {
    pub als_lux:      u16,
    pub ps_proximity: u16,
}

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible configuration for the LTR-553 driver.
///
/// Must match `light_ltr553_config_t` in `drv_light_ltr553.h`.
///
/// - `i2c_bus`:  opaque `i2c_master_bus_handle_t` from the board HAL.
/// - `i2c_addr`: 7-bit I2C address; default 0x23.
/// - `pin_int`:  GPIO number for the interrupt line (`gpio_num_t`).
#[repr(C)]
pub struct LightLtr553Config {
    /// Opaque ESP-IDF `i2c_master_bus_handle_t`.
    pub i2c_bus: *mut c_void,
    /// 7-bit I2C device address (default 0x23).
    pub i2c_addr: u8,
    /// Interrupt GPIO number (`gpio_num_t`).
    pub pin_int: i32,
}

// SAFETY: Config holds only primitive integers and an opaque pointer.
// Accessed only from single-threaded board-init, mirroring the C driver.
unsafe impl Send for LightLtr553Config {}
unsafe impl Sync for LightLtr553Config {}

// ── Driver state ─────────────────────────────────────────────────────────────

struct Ltr553State {
    cfg:         LightLtr553Config,
    /// Opaque `i2c_master_dev_handle_t`; null until a device is added to the bus.
    dev:         *mut c_void,
    initialized: bool,
}

// SAFETY: The driver state is only mutated from the single-threaded board-init
// and read path, mirroring the C driver's static state model.
unsafe impl Send for Ltr553State {}
unsafe impl Sync for Ltr553State {}

impl Ltr553State {
    const fn new() -> Self {
        Ltr553State {
            cfg: LightLtr553Config {
                i2c_bus:  std::ptr::null_mut(),
                i2c_addr: 0x23,
                pin_int:  -1,
            },
            dev:         std::ptr::null_mut(),
            initialized: false,
        }
    }
}

static mut S_LTR: Ltr553State = Ltr553State::new();

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialise the LTR-553ALS sensor.
///
/// Stores `*config` into the driver state and returns `ESP_ERR_NOT_SUPPORTED`.
///
/// TODO: Add I2C device to bus at config->i2c_addr, verify part ID
///       register == 0x92, write ALS_CONTR active mode, PS_CONTR active
///       mode, configure PS LED to 50 mA 100 % duty.  If pin_int is valid,
///       install interrupt ISR.
///
/// # Safety
/// `config` must point to a valid `LightLtr553Config`.
#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_init(config: *const LightLtr553Config) -> i32 {
    if !config.is_null() {
        let src = &*config;
        let ltr = &mut *(&raw mut S_LTR);
        ltr.cfg.i2c_bus  = src.i2c_bus;
        ltr.cfg.i2c_addr = src.i2c_addr;
        ltr.cfg.pin_int  = src.pin_int;
        ltr.initialized  = true;
    }
    ESP_ERR_NOT_SUPPORTED
}

/// De-initialise the LTR-553ALS sensor and clear driver state.
///
/// TODO: Remove I2C device, disable interrupts.
///
/// # Safety
/// Safe to call even if the driver was never initialised (no-op in that case).
#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_deinit() {
    // TODO: Remove I2C device, disable interrupts.
    *(&raw mut S_LTR) = Ltr553State::new();
}

/// Read ambient light (lux) and proximity from the LTR-553ALS.
///
/// Always zeroes `*data` and returns `ESP_ERR_NOT_SUPPORTED`.
///
/// TODO: Read ALS CH0/CH1 16-bit registers, apply gain/integration-time
///       formula to convert to lux.  Read PS 11-bit register for proximity.
///
/// # Safety
/// `data` must be a writable `Ltr553Data` or null (null is a no-op).
#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_read(data: *mut Ltr553Data) -> i32 {
    // TODO: Read ALS CH0/CH1 16-bit registers, apply gain/integration-time
    //       formula to convert to lux.  Read PS 11-bit register for proximity.
    if !data.is_null() {
        (*data).als_lux      = 0;
        (*data).ps_proximity = 0;
    }
    ESP_ERR_NOT_SUPPORTED
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_LTR) = Ltr553State::new();
    }

    // ── register constants ────────────────────────────────────────────────────

    #[test]
    fn test_register_constants() {
        assert_eq!(LTR553_REG_ALS_CONTR,      0x80);
        assert_eq!(LTR553_REG_PS_CONTR,       0x81);
        assert_eq!(LTR553_REG_PS_LED,         0x82);
        assert_eq!(LTR553_REG_PART_ID,        0x86);
        assert_eq!(LTR553_REG_ALS_DATA_CH1_L, 0x88);
        assert_eq!(LTR553_REG_PS_DATA_L,      0x8D);
    }

    // ── init ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_init_returns_not_supported() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus:  0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int:  -1,
            };
            assert_eq!(drv_ltr553_init(&cfg as *const LightLtr553Config), ESP_ERR_NOT_SUPPORTED);
        }
    }

    #[test]
    fn test_init_null_config_returns_not_supported() {
        unsafe {
            reset_state();
            // Null config is tolerated and still returns NOT_SUPPORTED.
            assert_eq!(drv_ltr553_init(std::ptr::null()), ESP_ERR_NOT_SUPPORTED);
        }
    }

    #[test]
    fn test_init_copies_config() {
        unsafe {
            reset_state();
            let bus_sentinel = 0xDEAD_BEEFusize as *mut c_void;
            let cfg = LightLtr553Config {
                i2c_bus:  bus_sentinel,
                i2c_addr: 0x23,
                pin_int:  10,
            };
            drv_ltr553_init(&cfg as *const LightLtr553Config);
            let ltr = &*(&raw const S_LTR);
            assert_eq!(ltr.cfg.i2c_bus,  bus_sentinel);
            assert_eq!(ltr.cfg.i2c_addr, 0x23);
            assert_eq!(ltr.cfg.pin_int,  10);
            assert!(ltr.initialized);
        }
    }

    // ── deinit ────────────────────────────────────────────────────────────────

    #[test]
    fn test_deinit_clears_state() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus:  0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int:  5,
            };
            drv_ltr553_init(&cfg as *const LightLtr553Config);
            drv_ltr553_deinit();
            let ltr = &*(&raw const S_LTR);
            assert!(ltr.cfg.i2c_bus.is_null());
            assert_eq!(ltr.cfg.i2c_addr, 0x23); // default restored
            assert_eq!(ltr.cfg.pin_int, -1);     // default restored
            assert!(!ltr.initialized);
            assert!(ltr.dev.is_null());
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            drv_ltr553_deinit(); // must not panic
            let ltr = &*(&raw const S_LTR);
            assert!(!ltr.initialized);
        }
    }

    // ── read ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_read_returns_not_supported() {
        unsafe {
            reset_state();
            let mut data = Ltr553Data { als_lux: 0xFFFF, ps_proximity: 0xFFFF };
            assert_eq!(drv_ltr553_read(&mut data as *mut Ltr553Data), ESP_ERR_NOT_SUPPORTED);
        }
    }

    #[test]
    fn test_read_zeroes_data() {
        unsafe {
            reset_state();
            let mut data = Ltr553Data { als_lux: 0xFFFF, ps_proximity: 0xFFFF };
            drv_ltr553_read(&mut data as *mut Ltr553Data);
            assert_eq!(data.als_lux,      0, "als_lux must be zeroed");
            assert_eq!(data.ps_proximity, 0, "ps_proximity must be zeroed");
        }
    }

    #[test]
    fn test_read_null_data_no_panic() {
        unsafe {
            reset_state();
            // Null pointer: function must not dereference it or panic.
            let ret = drv_ltr553_read(std::ptr::null_mut());
            assert_eq!(ret, ESP_ERR_NOT_SUPPORTED);
        }
    }

    #[test]
    fn test_read_not_supported_before_and_after_init() {
        unsafe {
            reset_state();

            // Before init
            let mut data = Ltr553Data { als_lux: 1, ps_proximity: 1 };
            assert_eq!(drv_ltr553_read(&mut data), ESP_ERR_NOT_SUPPORTED);

            // After init
            let cfg = LightLtr553Config {
                i2c_bus:  0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int:  -1,
            };
            drv_ltr553_init(&cfg as *const LightLtr553Config);
            let mut data2 = Ltr553Data { als_lux: 42, ps_proximity: 99 };
            assert_eq!(drv_ltr553_read(&mut data2), ESP_ERR_NOT_SUPPORTED);
            assert_eq!(data2.als_lux,      0);
            assert_eq!(data2.ps_proximity, 0);
        }
    }

    // ── Ltr553Data layout ─────────────────────────────────────────────────────

    #[test]
    fn test_ltr553_data_fields_accessible() {
        let d = Ltr553Data { als_lux: 100, ps_proximity: 2047 };
        assert_eq!(d.als_lux,      100);
        assert_eq!(d.ps_proximity, 2047);
    }
}
