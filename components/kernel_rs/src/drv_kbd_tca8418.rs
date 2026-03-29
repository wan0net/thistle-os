// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TCA8418 I2C keyboard matrix driver (Rust)
//
// Rust port of components/drv_kbd_tca8418/src/drv_kbd_tca8418.c.
//
// The TCA8418 scans up to an 8×10 key matrix and buffers key events in a
// 10-deep FIFO.  We expose an interrupt-driven poll() call: if pin_int is
// wired up the ISR sets a flag so that the poll is cheap when idle.
//
// The C driver remains untouched and is kept as a fallback.

#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hal_registry::{HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData,
                          HalInputEventType, HalInputKeyData};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── TCA8418 register map ─────────────────────────────────────────────────────

const TCA8418_REG_CFG: u8 = 0x01; // Configuration
const TCA8418_REG_INT_STAT: u8 = 0x02; // Interrupt status
const TCA8418_REG_KEY_LCK_EC: u8 = 0x03; // Key-lock + event count
const TCA8418_REG_KEY_EVENT_A: u8 = 0x04; // Key event FIFO (read repeatedly)
const TCA8418_REG_KP_GPIO1: u8 = 0x1D; // rows R0-R7 / cols C0-C7
const TCA8418_REG_KP_GPIO2: u8 = 0x1E; // cols C8-C9
const TCA8418_REG_KP_GPIO3: u8 = 0x1F;

// CFG bits
const TCA8418_CFG_KE_IEN: u8 = 1 << 0; // Key event interrupt enable
const TCA8418_CFG_AI: u8 = 1 << 7; // Auto-increment

// INT_STAT bits
const TCA8418_INT_STAT_K_INT: u8 = 1 << 0;

// Key event bits
const KEY_EVENT_PRESS: u8 = 0x80; // 1 = press, 0 = release
const KEY_EVENT_KEY_MSK: u8 = 0x7F;

// GPIO_NUM_NC sentinel (matches ESP-IDF GPIO_NUM_NC = -1)
const GPIO_NUM_NC: i32 = -1;

// ── Keymap ───────────────────────────────────────────────────────────────────
//
// TCA8418 encodes key position as: key_code = row*10 + col + 1  (1-based)
// Rows R0-R7 (8 rows), Cols C0-C9 (10 cols).
// Map to ASCII / special keycodes.  0 = unmapped.
//
// Matches the C driver's KEY_MAP exactly.

#[rustfmt::skip]
static KEY_MAP: [[u16; 10]; 8] = [
    // C0     C1     C2     C3     C4     C5     C6     C7     C8     C9
    [  b'q' as u16, b'w' as u16, b'e' as u16, b'r' as u16, b't' as u16, b'y' as u16, b'u' as u16, b'i' as u16, b'o' as u16, b'p' as u16  ],
    [  b'a' as u16, b's' as u16, b'd' as u16, b'f' as u16, b'g' as u16, b'h' as u16, b'j' as u16, b'k' as u16, b'l' as u16, 0x08u16      ],  // 0x08 = backspace
    [  b'z' as u16, b'x' as u16, b'c' as u16, b'v' as u16, b'b' as u16, b'n' as u16, b'm' as u16, b',' as u16, b'.' as u16, b'\n' as u16 ],
    [  0x01u16,     0x02u16,     0x03u16,     b' ' as u16, b'1' as u16, b'2' as u16, b'3' as u16, b'4' as u16, b'5' as u16, b'6' as u16  ],  // 0x01=Fn 0x02=Sym 0x03=Shift
    [  b'7' as u16, b'8' as u16, b'9' as u16, b'0' as u16, b'-' as u16, b'=' as u16, b'[' as u16, b']' as u16, b'\\' as u16, b'\'' as u16 ],
    [  b';' as u16, b'/' as u16, b'`' as u16, 0x1Bu16,     0,          0,           0,           0,           0,           0            ],  // 0x1B = Esc
    [  0,           0,           0,           0,           0,           0,           0,           0,           0,           0            ],
    [  0,           0,           0,           0,           0,           0,           0,           0,           0,           0            ],
];

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible config struct.  Must match `kbd_tca8418_config_t` in the C header.
#[repr(C)]
pub struct KbdTca8418Config {
    /// i2c_master_bus_handle_t
    pub i2c_bus: *mut c_void,
    /// I2C device address (default 0x34)
    pub i2c_addr: u8,
    /// Interrupt GPIO pin, active-low.  Use -1 (GPIO_NUM_NC) to disable.
    pub pin_int: i32,
}

