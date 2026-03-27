// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CardKB I2C keyboard driver (Rust)
//
// Driver for the M5Stack CardKB (and compatible) I2C keyboards.
//
// The CardKB is a simple I2C keyboard at address 0x5F.  Reading one byte
// returns the ASCII value of the currently pressed key, or 0 if no key is
// pressed.  Special keys use values >= 0x80 (function/arrow keys) or
// standard control codes (backspace 0x08, enter 0x0D, escape 0x1B, tab 0x09).
//
// Unlike the TCA8418, there is no interrupt pin and no on-chip FIFO —
// the host must poll periodically.  This driver maintains a small 8-entry
// software key buffer to handle burst reads.

#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData,
                          HalInputEventType, HalInputKeyData};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── CardKB constants ─────────────────────────────────────────────────────────

/// Default I2C address for the CardKB.
/// CardKB v1.0/v1.1 and Cardputer internal keyboard all use 0x5F.
/// The M5Stack Faces QWERTY (different product) uses 0x08.
pub const CARDKB_DEFAULT_ADDR: u8 = 0x5F;

/// Minimum valid I2C address (7-bit).
const I2C_ADDR_MIN: u8 = 0x03;
/// Maximum valid I2C address (7-bit).
const I2C_ADDR_MAX: u8 = 0x77;

/// Software key buffer capacity.
const KEY_BUF_SIZE: usize = 8;

// ── CardKB special key codes ─────────────────────────────────────────────────

// Fn key is a modifier — CardKB firmware never sends a standalone Fn byte.
// Arrow key codes match the real CardKB v1.0/v1.1 firmware output.
const CARDKB_KEY_ARROW_UP: u8 = 0xB5;
const CARDKB_KEY_ARROW_DOWN: u8 = 0xB6;
const CARDKB_KEY_ARROW_LEFT: u8 = 0xB4;
const CARDKB_KEY_ARROW_RIGHT: u8 = 0xB7;

const CARDKB_KEY_BACKSPACE: u8 = 0x08;
const CARDKB_KEY_ENTER: u8 = 0x0D;
const CARDKB_KEY_ESCAPE: u8 = 0x1B;
const CARDKB_KEY_TAB: u8 = 0x09;

// ── ThistleOS key codes ──────────────────────────────────────────────────────
//
// Map CardKB special codes to the same constants the TCA8418 driver uses.
// Printable ASCII passes through unchanged.

const THISTLE_KEY_BACKSPACE: u16 = 0x08;
const THISTLE_KEY_TAB: u16 = 0x09;
const THISTLE_KEY_ENTER: u16 = 0x0A; // normalise CR -> LF
const THISTLE_KEY_ESCAPE: u16 = 0x1B;

// Arrow keys — use values in the 0xF0xx range to avoid clashing with ASCII.
const THISTLE_KEY_UP: u16 = 0xF001;
const THISTLE_KEY_DOWN: u16 = 0xF002;
const THISTLE_KEY_LEFT: u16 = 0xF003;
const THISTLE_KEY_RIGHT: u16 = 0xF004;

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible config struct for the CardKB driver.
#[repr(C)]
pub struct KbdCardkbConfig {
    /// i2c_master_bus_handle_t
    pub i2c_bus: *mut c_void,
    /// I2C address (default 0x08)
    pub i2c_addr: u8,
}

// SAFETY: Config holds opaque C pointers; we never share them across threads
// except through the global static state which is guarded by the driver's
// single-initialisation semantics.
unsafe impl Send for KbdCardkbConfig {}
unsafe impl Sync for KbdCardkbConfig {}

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod esp_ffi {
    use std::os::raw::c_void;

    /// i2c_device_config_t (partial — only fields we set).
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

        pub fn esp_timer_get_time() -> i64;
    }
}

// ── Stub impls for non-ESP32 targets (host tests / simulator) ────────────────

#[cfg(not(target_os = "espidf"))]
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
        // On the host stub, read from the injectable key buffer instead of
        // returning all-zeros.  This enables meaningful poll() tests.
        let state = &mut *(&raw mut super::S_KBD);
        if rsz >= 1 && state.inject_head != state.inject_tail {
            let key = state.inject_buf[state.inject_head];
            state.inject_head = (state.inject_head + 1) % super::INJECT_BUF_SIZE;
            *read = key;
        } else {
            unsafe { std::ptr::write_bytes(read, 0, rsz) };
        }
        0
    }

    pub unsafe fn esp_timer_get_time() -> i64 { 0 }
}

