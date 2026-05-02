// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Liteon LTR-553ALS light/proximity sensor driver (Rust)
//
// Rust port of components/drv_light_ltr553/src/drv_light_ltr553.c.
//
// The active Rust path performs real I2C init, chip-ID verification, register
// programming, lux calculation, and proximity reads. On host-test targets the
// ESP-IDF I2C calls are stubbed with deterministic data so logic can be tested
// without hardware.

use std::os::raw::c_void;

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;

const LTR553_REG_ALS_CONTR: u8 = 0x80;
const LTR553_REG_PS_CONTR: u8 = 0x81;
const LTR553_REG_PS_LED: u8 = 0x82;
const LTR553_REG_PS_MEAS_RATE: u8 = 0x84;
const LTR553_REG_ALS_MEAS_RATE: u8 = 0x85;
pub const LTR553_REG_PART_ID: u8 = 0x86;
pub const LTR553_REG_ALS_DATA_CH1_L: u8 = 0x88;
pub const LTR553_REG_PS_DATA_L: u8 = 0x8D;

const LTR553_ALS_CONTR_ACTIVE: u8 = 0x01;
const LTR553_PS_CONTR_ACTIVE: u8 = 0x03;
const LTR553_PS_LED_CONFIG: u8 = 0x7B;
const LTR553_ALS_MEAS_RATE: u8 = 0x03;
const LTR553_PS_MEAS_RATE: u8 = 0x00;
const LTR553_EXPECTED_PART_ID: u8 = 0x92;
const LTR553_I2C_TIMEOUT_MS: i32 = 50;
const I2C_ADDR_BIT_LEN_7: u32 = 0;

#[repr(C)]
pub struct Ltr553Data {
    pub als_lux: u16,
    pub ps_proximity: u16,
}

#[repr(C)]
pub struct LightLtr553Config {
    pub i2c_bus: *mut c_void,
    pub i2c_addr: u8,
    pub pin_int: i32,
}

unsafe impl Send for LightLtr553Config {}
unsafe impl Sync for LightLtr553Config {}

#[cfg(target_os = "espidf")]
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
        pub fn i2c_master_transmit(
            handle: *mut c_void,
            data: *const u8,
            len: usize,
            timeout_ms: i32,
        ) -> i32;
        pub fn i2c_master_transmit_receive(
            handle: *mut c_void,
            write_data: *const u8,
            write_size: usize,
            read_data: *mut u8,
            read_size: usize,
            timeout_ms: i32,
        ) -> i32;
    }
}

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
        pub fn i2c_master_transmit(
            handle: *mut c_void,
            data: *const u8,
            len: usize,
            timeout_ms: i32,
        ) -> i32;
        pub fn i2c_master_transmit_receive(
            handle: *mut c_void,
            write_data: *const u8,
            write_size: usize,
            read_data: *mut u8,
            read_size: usize,
            timeout_ms: i32,
        ) -> i32;
    }
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
mod esp_ffi {
    use std::os::raw::c_void;

    use super::{LTR553_EXPECTED_PART_ID, LTR553_REG_ALS_DATA_CH1_L, LTR553_REG_PART_ID, LTR553_REG_PS_DATA_L};

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
        *handle = 1usize as *mut c_void;
        0
    }

    pub unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 {
        0
    }

    pub unsafe fn i2c_master_transmit(
        _handle: *mut c_void,
        _data: *const u8,
        _len: usize,
        _timeout_ms: i32,
    ) -> i32 {
        0
    }

    pub unsafe fn i2c_master_transmit_receive(
        _handle: *mut c_void,
        write_data: *const u8,
        write_size: usize,
        read_data: *mut u8,
        read_size: usize,
        _timeout_ms: i32,
    ) -> i32 {
        if write_data.is_null() || write_size == 0 || read_data.is_null() {
            return 0x103;
        }

        let reg = *write_data;
        std::ptr::write_bytes(read_data, 0, read_size);

        match (reg, read_size) {
            (LTR553_REG_PART_ID, 1) => {
                *read_data = LTR553_EXPECTED_PART_ID;
            }
            (LTR553_REG_ALS_DATA_CH1_L, 4) => {
                // ch1 = 0x0010, ch0 = 0x0020
                let vals = [0x10u8, 0x00, 0x20, 0x00];
                std::ptr::copy_nonoverlapping(vals.as_ptr(), read_data, 4);
            }
            (LTR553_REG_PS_DATA_L, 2) => {
                // 11-bit proximity = ((0x02 & 0x07) << 8) | 0x34 = 564
                let vals = [0x34u8, 0x02];
                std::ptr::copy_nonoverlapping(vals.as_ptr(), read_data, 2);
            }
            _ => {}
        }

        0
    }
}

struct Ltr553State {
    cfg: LightLtr553Config,
    dev: *mut c_void,
    initialized: bool,
}