// SAFETY: Config holds opaque C pointers; we never share them across threads
// except through the global static state which is guarded by the driver's
// single-initialisation semantics.
unsafe impl Send for KbdTca8418Config {}
unsafe impl Sync for KbdTca8418Config {}

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod esp_ffi {
    use std::os::raw::c_void;

    /// i2c_device_config_t (partial — only fields we set).
    /// Must match the ESP-IDF struct layout exactly.
    #[repr(C)]
    pub struct I2cDeviceConfig {
        /// dev_addr_length: I2C_ADDR_BIT_LEN_7 = 0
        pub dev_addr_length: u32,
        /// device_address
        pub device_address: u16,
        /// scl_speed_hz
        pub scl_speed_hz: u32,
        // Padding to match the full ESP-IDF struct size (20 bytes total on Xtensa).
        // Extra fields: scl_wait_us (u32) + flags field (u32)
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

        // GPIO
        pub fn gpio_set_direction(pin: u32, mode: u32) -> i32;
        pub fn gpio_set_pull_mode(pin: u32, mode: u32) -> i32;
        pub fn gpio_isr_handler_add(
            pin: u32,
            handler: unsafe extern "C" fn(*mut c_void),
            arg: *mut c_void,
        ) -> i32;
        pub fn gpio_isr_handler_remove(pin: u32) -> i32;
        pub fn gpio_set_intr_type(pin: u32, intr_type: u32) -> i32;
        pub fn gpio_intr_enable(pin: u32) -> i32;
        pub fn gpio_install_isr_service(flags: i32) -> i32;

        // Timer
        pub fn esp_timer_get_time() -> i64;
    }
}

// ── Extern "C" bindings for simulator with sim-bus feature ───────────────────

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
        pub fn i2c_master_bus_add_device(bus: *mut c_void, cfg: *const I2cDeviceConfig, handle: *mut *mut c_void) -> i32;
        pub fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
        pub fn i2c_master_transmit_receive(handle: *mut c_void, write_data: *const u8, write_size: usize, read_data: *mut u8, read_size: usize, timeout_ms: i32) -> i32;
        pub fn i2c_master_transmit(handle: *mut c_void, data: *const u8, len: usize, timeout_ms: i32) -> i32;
        pub fn gpio_set_direction(pin: u32, mode: u32) -> i32;
        pub fn gpio_set_pull_mode(pin: u32, mode: u32) -> i32;
        pub fn gpio_isr_handler_add(pin: u32, handler: unsafe extern "C" fn(*mut c_void), arg: *mut c_void) -> i32;
        pub fn gpio_isr_handler_remove(pin: u32) -> i32;
        pub fn gpio_set_intr_type(pin: u32, intr_type: u32) -> i32;
        pub fn gpio_intr_enable(pin: u32) -> i32;
        pub fn gpio_install_isr_service(flags: i32) -> i32;
        pub fn esp_timer_get_time() -> i64;
    }
}