// ── Inject buffer (test/simulator only) ──────────────────────────────────────

/// Size of the injectable key buffer for host/simulator builds.
const INJECT_BUF_SIZE: usize = 32;

// ── Driver state ─────────────────────────────────────────────────────────────

struct KbdState {
    /// I2C device handle (i2c_master_dev_handle_t)
    dev: *mut c_void,
    cfg: KbdCardkbConfig,
    cb: HalInputCb,
    cb_data: *mut c_void,
    initialized: bool,

    /// Software key buffer — stores mapped keycodes from burst reads.
    key_buf: [u16; KEY_BUF_SIZE],
    key_buf_head: usize,
    key_buf_tail: usize,

    /// Injectable key buffer for host/simulator testing.
    /// On ESP-IDF targets this is unused but kept for struct layout simplicity.
    inject_buf: [u8; INJECT_BUF_SIZE],
    inject_head: usize,
    inject_tail: usize,
}

// SAFETY: The driver state is accessed only from the HAL init/poll/deinit
// path which is guaranteed single-threaded by the HAL registry contract.
unsafe impl Send for KbdState {}
unsafe impl Sync for KbdState {}

impl KbdState {
    const fn new() -> Self {
        KbdState {
            dev: std::ptr::null_mut(),
            cfg: KbdCardkbConfig {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: CARDKB_DEFAULT_ADDR,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            initialized: false,
            key_buf: [0u16; KEY_BUF_SIZE],
            key_buf_head: 0,
            key_buf_tail: 0,
            inject_buf: [0u8; INJECT_BUF_SIZE],
            inject_head: 0,
            inject_tail: 0,
        }
    }

    /// Push a keycode into the software key buffer.
    /// Returns true if stored, false if buffer is full.
    fn key_buf_push(&mut self, keycode: u16) -> bool {
        let next_tail = (self.key_buf_tail + 1) % KEY_BUF_SIZE;
        if next_tail == self.key_buf_head {
            // Buffer full — drop the key
            return false;
        }
        self.key_buf[self.key_buf_tail] = keycode;
        self.key_buf_tail = next_tail;
        true
    }

    /// Pop a keycode from the software key buffer.
    /// Returns Some(keycode) if available, None if empty.
    fn key_buf_pop(&mut self) -> Option<u16> {
        if self.key_buf_head == self.key_buf_tail {
            return None;
        }
        let keycode = self.key_buf[self.key_buf_head];
        self.key_buf_head = (self.key_buf_head + 1) % KEY_BUF_SIZE;
        Some(keycode)
    }

    /// Returns the number of keys currently in the buffer.
    fn key_buf_len(&self) -> usize {
        if self.key_buf_tail >= self.key_buf_head {
            self.key_buf_tail - self.key_buf_head
        } else {
            KEY_BUF_SIZE - self.key_buf_head + self.key_buf_tail
        }
    }
}

static mut S_KBD: KbdState = KbdState::new();

// ── Key mapping ──────────────────────────────────────────────────────────────

/// Map a raw CardKB byte to a ThistleOS keycode.
/// Returns 0 for unmapped/unknown codes.
fn map_cardkb_key(raw: u8) -> u16 {
    match raw {
        0 => 0, // no key pressed
        CARDKB_KEY_BACKSPACE => THISTLE_KEY_BACKSPACE,
        CARDKB_KEY_TAB => THISTLE_KEY_TAB,
        CARDKB_KEY_ENTER => THISTLE_KEY_ENTER,
        CARDKB_KEY_ESCAPE => THISTLE_KEY_ESCAPE,
        CARDKB_KEY_ARROW_UP => THISTLE_KEY_UP,
        CARDKB_KEY_ARROW_DOWN => THISTLE_KEY_DOWN,
        CARDKB_KEY_ARROW_LEFT => THISTLE_KEY_LEFT,
        CARDKB_KEY_ARROW_RIGHT => THISTLE_KEY_RIGHT,
        // Printable ASCII range (space through tilde)
        0x20..=0x7E => raw as u16,
        // Other control codes or unknown — pass through as-is
        _ => raw as u16,
    }
}

// ── I2C helper ───────────────────────────────────────────────────────────────

/// Read a single byte from the CardKB.
///
/// The CardKB protocol is dead simple: send nothing (or send a dummy byte),
/// then read one byte.  Non-zero = keypress, zero = idle.
///
/// # Safety
/// Must be called with S_KBD.dev valid.
unsafe fn cardkb_read_key() -> (i32, u8) {
    let kbd = &*(&raw const S_KBD);
    let mut key: u8 = 0;
    // Some CardKB variants need a register address of 0x00, others just a
    // plain read.  Sending 0x00 as the write byte works universally.
    let reg: u8 = 0x00;
    let ret = esp_ffi::i2c_master_transmit_receive(
        kbd.dev,
        &reg as *const u8,
        1,
        &mut key as *mut u8,
        1,
        50,
    );
    (ret, key)
}

// ── Inject key (test/simulator) ──────────────────────────────────────────────

/// Inject a raw CardKB byte into the simulator/test I2C read buffer.
///
/// This allows tests to simulate keypresses without real hardware.
/// On ESP-IDF targets this function is still compiled but has no effect
/// because the I2C FFI reads from real hardware.
///
/// # Safety
/// Must be called when no concurrent poll() is running.
pub unsafe fn inject_key(ascii: u8) {
    let state = &mut *(&raw mut S_KBD);
    let next_tail = (state.inject_tail + 1) % INJECT_BUF_SIZE;
    if next_tail != state.inject_head {
        state.inject_buf[state.inject_tail] = ascii;
        state.inject_tail = next_tail;
    }
}

/// Clear the inject buffer.
///
/// # Safety
/// Must be called when no concurrent poll() is running.
pub unsafe fn inject_clear() {
    let state = &mut *(&raw mut S_KBD);
    state.inject_head = 0;
    state.inject_tail = 0;
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the CardKB driver.
///
/// # Safety
/// `config` must point to a valid `KbdCardkbConfig`.
unsafe extern "C" fn cardkb_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let kbd = &mut *(&raw mut S_KBD);

    if kbd.initialized {
        // Already initialised — idempotent
        return ESP_OK;
    }

    // Copy config
    let src = &*(config as *const KbdCardkbConfig);

    // Validate I2C address range
    if src.i2c_addr < I2C_ADDR_MIN || src.i2c_addr > I2C_ADDR_MAX {
        return ESP_ERR_INVALID_ARG;
    }

    if src.i2c_bus.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    kbd.cfg.i2c_bus = src.i2c_bus;
    kbd.cfg.i2c_addr = src.i2c_addr;

    // Reset key buffer
    kbd.key_buf_head = 0;
    kbd.key_buf_tail = 0;

    // Add the I2C device at 100 kHz (CardKB is a low-speed device)
    let dev_cfg = esp_ffi::I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: kbd.cfg.i2c_addr as u16,
        scl_speed_hz: 100_000,
        scl_wait_us: 0,
        flags: 0,
    };
    let ret = esp_ffi::i2c_master_bus_add_device(kbd.cfg.i2c_bus, &dev_cfg, &mut kbd.dev);
    if ret != ESP_OK {
        return ret;
    }