unsafe impl Send for Ltr553State {}
unsafe impl Sync for Ltr553State {}

impl Ltr553State {
    const fn new() -> Self {
        Self {
            cfg: LightLtr553Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x23,
                pin_int: -1,
            },
            dev: std::ptr::null_mut(),
            initialized: false,
        }
    }
}

static mut S_LTR: Ltr553State = Ltr553State::new();

fn ltr553_calculate_lux(ch0: u16, ch1: u16) -> u16 {
    if ch0.saturating_add(ch1) == 0 {
        return 0;
    }

    let ratio_1000 = (ch1 as u32 * 1000) / (ch0 as u32 + ch1 as u32);

    let lux = if ratio_1000 < 450 {
        (1774 * ch0 as u32 + 1106 * ch1 as u32) / 1000
    } else if ratio_1000 < 640 {
        (4279 * ch0 as u32).saturating_sub(1955 * ch1 as u32) / 1000
    } else if ratio_1000 < 850 {
        (593 * ch0 as u32 + 119 * ch1 as u32) / 1000
    } else {
        0
    };

    lux.min(u16::MAX as u32) as u16
}

unsafe fn ltr553_write_reg(reg: u8, val: u8) -> i32 {
    let ltr = &*(&raw const S_LTR);
    if ltr.dev.is_null() {
        return ESP_ERR_INVALID_STATE;
    }
    let buf = [reg, val];
    esp_ffi::i2c_master_transmit(ltr.dev, buf.as_ptr(), buf.len(), LTR553_I2C_TIMEOUT_MS)
}

unsafe fn ltr553_read_reg(reg: u8, val: *mut u8) -> i32 {
    let ltr = &*(&raw const S_LTR);
    if ltr.dev.is_null() || val.is_null() {
        return ESP_ERR_INVALID_STATE;
    }
    esp_ffi::i2c_master_transmit_receive(ltr.dev, &reg, 1, val, 1, LTR553_I2C_TIMEOUT_MS)
}

unsafe fn ltr553_read_regs(reg: u8, buf: *mut u8, len: usize) -> i32 {
    let ltr = &*(&raw const S_LTR);
    if ltr.dev.is_null() || buf.is_null() {
        return ESP_ERR_INVALID_STATE;
    }
    esp_ffi::i2c_master_transmit_receive(ltr.dev, &reg, 1, buf, len, LTR553_I2C_TIMEOUT_MS)
}

#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_init(config: *const LightLtr553Config) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let src = &*config;
    if src.i2c_bus.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let ltr = &mut *(&raw mut S_LTR);
    ltr.cfg.i2c_bus = src.i2c_bus;
    ltr.cfg.i2c_addr = src.i2c_addr;
    ltr.cfg.pin_int = src.pin_int;

    let dev_cfg = esp_ffi::I2cDeviceConfig {
        dev_addr_length: I2C_ADDR_BIT_LEN_7,
        device_address: src.i2c_addr as u16,
        scl_speed_hz: 400_000,
        scl_wait_us: 0,
        flags: 0,
    };

    let ret = esp_ffi::i2c_master_bus_add_device(ltr.cfg.i2c_bus, &dev_cfg, &mut ltr.dev);
    if ret != ESP_OK {
        ltr.dev = std::ptr::null_mut();
        ltr.initialized = false;
        return ret;
    }

    let mut part_id = 0u8;
    let ret = ltr553_read_reg(LTR553_REG_PART_ID, &mut part_id);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(ltr.dev);
        ltr.dev = std::ptr::null_mut();
        ltr.initialized = false;
        return ret;
    }

    if part_id != LTR553_EXPECTED_PART_ID {
        esp_ffi::i2c_master_bus_rm_device(ltr.dev);
        ltr.dev = std::ptr::null_mut();
        ltr.initialized = false;
        return ESP_ERR_NOT_FOUND;
    }

    for (reg, val) in [
        (LTR553_REG_ALS_CONTR, LTR553_ALS_CONTR_ACTIVE),
        (LTR553_REG_PS_CONTR, LTR553_PS_CONTR_ACTIVE),
        (LTR553_REG_PS_LED, LTR553_PS_LED_CONFIG),
        (LTR553_REG_ALS_MEAS_RATE, LTR553_ALS_MEAS_RATE),
        (LTR553_REG_PS_MEAS_RATE, LTR553_PS_MEAS_RATE),
    ] {
        let ret = ltr553_write_reg(reg, val);
        if ret != ESP_OK {
            esp_ffi::i2c_master_bus_rm_device(ltr.dev);
            ltr.dev = std::ptr::null_mut();
            ltr.initialized = false;
            return ret;
        }
    }

    ltr.initialized = true;
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_deinit() {
    let ltr = &mut *(&raw mut S_LTR);
    if ltr.dev.is_null() {
        *ltr = Ltr553State::new();
        return;
    }

    let _ = ltr553_write_reg(LTR553_REG_ALS_CONTR, 0x00);
    let _ = ltr553_write_reg(LTR553_REG_PS_CONTR, 0x00);
    let _ = esp_ffi::i2c_master_bus_rm_device(ltr.dev);
    *ltr = Ltr553State::new();
}

