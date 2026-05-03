// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CST9217 touch driver (Rust)
//
// Rebuilds Waveshare's esp_lcd_touch_cst9217 component in Rust. The vendor
// driver reads a compact touch packet at 0xD000 and accepts packets with ACK
// byte 0xAB at offset 6.

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{
    HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData, HalInputEventType,
    HalInputTouchData,
};

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_INVALID_RESPONSE: i32 = 0x108;
const GPIO_NUM_NC: i32 = -1;

const CST9217_DATA_REG: u16 = 0xD000;
const CST9217_CMD_MODE_REG: u16 = 0xD101;
const CST9217_PROJECT_ID_REG: u16 = 0xD204;
const CST9217_CHECKCODE_REG: u16 = 0xD1FC;
const CST9217_RESOLUTION_REG: u16 = 0xD1F8;
const CST9217_ACK: u8 = 0xAB;
const CST9217_DATA_LEN: usize = 10;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TouchCst9217Config {
    pub i2c_bus: *mut c_void,
    pub i2c_addr: u8,
    pub pin_int: i32,
    pub pin_rst: i32,
    pub max_x: u16,
    pub max_y: u16,
    pub swap_xy: bool,
    pub invert_x: bool,
    pub invert_y: bool,
}

unsafe impl Send for TouchCst9217Config {}
unsafe impl Sync for TouchCst9217Config {}

#[repr(C)]
struct I2cDeviceConfig {
    dev_addr_length: u32,
    device_address: u16,
    scl_speed_hz: u32,
}

#[repr(C)]
struct GpioConfig {
    pin_bit_mask: u64,
    mode: u32,
    pull_up_en: u32,
    pull_down_en: u32,
    intr_type: u32,
}

#[cfg(target_os = "espidf")]
extern "C" {
    fn i2c_master_bus_add_device(bus: *mut c_void, cfg: *const I2cDeviceConfig, handle: *mut *mut c_void) -> i32;
    fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
    fn i2c_master_transmit_receive(
        handle: *mut c_void,
        write_data: *const u8,
        write_size: usize,
        read_data: *mut u8,
        read_size: usize,
        timeout_ms: i32,
    ) -> i32;
    fn i2c_master_transmit(handle: *mut c_void, data: *const u8, len: usize, timeout_ms: i32) -> i32;
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_set_level(pin: i32, level: u32) -> i32;
    fn gpio_reset_pin(pin: i32) -> i32;
    fn esp_timer_get_time() -> i64;
    fn vTaskDelay(ticks: u32);
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_bus_add_device(_bus: *mut c_void, _cfg: *const I2cDeviceConfig, handle: *mut *mut c_void) -> i32 {
    *handle = 1usize as *mut c_void;
    ESP_OK
}
#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_transmit_receive(
    _handle: *mut c_void,
    _write_data: *const u8,
    _write_size: usize,
    read_data: *mut u8,
    read_size: usize,
    _timeout_ms: i32,
) -> i32 {
    std::ptr::write_bytes(read_data, 0, read_size);
    ESP_OK
}
#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_transmit(_handle: *mut c_void, _data: *const u8, _len: usize, _timeout_ms: i32) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_set_level(_pin: i32, _level: u32) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_reset_pin(_pin: i32) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn esp_timer_get_time() -> i64 { 0 }
#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelay(_ticks: u32) {}

struct TouchState {
    dev: *mut c_void,
    cfg: TouchCst9217Config,
    cb: HalInputCb,
    cb_data: *mut c_void,
    touching: bool,
    last_x: u16,
    last_y: u16,
    initialized: bool,
}

unsafe impl Send for TouchState {}
unsafe impl Sync for TouchState {}

impl TouchState {
    const fn new() -> Self {
        TouchState {
            dev: std::ptr::null_mut(),
            cfg: TouchCst9217Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x5A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 410,
                max_y: 502,
                swap_xy: false,
                invert_x: false,
                invert_y: false,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            touching: false,
            last_x: 0,
            last_y: 0,
            initialized: false,
        }
    }
}