    kbd.initialized = true;
    ESP_OK
}

/// De-initialise the CardKB driver.
unsafe extern "C" fn cardkb_deinit() {
    let kbd = &mut *(&raw mut S_KBD);
    if !kbd.initialized {
        return;
    }

    if !kbd.dev.is_null() {
        esp_ffi::i2c_master_bus_rm_device(kbd.dev);
        kbd.dev = std::ptr::null_mut();
    }

    kbd.cb = None;
    kbd.cb_data = std::ptr::null_mut();
    kbd.key_buf_head = 0;
    kbd.key_buf_tail = 0;
    kbd.initialized = false;
}

/// Register the input event callback.
unsafe extern "C" fn cardkb_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    let kbd = &mut *(&raw mut S_KBD);
    kbd.cb = cb;
    kbd.cb_data = user_data;
    ESP_OK
}

/// Poll for pending key events and dispatch them via the registered callback.
///
/// Reads up to KEY_BUF_SIZE keys in a burst.  Each non-zero byte from the
/// CardKB is mapped and dispatched as a KeyDown event.  (The CardKB has no
/// concept of key-up — we report only press events.)
unsafe extern "C" fn cardkb_poll() -> i32 {
    let kbd = &mut *(&raw mut S_KBD);

    if !kbd.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // First, drain any previously buffered keys
    while let Some(keycode) = kbd.key_buf_pop() {
        dispatch_key(kbd, keycode);
    }

    // Burst-read up to KEY_BUF_SIZE keys from the CardKB
    for _ in 0..KEY_BUF_SIZE {
        let (ret, raw) = cardkb_read_key();
        if ret != ESP_OK {
            return ret;
        }

        if raw == 0 {
            // No more keys — CardKB returns 0 when idle
            break;
        }

        let keycode = map_cardkb_key(raw);
        if keycode == 0 {
            continue;
        }

        // Try to dispatch immediately; buffer if callback is not set yet
        if kbd.cb.is_some() {
            dispatch_key(kbd, keycode);
        } else {
            kbd.key_buf_push(keycode);
        }
    }

    ESP_OK
}