#[no_mangle]
pub unsafe extern "C" fn drv_ltr553_read(data: *mut Ltr553Data) -> i32 {
    if data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let ltr = &*(&raw const S_LTR);
    if ltr.dev.is_null() || !ltr.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let mut als_buf = [0u8; 4];
    let ret = ltr553_read_regs(LTR553_REG_ALS_DATA_CH1_L, als_buf.as_mut_ptr(), als_buf.len());
    if ret != ESP_OK {
        return ret;
    }

    let ch1 = u16::from_le_bytes([als_buf[0], als_buf[1]]);
    let ch0 = u16::from_le_bytes([als_buf[2], als_buf[3]]);

    let mut ps_buf = [0u8; 2];
    let ret = ltr553_read_regs(LTR553_REG_PS_DATA_L, ps_buf.as_mut_ptr(), ps_buf.len());
    if ret != ESP_OK {
        return ret;
    }

    (*data).als_lux = ltr553_calculate_lux(ch0, ch1);
    (*data).ps_proximity = (((ps_buf[1] & 0x07) as u16) << 8) | ps_buf[0] as u16;
    ESP_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn reset_state() {
        *(&raw mut S_LTR) = Ltr553State::new();
    }

    #[test]
    fn test_register_constants() {
        assert_eq!(LTR553_REG_PART_ID, 0x86);
        assert_eq!(LTR553_REG_ALS_DATA_CH1_L, 0x88);
        assert_eq!(LTR553_REG_PS_DATA_L, 0x8D);
    }

    #[test]
    fn test_calculate_lux_zero_channels() {
        assert_eq!(ltr553_calculate_lux(0, 0), 0);
    }

    #[test]
    fn test_calculate_lux_low_ratio_branch() {
        assert_eq!(ltr553_calculate_lux(200, 100), 465);
    }

    #[test]
    fn test_calculate_lux_mid_ratio_branch() {
        assert_eq!(ltr553_calculate_lux(100, 100), 232);
    }

    #[test]
    fn test_calculate_lux_high_ratio_branch() {
        assert_eq!(ltr553_calculate_lux(40, 120), 38);
    }

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(drv_ltr553_init(std::ptr::null()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_null_bus_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x23,
                pin_int: -1,
            };
            assert_eq!(drv_ltr553_init(&cfg), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_copies_config_and_initializes() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus: 0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int: 10,
            };
            assert_eq!(drv_ltr553_init(&cfg), ESP_OK);
            let ltr = &*(&raw const S_LTR);
            assert_eq!(ltr.cfg.i2c_bus, cfg.i2c_bus);
            assert_eq!(ltr.cfg.i2c_addr, 0x23);
            assert_eq!(ltr.cfg.pin_int, 10);
            assert!(ltr.initialized);
            assert!(!ltr.dev.is_null());
        }
    }

    #[test]
    fn test_deinit_clears_state() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus: 0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int: 5,
            };
            assert_eq!(drv_ltr553_init(&cfg), ESP_OK);
            drv_ltr553_deinit();
            let ltr = &*(&raw const S_LTR);
            assert!(ltr.cfg.i2c_bus.is_null());
            assert_eq!(ltr.cfg.i2c_addr, 0x23);
            assert_eq!(ltr.cfg.pin_int, -1);
            assert!(!ltr.initialized);
            assert!(ltr.dev.is_null());
        }
    }

    #[test]
    fn test_read_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let mut data = Ltr553Data { als_lux: 0, ps_proximity: 0 };
            assert_eq!(drv_ltr553_read(&mut data), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_read_null_data_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(drv_ltr553_read(std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_read_after_init_populates_data() {
        unsafe {
            reset_state();
            let cfg = LightLtr553Config {
                i2c_bus: 0x1usize as *mut c_void,
                i2c_addr: 0x23,
                pin_int: -1,
            };
            assert_eq!(drv_ltr553_init(&cfg), ESP_OK);

            let mut data = Ltr553Data { als_lux: 0, ps_proximity: 0 };
            assert_eq!(drv_ltr553_read(&mut data), ESP_OK);
            assert_eq!(data.als_lux, 74);
            assert_eq!(data.ps_proximity, 564);
        }
    }

    #[test]
    fn test_ltr553_data_fields_accessible() {
        let d = Ltr553Data { als_lux: 100, ps_proximity: 2047 };
        assert_eq!(d.als_lux, 100);
        assert_eq!(d.ps_proximity, 2047);
    }
}
