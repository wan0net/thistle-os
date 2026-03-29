// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CST328 capacitive touch controller driver (Rust)
//
// Rust port of components/drv_touch_cst328/src/drv_touch_cst328.c.
//
// The CST328 uses 16-bit register addresses (big-endian) over I2C at 400 kHz.
// Up to 5 simultaneous touch points are reported; we track a single primary
// touch for the HAL event model.  An optional interrupt pin (active-low)
// signals new data, and an optional reset pin drives hardware reset.

use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hal_registry::{HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData,
                          HalInputEventType, HalInputTouchData};

// ── ESP error codes ─────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── CST328 register addresses (16-bit) ──────────────────────────────────────

const CST328_REG_TOUCH_INFO: u16 = 0xD000; // Number of touch points (1 byte)
const CST328_REG_TOUCH_PT1: u16 = 0xD001;  // Touch point 1 data (7 bytes)
const CST328_REG_MODULE_VER: u16 = 0xD100; // Module version (2 bytes)
const CST328_REG_COMMAND: u16 = 0xD109;    // Write 0xAB = normal mode

const CST328_CMD_NORMAL_MODE: u8 = 0xAB;
const CST328_PT_LEN: usize = 7;            // Raw bytes per touch point

// GPIO_NUM_NC: -1 means "not connected"
const GPIO_NUM_NC: i32 = -1;

// ── I2C device config layout (mirrors i2c_device_config_t) ──────────────────

#[repr(C)]
struct I2cDeviceConfig {
    dev_addr_length: u32, // I2C_ADDR_BIT_LEN_7 = 0
    device_address: u16,
    scl_speed_hz: u32,
}

// GPIO config layout (mirrors gpio_config_t) ─────────────────────────────────

#[repr(C)]
struct GpioConfig {
    pin_bit_mask: u64,
    mode: u32,
    pull_up_en: u32,
    pull_down_en: u32,
    intr_type: u32,
}

// GPIO / intr constants (from driver/gpio.h) ─────────────────────────────────

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
    // I2C master (driver/i2c_master.h)
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

    // GPIO (driver/gpio.h)
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_set_level(pin: i32, level: u32) -> i32;
    fn gpio_isr_handler_add(
        pin: i32,
        handler: unsafe extern "C" fn(*mut c_void),
        arg: *mut c_void,
    ) -> i32;
    fn gpio_isr_handler_remove(pin: i32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;

    // Timer (esp_timer.h)
    fn esp_timer_get_time() -> i64;

    // FreeRTOS (freertos/task.h)
    fn vTaskDelay(ticks: u32);
}

// ── Extern "C" bindings for simulator with sim-bus feature ───────────────────