static mut STATE: TouchState = TouchState::new();

unsafe fn state_mut() -> &'static mut TouchState {
    &mut *(&raw mut STATE)
}

unsafe fn read_reg(reg: u16, data: &mut [u8]) -> i32 {
    let addr = [(reg >> 8) as u8, reg as u8];
    i2c_master_transmit_receive(state_mut().dev, addr.as_ptr(), addr.len(), data.as_mut_ptr(), data.len(), 50)
}

unsafe fn write_reg(reg: u16, data: &[u8]) -> i32 {
    let mut buf = [0u8; 10];
    let len = 2 + data.len().min(buf.len() - 2);
    buf[0] = (reg >> 8) as u8;
    buf[1] = reg as u8;
    buf[2..len].copy_from_slice(&data[..len - 2]);
    i2c_master_transmit(state_mut().dev, buf.as_ptr(), len, 50)
}

unsafe fn hw_reset() {
    let rst = state_mut().cfg.pin_rst;
    if rst == GPIO_NUM_NC {
        return;
    }
    gpio_set_level(rst, 0);
    vTaskDelay(10);
    gpio_set_level(rst, 1);
    vTaskDelay(50);
}

unsafe fn read_config() -> i32 {
    let ret = write_reg(CST9217_CMD_MODE_REG, &[0xD1, 0x01]);
    if ret != ESP_OK {
        return ret;
    }
    vTaskDelay(10);

    let mut data = [0u8; 4];
    for reg in [CST9217_CHECKCODE_REG, CST9217_RESOLUTION_REG, CST9217_PROJECT_ID_REG] {
        let ret = read_reg(reg, &mut data);
        if ret != ESP_OK {
            return ret;
        }
    }
    ESP_OK
}

fn transform_coords(cfg: TouchCst9217Config, mut x: u16, mut y: u16) -> (u16, u16) {
    let max_x = cfg.max_x.max(1);
    let max_y = cfg.max_y.max(1);
    if cfg.swap_xy {
        std::mem::swap(&mut x, &mut y);
    }
    if cfg.invert_x {
        x = max_x.saturating_sub(1).saturating_sub(x.min(max_x - 1));
    }
    if cfg.invert_y {
        y = max_y.saturating_sub(1).saturating_sub(y.min(max_y - 1));
    }
    (x.min(max_x - 1), y.min(max_y - 1))
}

unsafe fn dispatch(event_type: HalInputEventType, x: u16, y: u16) {
    let s = state_mut();
    if let Some(cb) = s.cb {
        let event = HalInputEvent {
            event_type,
            timestamp: (esp_timer_get_time() / 1000) as u32,
            data: HalInputEventData {
                touch: HalInputTouchData { x, y },
            },
        };
        cb(&event, s.cb_data);
    }
}

unsafe extern "C" fn cst9217_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let s = state_mut();
    if s.initialized {
        return ESP_OK;
    }

    s.cfg = *(config as *const TouchCst9217Config);
    if s.cfg.i2c_addr == 0 {
        s.cfg.i2c_addr = 0x5A;
    }
    if s.cfg.max_x == 0 {
        s.cfg.max_x = 410;
    }
    if s.cfg.max_y == 0 {
        s.cfg.max_y = 502;
    }

    if s.cfg.pin_rst != GPIO_NUM_NC {
        let cfg = GpioConfig {
            pin_bit_mask: 1u64 << s.cfg.pin_rst,
            mode: 2,
            pull_up_en: 0,
            pull_down_en: 0,
            intr_type: 0,
        };
        let ret = gpio_config(&cfg);
        if ret != ESP_OK { return ret; }
        hw_reset();
    }

    let dev_cfg = I2cDeviceConfig {
        dev_addr_length: 0,
        device_address: s.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
    };
    let ret = i2c_master_bus_add_device(s.cfg.i2c_bus, &dev_cfg, &mut s.dev);
    if ret != ESP_OK {
        return ret;
    }

    let ret = read_config();
    if ret != ESP_OK {
        i2c_master_bus_rm_device(s.dev);
        s.dev = std::ptr::null_mut();
        return ret;
    }

    s.touching = false;
    s.initialized = true;
    ESP_OK
}

