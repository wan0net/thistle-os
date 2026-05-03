// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — FocalTech FT3x68/FT3168 touch driver (Rust)
//
// Rebuilds the FT3168 path used by Waveshare's Arduino_DriveBus examples.
// FT3168 follows the common FT3x68 touch data layout: TD_STATUS at 0x02 and
// point data beginning at 0x03.

use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hal_registry::{
    HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData, HalInputEventType,
    HalInputTouchData,
};

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const GPIO_NUM_NC: i32 = -1;

const FT_REG_TD_STATUS: u8 = 0x02;
const FT_REG_P1_XH: u8 = 0x03;
const FT_REG_CHIP_ID: u8 = 0xA3;
const FT_REG_POWER_MODE: u8 = 0xA5;
const FT_POWER_ACTIVE: u8 = 0x00;
const FT_POWER_SLEEP: u8 = 0x03;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TouchFt3x68Config {
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

unsafe impl Send for TouchFt3x68Config {}
unsafe impl Sync for TouchFt3x68Config {}

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
    fn gpio_isr_handler_add(pin: i32, handler: unsafe extern "C" fn(*mut c_void), arg: *mut c_void) -> i32;
    fn gpio_isr_handler_remove(pin: i32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;
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
unsafe fn gpio_isr_handler_add(_pin: i32, _handler: unsafe extern "C" fn(*mut c_void), _arg: *mut c_void) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_isr_handler_remove(_pin: i32) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_install_isr_service(_flags: i32) -> i32 { ESP_OK }
#[cfg(not(target_os = "espidf"))]
unsafe fn esp_timer_get_time() -> i64 { 0 }
#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelay(_ticks: u32) {}

struct TouchState {
    dev: *mut c_void,
    cfg: TouchFt3x68Config,
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
            cfg: TouchFt3x68Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x38,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 0,
                max_y: 0,
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
static IRQ_PENDING: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn ft_irq_handler(_arg: *mut c_void) {
    IRQ_PENDING.store(true, Ordering::Release);
}

unsafe fn state_mut() -> &'static mut TouchState {
    &mut *(&raw mut STATE)
}

unsafe fn read_regs(reg: u8, buf: &mut [u8]) -> i32 {
    i2c_master_transmit_receive(state_mut().dev, &reg, 1, buf.as_mut_ptr(), buf.len(), 50)
}

unsafe fn write_reg(reg: u8, val: u8) -> i32 {
    let data = [reg, val];
    i2c_master_transmit(state_mut().dev, data.as_ptr(), data.len(), 50)
}

unsafe fn hw_reset() {
    let rst = state_mut().cfg.pin_rst;
    if rst == GPIO_NUM_NC {
        return;
    }
    gpio_set_level(rst, 0);
    vTaskDelay(20);
    gpio_set_level(rst, 1);
    vTaskDelay(120);
}

fn transform_coords(cfg: TouchFt3x68Config, mut x: u16, mut y: u16) -> (u16, u16) {
    let max_x = if cfg.max_x == 0 { 410 } else { cfg.max_x };
    let max_y = if cfg.max_y == 0 { 502 } else { cfg.max_y };
    if cfg.swap_xy {
        std::mem::swap(&mut x, &mut y);
    }
    if cfg.invert_x {
        x = max_x.saturating_sub(1).saturating_sub(x.min(max_x.saturating_sub(1)));
    }
    if cfg.invert_y {
        y = max_y.saturating_sub(1).saturating_sub(y.min(max_y.saturating_sub(1)));
    }
    (x.min(max_x.saturating_sub(1)), y.min(max_y.saturating_sub(1)))
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

unsafe extern "C" fn ft_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let s = state_mut();
    if s.initialized {
        return ESP_OK;
    }
    s.cfg = *(config as *const TouchFt3x68Config);
    if s.cfg.i2c_addr == 0 {
        s.cfg.i2c_addr = 0x38;
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

    let mut chip_id = [0u8; 1];
    let _ = read_regs(FT_REG_CHIP_ID, &mut chip_id);
    let ret = write_reg(FT_REG_POWER_MODE, FT_POWER_ACTIVE);
    if ret != ESP_OK {
        i2c_master_bus_rm_device(s.dev);
        s.dev = std::ptr::null_mut();
        return ret;
    }

    if s.cfg.pin_int != GPIO_NUM_NC {
        let cfg = GpioConfig {
            pin_bit_mask: 1u64 << s.cfg.pin_int,
            mode: 1,
            pull_up_en: 1,
            pull_down_en: 0,
            intr_type: 2,
        };
        let ret = gpio_config(&cfg);
        if ret != ESP_OK { return ret; }
        let _ = gpio_install_isr_service(0);
        let ret = gpio_isr_handler_add(s.cfg.pin_int, ft_irq_handler, std::ptr::null_mut());
        if ret != ESP_OK { return ret; }
    }

    IRQ_PENDING.store(false, Ordering::Release);
    s.touching = false;
    s.initialized = true;
    ESP_OK
}

unsafe extern "C" fn ft_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }
    if s.cfg.pin_int != GPIO_NUM_NC {
        let _ = gpio_isr_handler_remove(s.cfg.pin_int);
    }
    let _ = i2c_master_bus_rm_device(s.dev);
    *s = TouchState::new();
}

unsafe extern "C" fn ft_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    let s = state_mut();
    s.cb = cb;
    s.cb_data = user_data;
    ESP_OK
}

unsafe extern "C" fn ft_poll() -> i32 {
    let s = state_mut();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if s.cfg.pin_int != GPIO_NUM_NC && !IRQ_PENDING.swap(false, Ordering::AcqRel) && s.touching {
        return ESP_OK;
    }

    let mut status = [0u8; 1];
    let ret = read_regs(FT_REG_TD_STATUS, &mut status);
    if ret != ESP_OK {
        return ret;
    }
    let touches = status[0] & 0x0F;
    if touches == 0 {
        if s.touching {
            s.touching = false;
            dispatch(HalInputEventType::TouchUp, s.last_x, s.last_y);
        }
        return ESP_OK;
    }

    let mut raw = [0u8; 4];
    let ret = read_regs(FT_REG_P1_XH, &mut raw);
    if ret != ESP_OK {
        return ret;
    }
    let x = (((raw[0] & 0x0F) as u16) << 8) | raw[1] as u16;
    let y = (((raw[2] & 0x0F) as u16) << 8) | raw[3] as u16;
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

unsafe extern "C" fn ft_sleep(enter: bool) -> i32 {
    if !state_mut().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    write_reg(FT_REG_POWER_MODE, if enter { FT_POWER_SLEEP } else { FT_POWER_ACTIVE })
}

static FT3X68_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(ft_init),
    deinit: Some(ft_deinit),
    register_callback: Some(ft_register_callback),
    poll: Some(ft_poll),
    name: b"FT3x68/FT3168 Touch\0".as_ptr() as *const c_char,
    is_touch: true,
};

#[no_mangle]
pub extern "C" fn drv_touch_ft3x68_get() -> *const HalInputDriver {
    &FT3X68_DRIVER
}

#[no_mangle]
pub extern "C" fn drv_touch_ft3168_get() -> *const HalInputDriver {
    &FT3X68_DRIVER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_invert_axes() {
        let cfg = TouchFt3x68Config {
            i2c_bus: std::ptr::null_mut(),
            i2c_addr: 0x38,
            pin_int: -1,
            pin_rst: -1,
            max_x: 410,
            max_y: 502,
            swap_xy: false,
            invert_x: true,
            invert_y: true,
        };
        assert_eq!(transform_coords(cfg, 0, 0), (409, 501));
    }
}
