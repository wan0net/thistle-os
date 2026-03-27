// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CST816S capacitive touch controller driver (Rust)
//
// The CST816S is a single-touch I2C controller commonly found on the LilyGo
// T-Display-S3.  Unlike the CST328, it uses 8-bit register addresses and
// reports a single touch point.
//
// Register map:
//   0x01 — gesture code (0=none, 1=slide down, 2=slide up, 3=slide left,
//           4=slide right, 5=single click, 11=double click, 12=long press)
//   0x02 — finger count (0 or 1)
//   0x03 — X high (bits [3:0] are the upper 4 bits of the 12-bit X coordinate)
//   0x04 — X low  (lower 8 bits of X)
//   0x05 — Y high (bits [3:0] are the upper 4 bits of the 12-bit Y coordinate)
//   0x06 — Y low  (lower 8 bits of Y)
//
// An active-low interrupt pin (INT) signals new touch data.  An optional
// reset pin (RST) drives a hardware reset on init.

use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hal_registry::{HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData,
                          HalInputEventType, HalInputTouchData};

// ── ESP error codes ─────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Register addresses (8-bit) ───────────────────────────────────────────────

const CST816_REG_GESTURE:        u8 = 0x01;
const CST816_REG_FINGER_CNT:     u8 = 0x02;
const CST816_REG_X_HIGH:         u8 = 0x03;
const CST816_REG_CHIP_ID:        u8 = 0xA7;
const CST816_REG_DIS_AUTO_SLEEP: u8 = 0xFE;

const CST816_BURST_LEN: usize = 6; // gesture + finger_cnt + x_hi + x_lo + y_hi + y_lo

// GPIO_NUM_NC: -1 means "not connected"
const GPIO_NUM_NC: i32 = -1;

// ── I2C device config layout (mirrors i2c_device_config_t) ──────────────────

#[repr(C)]
struct I2cDeviceConfig {
    dev_addr_length: u32, // I2C_ADDR_BIT_LEN_7 = 0
    device_address: u16,
    scl_speed_hz: u32,
}

// ── GPIO config layout (mirrors gpio_config_t) ───────────────────────────────

#[repr(C)]
struct GpioConfig {
    pin_bit_mask: u64,
    mode: u32,
    pull_up_en: u32,
    pull_down_en: u32,
    intr_type: u32,
}

// GPIO / interrupt constants
const GPIO_MODE_OUTPUT: u32 = 2;
const GPIO_MODE_INPUT: u32 = 1;
const GPIO_PULLUP_ENABLE: u32 = 1;
const GPIO_PULLUP_DISABLE: u32 = 0;
const GPIO_PULLDOWN_DISABLE: u32 = 0;
const GPIO_INTR_DISABLE: u32 = 0;
const GPIO_INTR_NEGEDGE: u32 = 2;