/// Dispatch a single key event to the registered callback.
///
/// The CardKB has no key-release concept, so we send only KeyDown events.
unsafe fn dispatch_key(kbd: &KbdState, keycode: u16) {
    if let Some(cb) = kbd.cb {
        let timestamp = (esp_ffi::esp_timer_get_time() / 1000) as u32;
        let event = HalInputEvent {
            event_type: HalInputEventType::KeyDown,
            timestamp,
            data: HalInputEventData {
                key: HalInputKeyData { keycode },
            },
        };
        cb(&event as *const HalInputEvent, kbd.cb_data);
    }
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL input driver vtable for the CardKB.
static KEYBOARD_DRIVER: HalInputDriver = HalInputDriver {
    init: Some(cardkb_init),
    deinit: Some(cardkb_deinit),
    register_callback: Some(cardkb_register_callback),
    poll: Some(cardkb_poll),
    name: b"CardKB\0".as_ptr() as *const c_char,
    is_touch: false,
};

/// Return the CardKB driver vtable.
///
/// # Safety
/// Returns a pointer to a static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_kbd_cardkb_get_driver() -> *const HalInputDriver {
    &KEYBOARD_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU16, Ordering as AtomicOrdering};

    /// Reset the global driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_KBD) = KbdState::new();
    }

    /// Helper: create a valid config with a non-null bus sentinel.
    fn test_config() -> KbdCardkbConfig {
        KbdCardkbConfig {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: CARDKB_DEFAULT_ADDR,
        }
    }

    #[test]
    fn test_vtable_pointer_is_non_null() {
        let p = drv_kbd_cardkb_get_driver();
        assert!(!p.is_null());
    }

    #[test]
    fn test_vtable_fields_are_populated() {
        let drv = unsafe { &*drv_kbd_cardkb_get_driver() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.poll.is_some());
        assert!(!drv.name.is_null());
        assert!(!drv.is_touch);
        // Verify the name string
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "CardKB");
    }

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            let ret = cardkb_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_null_bus_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = KbdCardkbConfig {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: CARDKB_DEFAULT_ADDR,
            };
            let ret = cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_invalid_i2c_addr_too_low() {
        unsafe {
            reset_state();
            let cfg = KbdCardkbConfig {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x01, // below minimum valid address
            };
            let ret = cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_invalid_i2c_addr_too_high() {
        unsafe {
            reset_state();
            let cfg = KbdCardkbConfig {
                i2c_bus: 1usize as *mut c_void,
                i2c_addr: 0x80, // above maximum valid address
            };
            let ret = cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_poll_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let ret = cardkb_poll();
            assert_eq!(ret, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = test_config();
            let ret = cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_KBD)).initialized);

            cardkb_deinit();
            assert!(!(*(&raw const S_KBD)).initialized);
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = test_config();
            let p = &cfg as *const KbdCardkbConfig as *const c_void;
            assert_eq!(cardkb_init(p), ESP_OK);
            assert_eq!(cardkb_init(p), ESP_OK); // idempotent
            cardkb_deinit();
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            // Should not panic
            cardkb_deinit();
            assert!(!(*(&raw const S_KBD)).initialized);
        }
    }

    #[test]
    fn test_register_callback_stores_values() {
        unsafe {
            reset_state();
            let cfg = test_config();
            cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);

            unsafe extern "C" fn dummy_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {}
            let sentinel = 0xDEAD_BEEFusize as *mut c_void;
            let ret = cardkb_register_callback(Some(dummy_cb), sentinel);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_KBD)).cb.is_some());
            assert_eq!((*(&raw const S_KBD)).cb_data, sentinel);

            cardkb_deinit();
        }
    }

    #[test]
    fn test_inject_and_poll_dispatches_key() {
        unsafe {
            reset_state();
            let cfg = test_config();
            cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);

            // Track the dispatched keycode via an atomic
            static LAST_KEY: AtomicU16 = AtomicU16::new(0);
            LAST_KEY.store(0, AtomicOrdering::Relaxed);

            unsafe extern "C" fn capture_cb(
                event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                let kd = (*event).data.key;
                LAST_KEY.store(kd.keycode, AtomicOrdering::Relaxed);
            }
            cardkb_register_callback(Some(capture_cb), std::ptr::null_mut());

            // Inject the letter 'A'
            inject_key(b'A');
            let ret = cardkb_poll();
            assert_eq!(ret, ESP_OK);
            assert_eq!(LAST_KEY.load(AtomicOrdering::Relaxed), b'A' as u16);

            cardkb_deinit();
        }
    }

    #[test]
    fn test_poll_no_key_returns_ok() {
        unsafe {
            reset_state();
            let cfg = test_config();
            cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);
            // No keys injected — should return OK and dispatch nothing
            let ret = cardkb_poll();
            assert_eq!(ret, ESP_OK);
            cardkb_deinit();
        }
    }

    #[test]
    fn test_special_key_mapping() {
        // Verify all special key mappings
        assert_eq!(map_cardkb_key(0), 0);
        assert_eq!(map_cardkb_key(CARDKB_KEY_BACKSPACE), THISTLE_KEY_BACKSPACE);
        assert_eq!(map_cardkb_key(CARDKB_KEY_TAB), THISTLE_KEY_TAB);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ENTER), THISTLE_KEY_ENTER);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ESCAPE), THISTLE_KEY_ESCAPE);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ARROW_UP), THISTLE_KEY_UP);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ARROW_DOWN), THISTLE_KEY_DOWN);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ARROW_LEFT), THISTLE_KEY_LEFT);
        assert_eq!(map_cardkb_key(CARDKB_KEY_ARROW_RIGHT), THISTLE_KEY_RIGHT);
        // Printable ASCII passthrough
        assert_eq!(map_cardkb_key(b' '), b' ' as u16);
        assert_eq!(map_cardkb_key(b'z'), b'z' as u16);
        assert_eq!(map_cardkb_key(b'~'), b'~' as u16);
    }

    #[test]
    fn test_key_buffer_overflow() {
        unsafe {
            reset_state();
            let kbd = &mut *(&raw mut S_KBD);

            // Fill the buffer to capacity (KEY_BUF_SIZE - 1 usable slots)
            for i in 0..(KEY_BUF_SIZE - 1) {
                assert!(kbd.key_buf_push((i + 1) as u16));
            }
            assert_eq!(kbd.key_buf_len(), KEY_BUF_SIZE - 1);

            // Next push should fail — buffer full
            assert!(!kbd.key_buf_push(0xFF));

            // Drain and verify order
            for i in 0..(KEY_BUF_SIZE - 1) {
                assert_eq!(kbd.key_buf_pop(), Some((i + 1) as u16));
            }
            assert_eq!(kbd.key_buf_pop(), None);
            assert_eq!(kbd.key_buf_len(), 0);
        }
    }

    #[test]
    fn test_inject_multiple_keys_poll_all() {
        unsafe {
            reset_state();
            let cfg = test_config();
            cardkb_init(&cfg as *const KbdCardkbConfig as *const c_void);

            static KEY_COUNT: AtomicU16 = AtomicU16::new(0);
            KEY_COUNT.store(0, AtomicOrdering::Relaxed);

            unsafe extern "C" fn count_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                KEY_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
            }
            cardkb_register_callback(Some(count_cb), std::ptr::null_mut());

            // Inject 3 keys
            inject_key(b'H');
            inject_key(b'i');
            inject_key(b'!');

            // Poll should read all 3 (burst read up to KEY_BUF_SIZE)
            let ret = cardkb_poll();
            assert_eq!(ret, ESP_OK);
            assert_eq!(KEY_COUNT.load(AtomicOrdering::Relaxed), 3);

            cardkb_deinit();
        }
    }
}