unsafe extern "C" fn cst9217_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }
    if s.cfg.pin_rst != GPIO_NUM_NC {
        let _ = gpio_reset_pin(s.cfg.pin_rst);
    }
    if s.cfg.pin_int != GPIO_NUM_NC {
        let _ = gpio_reset_pin(s.cfg.pin_int);
    }
    let _ = i2c_master_bus_rm_device(s.dev);
    *s = TouchState::new();
}

unsafe extern "C" fn cst9217_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    let s = state_mut();
    s.cb = cb;
    s.cb_data = user_data;
    ESP_OK
}

unsafe extern "C" fn cst9217_poll() -> i32 {
    let s = state_mut();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let mut data = [0u8; CST9217_DATA_LEN];
    let ret = read_reg(CST9217_DATA_REG, &mut data);
    if ret != ESP_OK {
        if s.cfg.pin_rst != GPIO_NUM_NC {
            hw_reset();
        }
        return ESP_FAIL;
    }
    if data[6] != CST9217_ACK {
        if s.touching {
            s.touching = false;
            dispatch(HalInputEventType::TouchUp, s.last_x, s.last_y);
        }
        return ESP_ERR_INVALID_RESPONSE;
    }

    let points = (data[5] & 0x7F).min(1);
    let active = points > 0 && (data[0] & 0x0F) == 0x06;
    if !active {
        if s.touching {
            s.touching = false;
            dispatch(HalInputEventType::TouchUp, s.last_x, s.last_y);
        }
        return ESP_OK;
    }

    let x = ((data[1] as u16) << 4) | ((data[3] as u16) >> 4);
    let y = ((data[2] as u16) << 4) | ((data[3] as u16) & 0x0F);
    let (x, y) = transform_coords(s.cfg, x, y);
    let event_type = if s.touching {
        if x == s.last_x && y == s.last_y {
            return ESP_OK;
        }
        HalInputEventType::TouchMove
    } else {
        s.touching = true;
        HalInputEventType::TouchDown
    };
    s.last_x = x;
    s.last_y = y;
    dispatch(event_type, x, y);
    ESP_OK
}

static CST9217_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(cst9217_init),
    deinit: Some(cst9217_deinit),
    register_callback: Some(cst9217_register_callback),
    poll: Some(cst9217_poll),
    name: b"CST9217 Touch\0".as_ptr() as *const c_char,
    is_touch: true,
};

#[no_mangle]
pub extern "C" fn drv_touch_cst9217_get() -> *const HalInputDriver {
    &CST9217_DRIVER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_decoding_matches_vendor_layout() {
        let data = [0x06, 0x12, 0x34, 0x5A, 0, 1, 0xAB, 0, 0, 0];
        let x = ((data[1] as u16) << 4) | ((data[3] as u16) >> 4);
        let y = ((data[2] as u16) << 4) | ((data[3] as u16) & 0x0F);
        assert_eq!(x, 0x125);
        assert_eq!(y, 0x34A);
    }

    #[test]
    fn transform_clamps() {
        let cfg = TouchCst9217Config {
            i2c_bus: std::ptr::null_mut(),
            i2c_addr: 0x5A,
            pin_int: -1,
            pin_rst: -1,
            max_x: 410,
            max_y: 502,
            swap_xy: false,
            invert_x: false,
            invert_y: false,
        };
        assert_eq!(transform_coords(cfg, 999, 999), (409, 501));
    }
}