// ── Stub impls for non-ESP32 targets without sim-bus (host tests) ────────────

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
        // Return a non-null sentinel so code can tell it "succeeded"
        *handle = 1usize as *mut c_void;
        0
    }
    pub unsafe fn i2c_master_bus_rm_device(_handle: *mut c_void) -> i32 { 0 }
    pub unsafe fn i2c_master_transmit_receive(
        _handle: *mut c_void,
        _write: *const u8,
        _wsz: usize,
        read: *mut u8,
        rsz: usize,
        _timeout: i32,
    ) -> i32 {
        // Fill read buffer with 0 (no events)
        unsafe { std::ptr::write_bytes(read, 0, rsz) };
        0
    }
    pub unsafe fn i2c_master_transmit(
        _handle: *mut c_void,
        _data: *const u8,
        _len: usize,
        _timeout: i32,
    ) -> i32 { 0 }
    pub unsafe fn gpio_set_direction(_pin: u32, _mode: u32) -> i32 { 0 }
    pub unsafe fn gpio_set_pull_mode(_pin: u32, _mode: u32) -> i32 { 0 }
    pub unsafe fn gpio_isr_handler_add(
        _pin: u32,
        _handler: unsafe extern "C" fn(*mut c_void),
        _arg: *mut c_void,
    ) -> i32 { 0 }
    pub unsafe fn gpio_isr_handler_remove(_pin: u32) -> i32 { 0 }
    pub unsafe fn gpio_set_intr_type(_pin: u32, _intr_type: u32) -> i32 { 0 }
    pub unsafe fn gpio_intr_enable(_pin: u32) -> i32 { 0 }
    pub unsafe fn gpio_install_isr_service(_flags: i32) -> i32 { 0 }
    pub unsafe fn esp_timer_get_time() -> i64 { 0 }
}

// ── Driver state ─────────────────────────────────────────────────────────────

struct KbdState {
    /// I2C device handle (i2c_master_dev_handle_t)
    dev: *mut c_void,
    cfg: KbdTca8418Config,
    cb: HalInputCb,
    cb_data: *mut c_void,
    irq_pending: AtomicBool,
    initialized: bool,
}

// SAFETY: The driver state is accessed only from the HAL init/poll/deinit
// path which is guaranteed single-threaded by the HAL registry contract.
// The ISR only writes one AtomicBool flag.
unsafe impl Send for KbdState {}
unsafe impl Sync for KbdState {}

impl KbdState {
    const fn new() -> Self {
        KbdState {
            dev: std::ptr::null_mut(),
            cfg: KbdTca8418Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x34,
                pin_int: GPIO_NUM_NC,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            irq_pending: AtomicBool::new(false),
            initialized: false,
        }
    }
}

static mut S_KBD: KbdState = KbdState::new();

// ── I2C helpers ──────────────────────────────────────────────────────────────

/// Write a single register on the TCA8418.
///
/// # Safety
/// Must be called with S_KBD.dev valid.
unsafe fn tca8418_write_reg(reg: u8, val: u8) -> i32 {
    let buf: [u8; 2] = [reg, val];
    esp_ffi::i2c_master_transmit(S_KBD.dev, buf.as_ptr(), 2, 50)
}

/// Read a single register from the TCA8418.
///
/// # Safety
/// Must be called with S_KBD.dev valid.
unsafe fn tca8418_read_reg(reg: u8, val: &mut u8) -> i32 {
    esp_ffi::i2c_master_transmit_receive(S_KBD.dev, &reg as *const u8, 1, val as *mut u8, 1, 50)
}

// ── ISR ──────────────────────────────────────────────────────────────────────