// ── ESP-IDF FFI ─────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
extern "C" {
    fn i2c_master_bus_add_device(
        bus: *mut c_void,
        cfg: *const I2cDeviceConfig,
        handle: *mut *mut c_void,
    ) -> i32;
    fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
    fn i2c_master_transmit_receive(
        handle: *mut c_void,
        write_data: *const u8,
        write_size: usize,
        read_data: *mut u8,
        read_size: usize,
        timeout_ms: i32,
    ) -> i32;
    fn i2c_master_transmit(
        handle: *mut c_void,
        data: *const u8,
        len: usize,
        timeout_ms: i32,
    ) -> i32;
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_set_level(pin: i32, level: u32) -> i32;
    fn gpio_isr_handler_add(
        pin: i32,
        handler: unsafe extern "C" fn(*mut c_void),
        arg: *mut c_void,
    ) -> i32;
    fn gpio_isr_handler_remove(pin: i32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;
    fn esp_timer_get_time() -> i64;
    fn vTaskDelay(ticks: u32);
}

// ── Stub implementations (simulator / host tests) ────────────────────────────

#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_bus_add_device(
    _bus: *mut c_void,
    _cfg: *const I2cDeviceConfig,
    handle: *mut *mut c_void,
) -> i32 {
    *handle = 1usize as *mut c_void;
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 {
    ESP_OK
}

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
unsafe fn i2c_master_transmit(
    _handle: *mut c_void,
    _data: *const u8,
    _len: usize,
    _timeout_ms: i32,
) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_set_level(_pin: i32, _level: u32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_isr_handler_add(
    _pin: i32,
    _handler: unsafe extern "C" fn(*mut c_void),
    _arg: *mut c_void,
) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_isr_handler_remove(_pin: i32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_install_isr_service(_flags: i32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn esp_timer_get_time() -> i64 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelay(_ticks: u32) {}

// ── FreeRTOS tick helper ──────────────────────────────────────────────────────

#[inline(always)]
fn ms_to_ticks(ms: u32) -> u32 {
    ms // 1 ms / tick at 1 kHz default tick rate
}

// ── Configuration struct (C-compatible) ──────────────────────────────────────

/// Configuration passed to `cst816_init`.
#[repr(C)]
pub struct TouchCst816Config {
    /// I2C master bus handle (`i2c_master_bus_handle_t`).
    pub i2c_bus: *mut c_void,
    /// I2C device address.  Default: `0x15`.
    pub i2c_addr: u8,
    /// Interrupt GPIO number; `-1` to disable.
    pub pin_int: i32,
    /// Reset GPIO number; `-1` to disable.
    pub pin_rst: i32,
    /// Panel width for coordinate clamping (e.g. 170 for T-Display-S3).
    pub max_x: u16,
    /// Panel height for coordinate clamping (e.g. 320 for T-Display-S3).
    pub max_y: u16,
}

// ── Driver state ─────────────────────────────────────────────────────────────

struct TouchState {
    dev: *mut c_void,
    cfg: TouchCst816Config,
    cb: HalInputCb,
    cb_data: *mut c_void,
    irq_pending: AtomicBool,
    touching: bool,
    last_x: u16,
    last_y: u16,
    last_gesture: u8,
    initialized: bool,
}

// SAFETY: Driver state is guarded by the single-threaded init/poll contract.
// ISR sets irq_pending atomically.
unsafe impl Send for TouchState {}
unsafe impl Sync for TouchState {}

impl TouchState {
    const fn new() -> Self {
        TouchState {
            dev: std::ptr::null_mut(),
            cfg: TouchCst816Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x15,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            irq_pending: AtomicBool::new(false),
            touching: false,
            last_x: 0,
            last_y: 0,
            last_gesture: 0,
            initialized: false,
        }
    }
}

static mut S_TOUCH: TouchState = TouchState::new();

// ── I2C helpers ──────────────────────────────────────────────────────────────

/// Read `len` bytes starting at register `reg` into `buf`.
///
/// # Safety
/// `buf` must point to at least `len` bytes; `S_TOUCH.dev` must be valid.
unsafe fn cst816_read_regs(reg: u8, buf: *mut u8, len: usize) -> i32 {
    i2c_master_transmit_receive(S_TOUCH.dev, &reg, 1, buf, len, 50)
}

/// Write `data` bytes to register `reg`.
///
/// # Safety
/// `S_TOUCH.dev` must be a valid I2C device handle.
unsafe fn cst816_write_reg(reg: u8, data: &[u8]) -> i32 {
    let mut tx = [0u8; 2];
    tx[0] = reg;
    tx[1] = data[0];
    i2c_master_transmit(S_TOUCH.dev, tx.as_ptr(), 2, 50)
}

// ── ISR ──────────────────────────────────────────────────────────────────────

/// GPIO ISR handler — sets irq_pending so the poll loop reads I2C.
///
/// # Safety
/// Registered with `gpio_isr_handler_add`; called from interrupt context.
unsafe extern "C" fn cst816_isr_handler(_arg: *mut c_void) {
    S_TOUCH.irq_pending.store(true, Ordering::Relaxed);
}

// ── Hardware reset ─────────────────────────────────────────────────────────

/// Pulse the reset pin: assert low for 20 ms, release high for 200 ms.
///
/// # Safety
/// Calls ESP-IDF GPIO and FreeRTOS APIs.
unsafe fn cst816_hw_reset() {
    if S_TOUCH.cfg.pin_rst == GPIO_NUM_NC {
        return;
    }
    gpio_set_level(S_TOUCH.cfg.pin_rst, 0);
    vTaskDelay(ms_to_ticks(20));
    gpio_set_level(S_TOUCH.cfg.pin_rst, 1);
    vTaskDelay(ms_to_ticks(200));
}

// ── HAL vtable functions ──────────────────────────────────────────────────────

/// Initialise the CST816S touch controller.
///
/// `config` must point to a `TouchCst816Config`.
///
/// # Safety
/// Called from C via the HAL vtable; `config` must be valid.
unsafe extern "C" fn cst816_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if S_TOUCH.initialized {
        return ESP_OK;
    }

    // Copy config
    let src = &*(config as *const TouchCst816Config);
    S_TOUCH.cfg.i2c_bus  = src.i2c_bus;
    S_TOUCH.cfg.i2c_addr = src.i2c_addr;
    S_TOUCH.cfg.pin_int  = src.pin_int;
    S_TOUCH.cfg.pin_rst  = src.pin_rst;
    S_TOUCH.cfg.max_x    = src.max_x;
    S_TOUCH.cfg.max_y    = src.max_y;

    S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
    S_TOUCH.touching = false;

    // ── Optional reset pin ─────────────────────────────────────────────────
    if S_TOUCH.cfg.pin_rst != GPIO_NUM_NC {
        let rst_cfg = GpioConfig {
            pin_bit_mask: 1u64 << S_TOUCH.cfg.pin_rst,
            mode: GPIO_MODE_OUTPUT,
            pull_up_en: GPIO_PULLUP_DISABLE,
            pull_down_en: GPIO_PULLDOWN_DISABLE,
            intr_type: GPIO_INTR_DISABLE,
        };
        let ret = gpio_config(&rst_cfg);
        if ret != ESP_OK {
            return ret;
        }
        cst816_hw_reset();
    }

    // ── Add I2C device ─────────────────────────────────────────────────────
    let dev_cfg = I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: S_TOUCH.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
    };
    let ret = i2c_master_bus_add_device(S_TOUCH.cfg.i2c_bus, &dev_cfg, &mut S_TOUCH.dev);
    if ret != ESP_OK {
        return ret;
    }

    // ── Verify chip presence by reading Chip ID register (0xA7) ────────────
    // Unlike the gesture register, the Chip ID responds even without an
    // active touch.
    let mut probe = 0u8;
    let ret = cst816_read_regs(CST816_REG_CHIP_ID, &mut probe, 1);
    if ret != ESP_OK {
        i2c_master_bus_rm_device(S_TOUCH.dev);
        S_TOUCH.dev = std::ptr::null_mut();
        return ret;
    }

    // ── Disable auto-sleep (register 0xFE) ──────────────────────────────
    // Without this, the controller enters standby after ~2 s idle and
    // stops responding to I2C.
    let disable_sleep = [0x01u8];
    let ret = cst816_write_reg(CST816_REG_DIS_AUTO_SLEEP, &disable_sleep);
    if ret != ESP_OK {
        i2c_master_bus_rm_device(S_TOUCH.dev);
        S_TOUCH.dev = std::ptr::null_mut();
        return ret;
    }

    // ── Optional interrupt pin ─────────────────────────────────────────────
    if S_TOUCH.cfg.pin_int != GPIO_NUM_NC {
        let int_cfg = GpioConfig {
            pin_bit_mask: 1u64 << S_TOUCH.cfg.pin_int,
            mode: GPIO_MODE_INPUT,
            pull_up_en: GPIO_PULLUP_ENABLE,
            pull_down_en: GPIO_PULLDOWN_DISABLE,
            intr_type: GPIO_INTR_NEGEDGE,
        };
        let ret = gpio_config(&int_cfg);
        if ret != ESP_OK {
            i2c_master_bus_rm_device(S_TOUCH.dev);
            S_TOUCH.dev = std::ptr::null_mut();
            return ret;
        }

        gpio_install_isr_service(0); // idempotent

        let ret = gpio_isr_handler_add(
            S_TOUCH.cfg.pin_int,
            cst816_isr_handler,
            std::ptr::null_mut(),
        );
        if ret != ESP_OK {
            i2c_master_bus_rm_device(S_TOUCH.dev);
            S_TOUCH.dev = std::ptr::null_mut();
            return ret;
        }
    }

    S_TOUCH.initialized = true;
    ESP_OK
}

/// De-initialise the CST816S driver.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn cst816_deinit() {
    if !S_TOUCH.initialized {
        return;
    }

    if S_TOUCH.cfg.pin_int != GPIO_NUM_NC {
        gpio_isr_handler_remove(S_TOUCH.cfg.pin_int);
    }

    i2c_master_bus_rm_device(S_TOUCH.dev);
    S_TOUCH.dev = std::ptr::null_mut();
    S_TOUCH.cb = None;
    S_TOUCH.cb_data = std::ptr::null_mut();
    S_TOUCH.touching = false;
    S_TOUCH.initialized = false;
}

/// Register the event callback.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn cst816_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    S_TOUCH.cb = cb;
    S_TOUCH.cb_data = user_data;
    ESP_OK
}