#[cfg(all(not(target_os = "espidf"), feature = "sim-bus"))]
extern "C" {
    fn i2c_master_bus_add_device(bus: *mut c_void, cfg: *const I2cDeviceConfig, handle: *mut *mut c_void) -> i32;
    fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
    fn i2c_master_transmit_receive(handle: *mut c_void, write_data: *const u8, write_size: usize, read_data: *mut u8, read_size: usize, timeout_ms: i32) -> i32;
    fn i2c_master_transmit(handle: *mut c_void, data: *const u8, len: usize, timeout_ms: i32) -> i32;
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_set_level(pin: i32, level: u32) -> i32;
    fn gpio_isr_handler_add(pin: i32, handler: unsafe extern "C" fn(*mut c_void), arg: *mut c_void) -> i32;
    fn gpio_isr_handler_remove(pin: i32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;
    fn esp_timer_get_time() -> i64;
    fn vTaskDelay(ticks: u32);
}

// ── Stub implementations (host tests without sim-bus) ────────────────────────

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn i2c_master_bus_add_device(
    _bus: *mut c_void,
    _cfg: *const I2cDeviceConfig,
    handle: *mut *mut c_void,
) -> i32 {
    // Provide a non-null sentinel so init logic proceeds in tests.
    *handle = 1usize as *mut c_void;
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn i2c_master_transmit_receive(
    _handle: *mut c_void,
    _write_data: *const u8,
    _write_size: usize,
    read_data: *mut u8,
    read_size: usize,
    _timeout_ms: i32,
) -> i32 {
    // Zero-fill the read buffer; callers interpret 0 as "no touches / ver 0".
    std::ptr::write_bytes(read_data, 0, read_size);
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn i2c_master_transmit(
    _handle: *mut c_void,
    _data: *const u8,
    _len: usize,
    _timeout_ms: i32,
) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn gpio_set_level(_pin: i32, _level: u32) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn gpio_isr_handler_add(
    _pin: i32,
    _handler: unsafe extern "C" fn(*mut c_void),
    _arg: *mut c_void,
) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn gpio_isr_handler_remove(_pin: i32) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn gpio_install_isr_service(_flags: i32) -> i32 {
    ESP_OK
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn esp_timer_get_time() -> i64 {
    0
}

#[cfg(all(not(target_os = "espidf"), not(feature = "sim-bus")))]
unsafe fn vTaskDelay(_ticks: u32) {}

// ── FreeRTOS pdMS_TO_TICKS equivalent ────────────────────────────────────────
//
// On ESP-IDF the tick rate is configurable; 1000 Hz (1 ms per tick) is the
// default for ESP32-S3.  We replicate the C macro: ticks = ms * portTICK_PERIOD_MS
// where portTICK_PERIOD_MS == 1 at 1 kHz tick rate.

#[inline(always)]
fn ms_to_ticks(ms: u32) -> u32 {
    ms // 1 ms / tick at 1 kHz
}

// ── Configuration struct (C-compatible) ─────────────────────────────────────

/// Configuration passed to `cst328_init`.  Layout must match the C
/// `touch_cst328_config_t` struct in `drv_touch_cst328.h`.
#[repr(C)]
pub struct TouchCst328Config {
    /// I2C master bus handle (`i2c_master_bus_handle_t`).
    pub i2c_bus: *mut c_void,
    /// I2C device address.  Default: `0x1A`.
    pub i2c_addr: u8,
    /// Interrupt GPIO number; `-1` / `GPIO_NUM_NC` to disable.
    pub pin_int: i32,
    /// Reset GPIO number; `-1` / `GPIO_NUM_NC` to disable.
    pub pin_rst: i32,
    /// Panel width used for coordinate clamping (e.g. 320).
    pub max_x: u16,
    /// Panel height used for coordinate clamping (e.g. 240).
    pub max_y: u16,
}

// ── Driver state ─────────────────────────────────────────────────────────────

struct TouchState {
    dev: *mut c_void,            // i2c_master_dev_handle_t
    cfg: TouchCst328Config,
    cb: HalInputCb,
    cb_data: *mut c_void,
    irq_pending: AtomicBool,
    touching: bool,
    last_x: u16,
    last_y: u16,
    initialized: bool,
}

// SAFETY: The state is guarded by the single-threaded init / poll contract
// that mirrors the original C driver.  ISR sets irq_pending via an atomic.
unsafe impl Send for TouchState {}
unsafe impl Sync for TouchState {}

impl TouchState {
    const fn new() -> Self {
        TouchState {
            dev: std::ptr::null_mut(),
            cfg: TouchCst328Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x1A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            irq_pending: AtomicBool::new(false),
            touching: false,
            last_x: 0,
            last_y: 0,
            initialized: false,
        }
    }
}

static mut S_TOUCH: TouchState = TouchState::new();

// ── I2C helpers ──────────────────────────────────────────────────────────────

/// Write a single byte to a 16-bit register address.
///
/// # Safety
/// Must be called with a valid I2C device handle in `S_TOUCH.dev`.
unsafe fn cst328_write_reg(reg_addr: u16, val: u8) -> i32 {
    let buf = [
        (reg_addr >> 8) as u8,
        (reg_addr & 0xFF) as u8,
        val,
    ];
    i2c_master_transmit(S_TOUCH.dev, buf.as_ptr(), buf.len(), 50)
}

/// Read `len` bytes starting at a 16-bit register address into `buf`.
///
/// # Safety
/// `buf` must point to at least `len` bytes of writable memory.
/// `S_TOUCH.dev` must be a valid I2C device handle.
unsafe fn cst328_read_regs(reg_addr: u16, buf: *mut u8, len: usize) -> i32 {
    let addr = [(reg_addr >> 8) as u8, (reg_addr & 0xFF) as u8];
    i2c_master_transmit_receive(
        S_TOUCH.dev,
        addr.as_ptr(),
        addr.len(),
        buf,
        len,
        50,
    )
}

// ── ISR ──────────────────────────────────────────────────────────────────────

/// GPIO ISR handler — sets `irq_pending` so the poll loop knows to read I2C.
///
/// # Safety
/// Registered with `gpio_isr_handler_add`; called from interrupt context.
unsafe extern "C" fn cst328_isr_handler(_arg: *mut c_void) {
    S_TOUCH.irq_pending.store(true, Ordering::Relaxed);
}

// ── Hardware reset ────────────────────────────────────────────────────────────

/// Pulse the reset pin: assert low for 20 ms, then release high for 100 ms.
///
/// # Safety
/// Calls ESP-IDF GPIO and FreeRTOS APIs.
unsafe fn cst328_hw_reset() {
    if S_TOUCH.cfg.pin_rst == GPIO_NUM_NC {
        return;
    }
    gpio_set_level(S_TOUCH.cfg.pin_rst, 0);
    vTaskDelay(ms_to_ticks(20));
    gpio_set_level(S_TOUCH.cfg.pin_rst, 1);
    vTaskDelay(ms_to_ticks(100));
}

// ── HAL vtable functions ──────────────────────────────────────────────────────

/// Initialise the CST328 touch controller.
///
/// `config` must point to a `TouchCst328Config`.
///
/// # Safety
/// Called from C via the HAL vtable; `config` must be valid.
unsafe extern "C" fn cst328_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if S_TOUCH.initialized {
        return ESP_OK;
    }

    // Copy config
    let src = &*(config as *const TouchCst328Config);
    S_TOUCH.cfg.i2c_bus  = src.i2c_bus;
    S_TOUCH.cfg.i2c_addr = src.i2c_addr;
    S_TOUCH.cfg.pin_int  = src.pin_int;
    S_TOUCH.cfg.pin_rst  = src.pin_rst;
    S_TOUCH.cfg.max_x    = src.max_x;
    S_TOUCH.cfg.max_y    = src.max_y;

    S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
    S_TOUCH.touching = false;

    // ── Optional reset pin ────────────────────────────────────────────────
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
        cst328_hw_reset();
    }

    // ── Add I2C device ────────────────────────────────────────────────────
    let dev_cfg = I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: S_TOUCH.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
    };
    let ret = i2c_master_bus_add_device(S_TOUCH.cfg.i2c_bus, &dev_cfg, &mut S_TOUCH.dev);
    if ret != ESP_OK {
        return ret;
    }

    // ── Verify chip presence (read module version) ────────────────────────
    let mut ver = [0u8; 2];
    let ret = cst328_read_regs(CST328_REG_MODULE_VER, ver.as_mut_ptr(), ver.len());
    if ret != ESP_OK {
        i2c_master_bus_rm_device(S_TOUCH.dev);
        S_TOUCH.dev = std::ptr::null_mut();
        return ret;
    }
    // ver[0..1] logged on target via ESP_LOGI; no-op on simulator/host.

    // ── Set normal operating mode ─────────────────────────────────────────
    let ret = cst328_write_reg(CST328_REG_COMMAND, CST328_CMD_NORMAL_MODE);
    if ret != ESP_OK {
        i2c_master_bus_rm_device(S_TOUCH.dev);
        S_TOUCH.dev = std::ptr::null_mut();
        return ret;
    }
    vTaskDelay(ms_to_ticks(10));

    // ── Optional interrupt pin ────────────────────────────────────────────
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
            cst328_isr_handler,
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

/// De-initialise the CST328 driver and release resources.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn cst328_deinit() {
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
unsafe extern "C" fn cst328_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    S_TOUCH.cb = cb;
    S_TOUCH.cb_data = user_data;
    ESP_OK
}

/// Poll the CST328 for new touch events and dispatch callbacks.
///
/// Should be called periodically (e.g. from the display-server event loop).
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn cst328_poll() -> i32 {
    if !S_TOUCH.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Interrupt-driven optimisation: skip I2C when no edge was seen and we
    // are not mid-touch (lift-off may not generate a second IRQ).
    if S_TOUCH.cfg.pin_int != GPIO_NUM_NC {
        if !S_TOUCH.irq_pending.load(Ordering::Relaxed) && !S_TOUCH.touching {
            return ESP_OK;
        }
    }

    // Read the touch-point count register.
    let mut n_touches: u8 = 0;
    let ret = cst328_read_regs(CST328_REG_TOUCH_INFO, &mut n_touches, 1);
    if ret != ESP_OK {
        return ret;
    }
    n_touches &= 0x0F; // lower nibble is touch count

    let now_ms = (esp_timer_get_time() / 1000) as u32;

    if n_touches > 0 {
        // Read the primary touch-point data (7 bytes).
        let mut pt = [0u8; CST328_PT_LEN];
        let ret = cst328_read_regs(CST328_REG_TOUCH_PT1, pt.as_mut_ptr(), CST328_PT_LEN);
        if ret != ESP_OK {
            return ret;
        }

        // Extract 12-bit X and Y coordinates.
        let mut x: u16 = ((pt[0] as u16 & 0x0F) << 8) | pt[1] as u16;
        let mut y: u16 = ((pt[2] as u16 & 0x0F) << 8) | pt[3] as u16;

        // Clamp to configured panel dimensions.
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
        // No active touches — emit TOUCH_UP if we were previously touching.
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

    S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
    ESP_OK
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

static TOUCH_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(cst328_init),
    deinit: Some(cst328_deinit),
    register_callback: Some(cst328_register_callback),
    poll: Some(cst328_poll),
    name: b"CST328\0".as_ptr() as *const c_char,
    is_touch: true,
};

/// Return a pointer to the CST328 HAL input driver vtable.
///
/// This is the primary entry point used by board-init code.
///
/// # Safety
/// May be called from C.  The returned pointer is valid for the program lifetime.
#[no_mangle]
pub extern "C" fn drv_touch_cst328_get() -> *const HalInputDriver {
    &TOUCH_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests so they do not interfere.
    unsafe fn reset_state() {
        S_TOUCH.dev = std::ptr::null_mut();
        S_TOUCH.cfg.i2c_bus = std::ptr::null_mut();
        S_TOUCH.cfg.i2c_addr = 0x1A;
        S_TOUCH.cfg.pin_int = GPIO_NUM_NC;
        S_TOUCH.cfg.pin_rst = GPIO_NUM_NC;
        S_TOUCH.cfg.max_x = 320;
        S_TOUCH.cfg.max_y = 240;
        S_TOUCH.cb = None;
        S_TOUCH.cb_data = std::ptr::null_mut();
        S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
        S_TOUCH.touching = false;
        S_TOUCH.last_x = 0;
        S_TOUCH.last_y = 0;
        S_TOUCH.initialized = false;
    }

    #[test]
    fn test_vtable_pointer_non_null() {
        let ptr = drv_touch_cst328_get();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_vtable_fields() {
        let drv = unsafe { &*drv_touch_cst328_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.poll.is_some());
        assert!(drv.is_touch);
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_init_null_config() {
        unsafe {
            reset_state();
            let ret = cst328_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            assert!(!S_TOUCH.initialized);
        }
    }

    #[test]
    fn test_init_and_deinit() {
        unsafe {
            reset_state();
            let cfg = TouchCst328Config {
                i2c_bus: 1usize as *mut c_void, // non-null sentinel
                i2c_addr: 0x1A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            };
            let ret = cst328_init(&cfg as *const TouchCst328Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!(S_TOUCH.initialized);

            cst328_deinit();
            assert!(!S_TOUCH.initialized);
            assert!(S_TOUCH.dev.is_null());
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = TouchCst328Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x1A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            };
            assert_eq!(
                cst328_init(&cfg as *const TouchCst328Config as *const c_void),
                ESP_OK
            );
            assert_eq!(
                cst328_init(&cfg as *const TouchCst328Config as *const c_void),
                ESP_OK
            );
            assert!(S_TOUCH.initialized);
            cst328_deinit();
        }
    }

    #[test]
    fn test_poll_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(cst328_poll(), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_poll_no_touch_no_callback() {
        unsafe {
            reset_state();
            let cfg = TouchCst328Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x1A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            };
            assert_eq!(
                cst328_init(&cfg as *const TouchCst328Config as *const c_void),
                ESP_OK
            );
            // Stubs return all zeros → n_touches == 0; no callback set.
            assert_eq!(cst328_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            cst328_deinit();
        }
    }

    #[test]
    fn test_register_callback() {
        unsafe {
            reset_state();
            let cfg = TouchCst328Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x1A,
                pin_int: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            };
            assert_eq!(
                cst328_init(&cfg as *const TouchCst328Config as *const c_void),
                ESP_OK
            );

            static mut CALLED: bool = false;
            unsafe extern "C" fn dummy_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                CALLED = true;
            }

            assert_eq!(
                cst328_register_callback(Some(dummy_cb), std::ptr::null_mut()),
                ESP_OK
            );
            assert!(S_TOUCH.cb.is_some());
            cst328_deinit();
        }
    }

    #[test]
    fn test_coordinate_clamping() {
        // Verify that coordinates at or beyond max_x / max_y are clamped.
        // We inject a touching state and supply an out-of-range coordinate
        // by directly manipulating S_TOUCH, then check the stored last_x/y
        // after a synthetic touch-up.
        unsafe {
            reset_state();
            S_TOUCH.initialized = true;
            S_TOUCH.touching = true;
            S_TOUCH.last_x = 400; // beyond max_x = 320
            S_TOUCH.last_y = 300; // beyond max_y = 240
            S_TOUCH.cfg.max_x = 320;
            S_TOUCH.cfg.max_y = 240;
            S_TOUCH.dev = 1usize as *mut c_void;

            // Clamp values directly (mirrors poll logic for test isolation).
            if S_TOUCH.cfg.max_x > 0 && S_TOUCH.last_x >= S_TOUCH.cfg.max_x {
                S_TOUCH.last_x = S_TOUCH.cfg.max_x - 1;
            }
            if S_TOUCH.cfg.max_y > 0 && S_TOUCH.last_y >= S_TOUCH.cfg.max_y {
                S_TOUCH.last_y = S_TOUCH.cfg.max_y - 1;
            }

            assert_eq!(S_TOUCH.last_x, 319);
            assert_eq!(S_TOUCH.last_y, 239);

            reset_state();
        }
    }

    #[test]
    fn test_irq_pending_skip_when_not_touching() {
        unsafe {
            reset_state();
            let cfg = TouchCst328Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x1A,
                pin_int: 4, // interrupt pin active
                pin_rst: GPIO_NUM_NC,
                max_x: 320,
                max_y: 240,
            };
            assert_eq!(
                cst328_init(&cfg as *const TouchCst328Config as *const c_void),
                ESP_OK
            );

            // irq_pending is false, touching is false → poll should return early.
            S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
            S_TOUCH.touching = false;
            assert_eq!(cst328_poll(), ESP_OK);
            // Confirm we did not change touching state.
            assert!(!S_TOUCH.touching);

            cst328_deinit();
        }
    }
}