/// Interrupt service routine — sets the irq_pending flag.
///
/// Marked `#[link_section = ".iram1"]` so it lives in IRAM on ESP-IDF targets
/// (equivalent to the C `IRAM_ATTR` macro).  On host targets the attribute is
/// ignored.
#[cfg_attr(target_os = "espidf", link_section = ".iram1.tca8418_isr")]
#[no_mangle]
pub unsafe extern "C" fn tca8418_isr_handler(_arg: *mut c_void) {
    // SAFETY: AtomicBool store is async-signal-safe and this is the only
    // write path from ISR context.  We use raw pointer access to avoid
    // the static_mut_refs lint while remaining compatible with older editions.
    let irq = &raw mut S_KBD;
    (*irq).irq_pending.store(true, Ordering::Release);
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the TCA8418 driver.
///
/// # Safety
/// `config` must point to a valid `KbdTca8418Config`.
unsafe extern "C" fn tca8418_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let kbd = &mut *(&raw mut S_KBD);

    if kbd.initialized {
        // Already initialised — idempotent
        return ESP_OK;
    }

    // Copy config
    let src = &*(config as *const KbdTca8418Config);
    kbd.cfg.i2c_bus = src.i2c_bus;
    kbd.cfg.i2c_addr = src.i2c_addr;
    kbd.cfg.pin_int = src.pin_int;
    kbd.irq_pending.store(false, Ordering::Relaxed);

    // Add the I2C device at 400 kHz
    let dev_cfg = esp_ffi::I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: kbd.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
        scl_wait_us: 0,
        flags: 0,
    };
    let ret = esp_ffi::i2c_master_bus_add_device(kbd.cfg.i2c_bus, &dev_cfg, &mut kbd.dev);
    if ret != ESP_OK {
        return ret;
    }

    // Enable auto-increment and key-event interrupt
    let ret = tca8418_write_reg(TCA8418_REG_CFG, TCA8418_CFG_KE_IEN | TCA8418_CFG_AI);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
        return ret;
    }

    // Configure R0-R7 as keypad rows (bits [7:0] of KP_GPIO1)
    let ret = tca8418_write_reg(TCA8418_REG_KP_GPIO1, 0xFF);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
        return ret;
    }

    // Configure C0-C7 (KP_GPIO2) and C8-C9 (KP_GPIO3 bits [1:0])
    let ret = tca8418_write_reg(TCA8418_REG_KP_GPIO2, 0xFF);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
        return ret;
    }

    let ret = tca8418_write_reg(TCA8418_REG_KP_GPIO3, 0x03);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
        return ret;
    }

    // Clear any stale interrupts
    let ret = tca8418_write_reg(TCA8418_REG_INT_STAT, 0xFF);
    if ret != ESP_OK {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
        return ret;
    }

    // Optional interrupt pin (active-low, falling edge)
    if kbd.cfg.pin_int != GPIO_NUM_NC {
        let pin = kbd.cfg.pin_int as u32;

        // GPIO_MODE_INPUT = 1
        esp_ffi::gpio_set_direction(pin, 1);
        // GPIO_PULLUP_ENABLE = 1
        esp_ffi::gpio_set_pull_mode(pin, 1);
        // GPIO_INTR_NEGEDGE = 2
        esp_ffi::gpio_set_intr_type(pin, 2);
        esp_ffi::gpio_intr_enable(pin);

        // gpio_install_isr_service is idempotent; ignore ESP_ERR_INVALID_STATE
        esp_ffi::gpio_install_isr_service(0);

        let ret = esp_ffi::gpio_isr_handler_add(pin, tca8418_isr_handler, std::ptr::null_mut());
        if ret != ESP_OK {
            esp_ffi::i2c_master_bus_rm_device(kbd.dev);
            kbd.dev = std::ptr::null_mut();
            return ret;
        }

        // Mark pending so the first poll drains anything already in the FIFO.
        kbd.irq_pending.store(true, Ordering::Release);
    }

    kbd.initialized = true;
    ESP_OK
}

/// De-initialise the TCA8418 driver.
unsafe extern "C" fn tca8418_deinit() {
    let kbd = &mut *(&raw mut S_KBD);
    if !kbd.initialized {
        return;
    }

    if kbd.cfg.pin_int != GPIO_NUM_NC {
        esp_ffi::gpio_isr_handler_remove(kbd.cfg.pin_int as u32);
    }

    if !kbd.dev.is_null() {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
    }

    kbd.cb = None;
    kbd.cb_data = std::ptr::null_mut();
    kbd.initialized = false;
}