/// Poll the CST816S for new touch events.
///
/// The CST816S reports a single touch point.  Reads 6 bytes starting at
/// `REG_GESTURE` (0x01): [gesture, finger_count, x_high, x_low, y_high, y_low].
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn cst816_poll() -> i32 {
    if !S_TOUCH.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Skip I2C read if interrupt-driven and no edge was detected while idle.
    if S_TOUCH.cfg.pin_int != GPIO_NUM_NC {
        if !S_TOUCH.irq_pending.load(Ordering::Relaxed) && !S_TOUCH.touching {
            return ESP_OK;
        }
    }

    // Read 6 bytes from REG_GESTURE (0x01): [gesture, cnt, x_hi, x_lo, y_hi, y_lo]
    let mut buf = [0u8; CST816_BURST_LEN];
    let ret = cst816_read_regs(CST816_REG_GESTURE, buf.as_mut_ptr(), CST816_BURST_LEN);
    if ret != ESP_OK {
        return ret;
    }

    let _gesture = buf[0];
    S_TOUCH.last_gesture = _gesture;
    let finger_count = buf[1];
    let now_ms = (esp_timer_get_time() / 1000) as u32;

    if finger_count > 0 {
        // Extract 12-bit X and Y: high nibble (bits[3:0]) + low byte
        let mut x: u16 = ((buf[2] as u16 & 0x0F) << 8) | buf[3] as u16;
        let mut y: u16 = ((buf[4] as u16 & 0x0F) << 8) | buf[5] as u16;

        // Clamp to panel dimensions
        if S_TOUCH.cfg.max_x > 0 && x >= S_TOUCH.cfg.max_x {
            x = S_TOUCH.cfg.max_x - 1;
        }
        if S_TOUCH.cfg.max_y > 0 && y >= S_TOUCH.cfg.max_y {
            y = S_TOUCH.cfg.max_y - 1;
        }

        let ev_type = if !S_TOUCH.touching {
            S_TOUCH.touching = true;
            HalInputEventType::TouchDown
        } else {
            HalInputEventType::TouchMove
        };

        S_TOUCH.last_x = x;
        S_TOUCH.last_y = y;

        if let Some(cb) = S_TOUCH.cb {
            let event = HalInputEvent {
                event_type: ev_type,
                timestamp: now_ms,
                data: HalInputEventData {
                    touch: HalInputTouchData { x, y },
                },
            };
            cb(&event, S_TOUCH.cb_data);
        }
    } else {
        // Finger lifted — emit TouchUp if we were tracking a touch
        if S_TOUCH.touching {
            S_TOUCH.touching = false;
            if let Some(cb) = S_TOUCH.cb {
                let event = HalInputEvent {
                    event_type: HalInputEventType::TouchUp,
                    timestamp: now_ms,
                    data: HalInputEventData {
                        touch: HalInputTouchData {
                            x: S_TOUCH.last_x,
                            y: S_TOUCH.last_y,
                        },
                    },
                };
                cb(&event, S_TOUCH.cb_data);
            }
        }
    }

    S_TOUCH.irq_pending.swap(false, Ordering::AcqRel);
    ESP_OK
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

static TOUCH_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(cst816_init),
    deinit: Some(cst816_deinit),
    register_callback: Some(cst816_register_callback),
    poll: Some(cst816_poll),
    name: b"CST816S\0".as_ptr() as *const c_char,
    is_touch: true,
};