/// Register the input event callback.
unsafe extern "C" fn tca8418_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    let kbd = &mut *(&raw mut S_KBD);
    kbd.cb = cb;
    kbd.cb_data = user_data;
    ESP_OK
}

/// Poll for pending key events and dispatch them via the registered callback.
unsafe extern "C" fn tca8418_poll() -> i32 {
    let kbd = &mut *(&raw mut S_KBD);

    if !kbd.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // If using the interrupt pin, skip I2C traffic when no interrupt fired.
    if kbd.cfg.pin_int != GPIO_NUM_NC && !kbd.irq_pending.load(Ordering::Acquire) {
        return ESP_OK;
    }

    // Read interrupt status register
    let mut int_stat: u8 = 0;
    let ret = tca8418_read_reg(TCA8418_REG_INT_STAT, &mut int_stat);
    if ret != ESP_OK {
        return ret;
    }

    if (int_stat & TCA8418_INT_STAT_K_INT) == 0 {
        // No key event — clear the pending flag and return
        kbd.irq_pending.store(false, Ordering::Release);
        return ESP_OK;
    }

    // Read event count from KEY_LCK_EC (lower nibble)
    let mut ec_reg: u8 = 0;
    let ret = tca8418_read_reg(TCA8418_REG_KEY_LCK_EC, &mut ec_reg);
    if ret != ESP_OK {
        return ret;
    }

    let mut event_count = ec_reg & 0x0F;
    if event_count == 0 {
        // Overflow — drain all 10 FIFO slots
        event_count = 10;
    }

    let mut last_ret = ESP_OK;

    for _ in 0..event_count {
        let mut ev: u8 = 0;
        let ret = tca8418_read_reg(TCA8418_REG_KEY_EVENT_A, &mut ev);
        if ret != ESP_OK {
            last_ret = ret;
            break;
        }

        if ev == 0 {
            // FIFO empty sentinel
            break;
        }

        let pressed = (ev & KEY_EVENT_PRESS) != 0;
        let key_code = ev & KEY_EVENT_KEY_MSK;

        if key_code == 0 {
            continue;
        }

        // key_code is 1-based: key_code = row*10 + col + 1
        let kc0 = key_code - 1; // make 0-based
        let row = (kc0 / 10) as usize;
        let col = (kc0 % 10) as usize;

        let keycode: u16 = if row < 8 && col < 10 {
            KEY_MAP[row][col]
        } else {
            0
        };

        if keycode == 0 {
            // Unmapped key — skip silently
            continue;
        }

        if let Some(cb) = kbd.cb {
            let timestamp = (esp_ffi::esp_timer_get_time() / 1000) as u32;
            let event = HalInputEvent {
                event_type: if pressed {
                    HalInputEventType::KeyDown
                } else {
                    HalInputEventType::KeyUp
                },
                timestamp,
                data: HalInputEventData {
                    key: HalInputKeyData { keycode },
                },
            };
            cb(&event as *const HalInputEvent, kbd.cb_data);
        }
    }

    // Clear the key-event interrupt bit by writing 1 to it (W1C)
    tca8418_write_reg(TCA8418_REG_INT_STAT, TCA8418_INT_STAT_K_INT);
    kbd.irq_pending.store(false, Ordering::Release);

    last_ret
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL input driver vtable for the TCA8418.
///
/// Returned by `drv_kbd_tca8418_get()` and passed to `hal_input_register()`.
static KEYBOARD_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(tca8418_init),
    deinit: Some(tca8418_deinit),
    register_callback: Some(tca8418_register_callback),
    poll: Some(tca8418_poll),
    name: b"TCA8418\0".as_ptr() as *const c_char,
    is_touch: false,
};

/// Return the TCA8418 driver vtable.
///
/// Drop-in replacement for the C `drv_kbd_tca8418_get()`.
///
/// # Safety
/// Returns a pointer to a static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_kbd_tca8418_get() -> *const HalInputDriver {
    &KEYBOARD_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the global driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_KBD) = KbdState::new();
    }

    #[test]
    fn test_vtable_pointer_is_non_null() {
        let p = drv_kbd_tca8418_get();
        assert!(!p.is_null());
    }

    #[test]
    fn test_vtable_fields_are_populated() {
        let drv = unsafe { &*drv_kbd_tca8418_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.poll.is_some());
        assert!(!drv.name.is_null());
        assert!(!drv.is_touch);
    }

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            let ret = tca8418_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_poll_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let ret = tca8418_poll();
            assert_eq!(ret, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = KbdTca8418Config {
                i2c_bus: 1usize as *mut c_void, // non-null sentinel
                i2c_addr: 0x34,
                pin_int: GPIO_NUM_NC,
            };
            let ret = tca8418_init(&cfg as *const KbdTca8418Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_KBD)).initialized);

            tca8418_deinit();
            assert!(!(*(&raw const S_KBD)).initialized);
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = KbdTca8418Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x34,
                pin_int: GPIO_NUM_NC,
            };
            let p = &cfg as *const KbdTca8418Config as *const c_void;
            assert_eq!(tca8418_init(p), ESP_OK);
            assert_eq!(tca8418_init(p), ESP_OK); // idempotent
        }
    }

    #[test]
    fn test_register_callback_stores_values() {
        unsafe {
            reset_state();
            let cfg = KbdTca8418Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x34,
                pin_int: GPIO_NUM_NC,
            };
            tca8418_init(&cfg as *const KbdTca8418Config as *const c_void);

            unsafe extern "C" fn dummy_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
            }
            let sentinel = 0xDEAD_BEEFusize as *mut c_void;
            let ret = tca8418_register_callback(Some(dummy_cb), sentinel);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_KBD)).cb.is_some());
            assert_eq!((*(&raw const S_KBD)).cb_data, sentinel);

            tca8418_deinit();
        }
    }

    #[test]
    fn test_poll_after_init_no_interrupt_returns_ok() {
        unsafe {
            reset_state();
            let cfg = KbdTca8418Config {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x34,
                pin_int: GPIO_NUM_NC, // polling mode — always check
            };
            tca8418_init(&cfg as *const KbdTca8418Config as *const c_void);
            // Stub returns 0 for INT_STAT (no K_INT set), so poll is a no-op.
            let ret = tca8418_poll();
            assert_eq!(ret, ESP_OK);
            tca8418_deinit();
        }
    }

    #[test]
    fn test_keymap_first_row() {
        // Spot-check the keymap against the C driver values
        assert_eq!(KEY_MAP[0][0], b'q' as u16);
        assert_eq!(KEY_MAP[0][9], b'p' as u16);
        assert_eq!(KEY_MAP[1][9], 0x08); // backspace
        assert_eq!(KEY_MAP[2][9], b'\n' as u16);
        assert_eq!(KEY_MAP[3][0], 0x01); // Fn
        assert_eq!(KEY_MAP[3][1], 0x02); // Sym
        assert_eq!(KEY_MAP[3][2], 0x03); // Shift
        assert_eq!(KEY_MAP[5][3], 0x1B); // Esc
    }

    #[test]
    fn test_keymap_unmapped_rows_are_zero() {
        for col in 0..10 {
            assert_eq!(KEY_MAP[6][col], 0);
            assert_eq!(KEY_MAP[7][col], 0);
        }
    }

    #[test]
    fn test_isr_handler_sets_irq_pending() {
        unsafe {
            reset_state();
            (*(&raw mut S_KBD)).irq_pending.store(false, Ordering::Relaxed);
            tca8418_isr_handler(std::ptr::null_mut());
            assert!((*(&raw const S_KBD)).irq_pending.load(Ordering::Acquire));
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            // Should not panic
            tca8418_deinit();
            assert!(!(*(&raw const S_KBD)).initialized);
        }
    }
}