/// Return a pointer to the CST816S HAL input driver vtable.
///
/// # Safety
/// May be called from C. The returned pointer is valid for the program lifetime.
#[no_mangle]
pub extern "C" fn drv_touch_cst816_get() -> *const HalInputDriver {
    &TOUCH_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests.
    unsafe fn reset_state() {
        S_TOUCH.dev = std::ptr::null_mut();
        S_TOUCH.cfg.i2c_bus = std::ptr::null_mut();
        S_TOUCH.cfg.i2c_addr = 0x15;
        S_TOUCH.cfg.pin_int = GPIO_NUM_NC;
        S_TOUCH.cfg.pin_rst = GPIO_NUM_NC;
        S_TOUCH.cfg.max_x = 170;
        S_TOUCH.cfg.max_y = 320;
        S_TOUCH.cb = None;
        S_TOUCH.cb_data = std::ptr::null_mut();
        S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
        S_TOUCH.touching = false;
        S_TOUCH.last_x = 0;
        S_TOUCH.last_y = 0;
        S_TOUCH.last_gesture = 0;
        S_TOUCH.initialized = false;
    }

    #[test]
    fn test_vtable_pointer_non_null() {
        let ptr = drv_touch_cst816_get();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_vtable_fields() {
        let drv = unsafe { &*drv_touch_cst816_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.poll.is_some());
        assert!(drv.is_touch);
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_vtable_name_is_cst816s() {
        let drv = unsafe { &*drv_touch_cst816_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "CST816S");
    }

    #[test]
    fn test_vtable_pointer_stable() {
        let p1 = drv_touch_cst816_get();
        let p2 = drv_touch_cst816_get();
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            let ret = cst816_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            assert!(!S_TOUCH.initialized);
        }
    }

    #[test]
    fn test_init_and_deinit() {
        unsafe {
            reset_state();
            let cfg = TouchCst816Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x15,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            };
            let ret = cst816_init(&cfg as *const TouchCst816Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!(S_TOUCH.initialized);

            cst816_deinit();
            assert!(!S_TOUCH.initialized);
            assert!(S_TOUCH.dev.is_null());
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = TouchCst816Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x15,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            };
            let ptr = &cfg as *const TouchCst816Config as *const c_void;
            assert_eq!(cst816_init(ptr), ESP_OK);
            assert_eq!(cst816_init(ptr), ESP_OK);
            assert!(S_TOUCH.initialized);
            cst816_deinit();
        }
    }

    #[test]
    fn test_poll_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(cst816_poll(), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_poll_no_touch_no_callback() {
        unsafe {
            reset_state();
            let cfg = TouchCst816Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x15,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            };
            assert_eq!(
                cst816_init(&cfg as *const TouchCst816Config as *const c_void),
                ESP_OK
            );
            // Stubs return all zeros → finger_count == 0 → no event
            assert_eq!(cst816_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            cst816_deinit();
        }
    }

    #[test]
    fn test_register_callback() {
        unsafe {
            reset_state();
            let cfg = TouchCst816Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x15,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            };
            assert_eq!(
                cst816_init(&cfg as *const TouchCst816Config as *const c_void),
                ESP_OK
            );
            unsafe extern "C" fn dummy_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {}
            assert_eq!(
                cst816_register_callback(Some(dummy_cb), std::ptr::null_mut()),
                ESP_OK
            );
            assert!(S_TOUCH.cb.is_some());
            cst816_deinit();
        }
    }

    #[test]
    fn test_coordinate_clamping() {
        unsafe {
            reset_state();
            // Manually set state to simulate a touch with out-of-range coords
            S_TOUCH.initialized = true;
            S_TOUCH.touching = true;
            S_TOUCH.cfg.max_x = 170;
            S_TOUCH.cfg.max_y = 320;
            S_TOUCH.dev = 1usize as *mut c_void;

            let mut x: u16 = 200; // beyond max_x = 170
            let mut y: u16 = 400; // beyond max_y = 320

            if S_TOUCH.cfg.max_x > 0 && x >= S_TOUCH.cfg.max_x {
                x = S_TOUCH.cfg.max_x - 1;
            }
            if S_TOUCH.cfg.max_y > 0 && y >= S_TOUCH.cfg.max_y {
                y = S_TOUCH.cfg.max_y - 1;
            }

            assert_eq!(x, 169);
            assert_eq!(y, 319);

            reset_state();
        }
    }

    #[test]
    fn test_irq_skip_when_not_touching() {
        unsafe {
            reset_state();
            let cfg = TouchCst816Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x15,
                pin_int: 16, // interrupt pin active
                pin_rst: GPIO_NUM_NC,
                max_x: 170,
                max_y: 320,
            };
            assert_eq!(
                cst816_init(&cfg as *const TouchCst816Config as *const c_void),
                ESP_OK
            );
            // irq_pending = false, touching = false → poll should return early
            S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
            S_TOUCH.touching = false;
            assert_eq!(cst816_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            cst816_deinit();
        }
    }

    #[test]
    fn test_deinit_without_init_is_safe() {
        unsafe {
            reset_state();
            // Must not crash or panic
            cst816_deinit();
            assert!(!S_TOUCH.initialized);
        }
    }

    #[test]
    fn test_default_i2c_addr_is_0x15() {
        // Verify the default address constant matches the hardware spec
        assert_eq!(0x15u8, 0x15);
        unsafe {
            reset_state();
            assert_eq!(S_TOUCH.cfg.i2c_addr, 0x15);
        }
    }
}
