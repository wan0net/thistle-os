// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SSD1306 128×64 monochrome OLED display driver (Rust)
//
// The SSD1306 is a common 128×64 monochrome OLED controller addressed via I2C
// (address 0x3C or 0x3D).  It uses a page-based framebuffer: 8 pages × 128
// columns × 1 byte, where each byte represents 8 vertical pixels (LSB = top).
// Total framebuffer: 1024 bytes.
//
// Init sequence follows the Adafruit / common SSD1306 datasheet recommendation.
// Flush writes the full framebuffer via I2C using the "Horizontal Addressing
// Mode" (0x00), which auto-increments column then page.
//
// The driver exposes a HalDisplayDriver vtable with:
//   - display_type = Lcd   (no deferred refresh; flush is immediate)
//   - refresh      = None  (same as LCD — no separate refresh call needed)
//   - width  = 128
//   - height = 64

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalArea, HalDisplayDriver, HalDisplayRefreshMode, HalDisplayType};

// ── ESP error codes ───────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Display geometry ──────────────────────────────────────────────────────────

const OLED_WIDTH: u16 = 128;
const OLED_HEIGHT: u16 = 64;
const OLED_PAGES: usize = (OLED_HEIGHT as usize) / 8; // 8
const OLED_FB_SIZE: usize = (OLED_WIDTH as usize) * OLED_PAGES; // 1024

// GPIO_NUM_NC sentinel
const GPIO_NUM_NC: i32 = -1;

// ── SSD1306 command bytes ─────────────────────────────────────────────────────

const SSD1306_CMD_DISPLAY_OFF:       u8 = 0xAE;
const SSD1306_CMD_DISPLAY_ON:        u8 = 0xAF;
const SSD1306_CMD_SET_CONTRAST:      u8 = 0x81;
const SSD1306_CMD_ENTIRE_DISPLAY_ON: u8 = 0xA4; // follow VRAM
const SSD1306_CMD_NORMAL_DISPLAY:    u8 = 0xA6; // not inverted
const SSD1306_CMD_SET_ADDR_MODE:     u8 = 0x20;
const SSD1306_ADDR_MODE_HORIZONTAL:  u8 = 0x00;
const SSD1306_CMD_SET_COL_ADDR:      u8 = 0x21;
const SSD1306_CMD_SET_PAGE_ADDR:     u8 = 0x22;
const SSD1306_CMD_SET_MUX_RATIO:     u8 = 0xA8;
const SSD1306_CMD_SET_DISP_OFFSET:   u8 = 0xD3;
const SSD1306_CMD_SET_START_LINE:    u8 = 0x40; // OR with start line (0)
const SSD1306_CMD_SET_SEG_REMAP:     u8 = 0xA1; // column 127 → SEG0 (flip H)
const SSD1306_CMD_SET_COM_SCAN_DIR:  u8 = 0xC8; // remapped (flip V)
const SSD1306_CMD_SET_COM_PINS:      u8 = 0xDA;
const SSD1306_CMD_COM_PINS_ALT:      u8 = 0x12; // alternative COM pin config for 128×64
const SSD1306_CMD_SET_DISP_CLK:      u8 = 0xD5;
const SSD1306_CMD_DISP_CLK_DEFAULT:  u8 = 0x80;
const SSD1306_CMD_SET_PRECHARGE:     u8 = 0xD9;
const SSD1306_CMD_PRECHARGE_DEFAULT: u8 = 0x22;
const SSD1306_CMD_SET_VCOM_DESEL:    u8 = 0xDB;
const SSD1306_CMD_VCOM_DESEL_DEFAULT:u8 = 0x30;
const SSD1306_CMD_CHARGE_PUMP:       u8 = 0x8D;
const SSD1306_CMD_CHARGE_PUMP_ON:    u8 = 0x14;

/// I2C control byte prefix for command transmissions.
/// 0x00 = Co=0, D/C#=0 → following bytes are commands.
const SSD1306_CMD_PREFIX: u8 = 0x00;
/// I2C control byte prefix for data transmissions.
/// 0x40 = Co=0, D/C#=1 → following bytes are GDDRAM data.
const SSD1306_DATA_PREFIX: u8 = 0x40;

// ── I2C device config layout (mirrors i2c_device_config_t) ──────────────────

#[repr(C)]
struct I2cDeviceConfig {
    dev_addr_length: u32,
    device_address: u16,
    scl_speed_hz: u32,
}

// ── Configuration struct ──────────────────────────────────────────────────────

/// Configuration for the SSD1306 OLED driver.
///
/// Layout must match `oled_ssd1306_config_t` from the board init side.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OledSsd1306Config {
    /// I2C master bus handle (`i2c_master_bus_handle_t`).
    pub i2c_bus: *mut c_void,
    /// I2C device address: 0x3C (SA0 = GND) or 0x3D (SA0 = VCC).
    pub i2c_addr: u8,
    /// Hardware reset GPIO; -1 = no reset pin.
    pub pin_rst: i32,
    /// External power enable GPIO (Heltec V3: GPIO 36, drive LOW to power on).
    /// -1 = no Vext pin (OLED is always powered).
    pub pin_vext: i32,
}

// ── Driver state ──────────────────────────────────────────────────────────────

struct OledState {
    cfg: OledSsd1306Config,
    dev: *mut c_void,                // i2c_master_dev_handle_t
    fb: [u8; OLED_FB_SIZE],          // 1-bit monochrome framebuffer, page-packed
    initialized: bool,
    display_on: bool,
}

// SAFETY: Only mutated during single-threaded board init / driver calls.
unsafe impl Send for OledState {}
unsafe impl Sync for OledState {}

impl OledState {
    const fn new() -> Self {
        OledState {
            cfg: OledSsd1306Config {
                i2c_bus: std::ptr::null_mut(),
                i2c_addr: 0x3C,
                pin_rst: GPIO_NUM_NC,
                pin_vext: GPIO_NUM_NC,
            },
            dev: std::ptr::null_mut(),
            fb: [0u8; OLED_FB_SIZE],
            initialized: false,
            display_on: false,
        }
    }
}

struct GlobalOledState {
    inner: UnsafeCell<OledState>,
}

// SAFETY: Only mutated during single-threaded board init.
unsafe impl Sync for GlobalOledState {}

static STATE: GlobalOledState = GlobalOledState {
    inner: UnsafeCell::new(OledState::new()),
};

#[inline]
fn state() -> &'static OledState {
    unsafe { &*STATE.inner.get() }
}

#[inline]
fn state_mut() -> &'static mut OledState {
    unsafe { &mut *STATE.inner.get() }
}

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
extern "C" {
    fn i2c_master_bus_add_device(
        bus: *mut c_void,
        cfg: *const I2cDeviceConfig,
        handle: *mut *mut c_void,
    ) -> i32;
    fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
    fn i2c_master_transmit(
        handle: *mut c_void,
        data: *const u8,
        len: usize,
        timeout_ms: i32,
    ) -> i32;
    fn gpio_set_direction(pin: i32, mode: u32) -> i32;
    fn gpio_set_level(pin: i32, level: u32) -> i32;
    fn vTaskDelay(ticks: u32);
}

// ── Stub implementations (simulator / host tests) ─────────────────────────────

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
unsafe fn i2c_master_transmit(
    _handle: *mut c_void,
    _data: *const u8,
    _len: usize,
    _timeout_ms: i32,
) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_set_direction(_pin: i32, _mode: u32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_set_level(_pin: i32, _level: u32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelay(_ticks: u32) {}

// ── I2C helpers ───────────────────────────────────────────────────────────────

/// Send a single SSD1306 command byte.
///
/// # Safety
/// `state().dev` must be a valid I2C device handle.
unsafe fn ssd1306_send_cmd(cmd: u8) -> i32 {
    let buf = [SSD1306_CMD_PREFIX, cmd];
    i2c_master_transmit(state().dev, buf.as_ptr(), buf.len(), 50)
}

/// Send two SSD1306 command bytes (command + parameter).
///
/// # Safety
/// `state().dev` must be a valid I2C device handle.
unsafe fn ssd1306_send_cmd2(cmd: u8, arg: u8) -> i32 {
    let buf = [SSD1306_CMD_PREFIX, cmd, arg];
    i2c_master_transmit(state().dev, buf.as_ptr(), buf.len(), 50)
}

/// Send the full framebuffer to the display via I2C.
///
/// Prepends the data-mode control byte (0x40) and transmits the entire
/// 1024-byte framebuffer in one I2C transaction.
///
/// # Safety
/// `state().dev` must be a valid I2C device handle.
unsafe fn ssd1306_send_framebuffer() -> i32 {
    // Build a 1025-byte buffer: [0x40, fb[0], fb[1], ..., fb[1023]]
    // Allocate on the stack; 1025 bytes is within ESP32-S3 stack limits.
    let mut buf = [0u8; 1 + OLED_FB_SIZE];
    buf[0] = SSD1306_DATA_PREFIX;
    buf[1..].copy_from_slice(&state().fb);
    i2c_master_transmit(state().dev, buf.as_ptr(), buf.len(), 200)
}

// ── Hardware reset ─────────────────────────────────────────────────────────

/// Assert the RST pin low for 10 ms then release high for 100 ms.
///
/// # Safety
/// Calls ESP-IDF GPIO and FreeRTOS APIs.
unsafe fn ssd1306_hw_reset(pin_rst: i32) {
    if pin_rst == GPIO_NUM_NC {
        return;
    }
    gpio_set_direction(pin_rst, 2); // GPIO_MODE_OUTPUT
    gpio_set_level(pin_rst, 0);
    vTaskDelay(10); // 10 ms
    gpio_set_level(pin_rst, 1);
    vTaskDelay(100); // 100 ms
}

// ── Init sequence ─────────────────────────────────────────────────────────────

/// Send the standard SSD1306 128×64 initialisation command sequence.
///
/// # Safety
/// `state().dev` must be a valid I2C device handle.
unsafe fn ssd1306_init_sequence() -> i32 {
    // Display off during init
    let mut r = ssd1306_send_cmd(SSD1306_CMD_DISPLAY_OFF);
    if r != ESP_OK { return r; }

    // Oscillator frequency & divide ratio
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_DISP_CLK, SSD1306_CMD_DISP_CLK_DEFAULT);
    if r != ESP_OK { return r; }

    // Multiplex ratio (63 = 64 rows)
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_MUX_RATIO, 63);
    if r != ESP_OK { return r; }

    // Display offset = 0
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_DISP_OFFSET, 0);
    if r != ESP_OK { return r; }

    // Start line 0
    r = ssd1306_send_cmd(SSD1306_CMD_SET_START_LINE | 0);
    if r != ESP_OK { return r; }

    // Charge pump enabled (required for typical SSD1306 modules without VCC pin)
    r = ssd1306_send_cmd2(SSD1306_CMD_CHARGE_PUMP, SSD1306_CMD_CHARGE_PUMP_ON);
    if r != ESP_OK { return r; }

    // Horizontal addressing mode (auto-increment column → page)
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_ADDR_MODE, SSD1306_ADDR_MODE_HORIZONTAL);
    if r != ESP_OK { return r; }

    // Segment remap (column 127 → SEG0 for correct left-to-right)
    r = ssd1306_send_cmd(SSD1306_CMD_SET_SEG_REMAP);
    if r != ESP_OK { return r; }

    // COM scan direction remapped (for correct top-to-bottom)
    r = ssd1306_send_cmd(SSD1306_CMD_SET_COM_SCAN_DIR);
    if r != ESP_OK { return r; }

    // COM pins hardware configuration (alt config, no left-right remap)
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_COM_PINS, SSD1306_CMD_COM_PINS_ALT);
    if r != ESP_OK { return r; }

    // Contrast (mid-level default)
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_CONTRAST, 0xCF);
    if r != ESP_OK { return r; }

    // Pre-charge period
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_PRECHARGE, SSD1306_CMD_PRECHARGE_DEFAULT);
    if r != ESP_OK { return r; }

    // VCOM deselect level
    r = ssd1306_send_cmd2(SSD1306_CMD_SET_VCOM_DESEL, SSD1306_CMD_VCOM_DESEL_DEFAULT);
    if r != ESP_OK { return r; }

    // Entire display on (follow VRAM, not force on)
    r = ssd1306_send_cmd(SSD1306_CMD_ENTIRE_DISPLAY_ON);
    if r != ESP_OK { return r; }

    // Normal (non-inverted) display
    r = ssd1306_send_cmd(SSD1306_CMD_NORMAL_DISPLAY);
    if r != ESP_OK { return r; }

    // Display on
    ssd1306_send_cmd(SSD1306_CMD_DISPLAY_ON)
}

// ── HAL vtable functions ──────────────────────────────────────────────────────

/// Initialise the SSD1306 OLED display.
///
/// `config` must point to an `OledSsd1306Config`.
///
/// # Safety
/// Called from C via the HAL vtable; `config` must be valid.
pub unsafe extern "C" fn ssd1306_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let s = state_mut();
    if s.initialized {
        return ESP_OK;
    }

    let cfg = &*(config as *const OledSsd1306Config);
    s.cfg.i2c_bus  = cfg.i2c_bus;
    s.cfg.i2c_addr = cfg.i2c_addr;
    s.cfg.pin_rst  = cfg.pin_rst;
    s.cfg.pin_vext = cfg.pin_vext;

    // Clear framebuffer
    s.fb = [0u8; OLED_FB_SIZE];

    // Enable external power rail (Heltec V3: GPIO 36 driven LOW powers the OLED)
    if s.cfg.pin_vext != GPIO_NUM_NC {
        gpio_set_direction(s.cfg.pin_vext, 2); // GPIO_MODE_OUTPUT
        gpio_set_level(s.cfg.pin_vext, 0);     // LOW = power on
        vTaskDelay(2); // ~20ms for rail to stabilise
    }

    // Optional hardware reset
    ssd1306_hw_reset(s.cfg.pin_rst);

    // Add I2C device
    let dev_cfg = I2cDeviceConfig {
        dev_addr_length: 0, // I2C_ADDR_BIT_LEN_7
        device_address: s.cfg.i2c_addr as u16,
        scl_speed_hz: 400_000,
    };
    let ret = i2c_master_bus_add_device(s.cfg.i2c_bus, &dev_cfg, &mut s.dev);
    if ret != ESP_OK {
        return ret;
    }

    // Send init command sequence
    let ret = ssd1306_init_sequence();
    if ret != ESP_OK {
        i2c_master_bus_rm_device(s.dev);
        s.dev = std::ptr::null_mut();
        return ret;
    }

    // Flush the blank framebuffer so the screen is cleared
    let ret = ssd1306_send_framebuffer();
    if ret != ESP_OK {
        i2c_master_bus_rm_device(s.dev);
        s.dev = std::ptr::null_mut();
        return ret;
    }

    s.initialized = true;
    s.display_on  = true;
    ESP_OK
}

/// De-initialise the SSD1306 driver.
///
/// Turns the display off and releases the I2C device handle.
///
/// # Safety
/// Called from C via the HAL vtable.
pub unsafe extern "C" fn ssd1306_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }

    // Turn display off before releasing resources
    ssd1306_send_cmd(SSD1306_CMD_DISPLAY_OFF);

    i2c_master_bus_rm_device(s.dev);
    s.dev = std::ptr::null_mut();
    s.initialized = false;
    s.display_on  = false;
}

/// Flush a rectangular region to the display.
///
/// Because the SSD1306 uses a page-based monochrome framebuffer we cannot
/// efficiently update a sub-region without re-packing the entire framebuffer.
/// We therefore:
///   1. Convert the provided RGB565 `color_data` to 1-bit luminance and pack
///      into the affected pages of the internal framebuffer.
///   2. Flush the entire framebuffer via I2C.
///
/// Luminance threshold: a pixel is "on" if the 5-bit red channel > 8 (roughly
/// > 25% brightness on a greyscale ramp).
///
/// # Safety
/// Called from C via the HAL vtable.
pub unsafe extern "C" fn ssd1306_flush(area: *const HalArea, color_data: *const u8) -> i32 {
    let s = state_mut();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if area.is_null() || color_data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let a = &*area;
    let x1 = a.x1 as usize;
    let y1 = a.y1 as usize;
    let x2 = (a.x2 as usize).min(OLED_WIDTH as usize - 1);
    let y2 = (a.y2 as usize).min(OLED_HEIGHT as usize - 1);

    let src_w = (x2 - x1 + 1) as usize;

    for py in y1..=y2 {
        let page = py / 8;
        let bit  = py % 8;
        for px in x1..=x2 {
            // color_data is RGB565 LE: low byte then high byte
            let pixel_idx = (py - y1) * src_w + (px - x1);
            let byte_lo = *color_data.add(pixel_idx * 2) as u16;
            let byte_hi = *color_data.add(pixel_idx * 2 + 1) as u16;
            let rgb565  = byte_lo | (byte_hi << 8);

            // Extract 5-bit red channel (bits 15:11)
            let r5 = ((rgb565 >> 11) & 0x1F) as u8;
            let on = r5 > 8; // threshold: ~25% brightness

            let fb_idx = page * OLED_WIDTH as usize + px;
            if on {
                s.fb[fb_idx] |= 1 << bit;
            } else {
                s.fb[fb_idx] &= !(1 << bit);
            }
        }
    }

    ssd1306_send_framebuffer()
}

/// Set display brightness (contrast).  Maps 0-100 percent to 0x00-0xFF.
///
/// # Safety
/// Called from C via the HAL vtable.
pub unsafe extern "C" fn ssd1306_set_brightness(percent: u8) -> i32 {
    if !state().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    let clamped = if percent > 100 { 100u32 } else { percent as u32 };
    let contrast = (clamped * 255 / 100) as u8;
    ssd1306_send_cmd2(SSD1306_CMD_SET_CONTRAST, contrast)
}

/// Enter or exit display sleep.
///
/// `enter = true`:  display off (pixels dark, controller in low-power).
/// `enter = false`: display on (resume from previous contrast / VRAM).
///
/// # Safety
/// Called from C via the HAL vtable.
pub unsafe extern "C" fn ssd1306_sleep(enter: bool) -> i32 {
    if !state().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    let cmd = if enter { SSD1306_CMD_DISPLAY_OFF } else { SSD1306_CMD_DISPLAY_ON };
    state_mut().display_on = !enter;
    ssd1306_send_cmd(cmd)
}

/// Set refresh mode — SSD1306 is always immediate; all modes are accepted.
///
/// # Safety
/// Called from C via the HAL vtable.
pub unsafe extern "C" fn ssd1306_set_refresh_mode(_mode: HalDisplayRefreshMode) -> i32 {
    ESP_OK
}

// ── Driver name ───────────────────────────────────────────────────────────────

static DRIVER_NAME: &[u8] = b"SSD1306 128x64 OLED\0";

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL display driver vtable for the SSD1306 OLED.
///
/// `refresh` is None — the SSD1306 is updated immediately on flush.
static OLED_DRIVER: HalDisplayDriver = HalDisplayDriver {
    init: Some(ssd1306_init),
    deinit: Some(ssd1306_deinit),
    flush: Some(ssd1306_flush),
    refresh: None, // immediate — no deferred refresh
    set_brightness: Some(ssd1306_set_brightness),
    sleep: Some(ssd1306_sleep),
    set_refresh_mode: Some(ssd1306_set_refresh_mode),
    width: OLED_WIDTH,
    height: OLED_HEIGHT,
    display_type: HalDisplayType::Lcd, // treated as LCD (no refresh call)
    name: DRIVER_NAME.as_ptr() as *const c_char,
};

/// Return a pointer to the static SSD1306 HAL display driver vtable.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn drv_oled_ssd1306_get() -> *const HalDisplayDriver {
    &OLED_DRIVER as *const HalDisplayDriver
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal_registry::HalArea;

    fn reset_state() {
        *state_mut() = OledState::new();
    }

    // ── Vtable metadata ───────────────────────────────────────────────────────

    #[test]
    fn test_driver_dimensions() {
        let drv = unsafe { &*drv_oled_ssd1306_get() };
        assert_eq!(drv.width, 128);
        assert_eq!(drv.height, 64);
        assert_eq!(drv.display_type, HalDisplayType::Lcd);
    }

    #[test]
    fn test_driver_refresh_is_none() {
        let drv = unsafe { &*drv_oled_ssd1306_get() };
        assert!(
            drv.refresh.is_none(),
            "SSD1306 must have refresh=None (immediate display, like LCD)"
        );
    }

    #[test]
    fn test_driver_name() {
        let drv = unsafe { &*drv_oled_ssd1306_get() };
        assert!(!drv.name.is_null());
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "SSD1306 128x64 OLED");
    }

    #[test]
    fn test_driver_pointer_stable() {
        let p1 = drv_oled_ssd1306_get();
        let p2 = drv_oled_ssd1306_get();
        assert_eq!(p1, p2);
        assert!(!p1.is_null());
    }

    // ── Init / deinit lifecycle ───────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        reset_state();
        let ret = unsafe { ssd1306_init(std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_init_and_deinit() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        let ret = unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        assert!(state().initialized);
        assert!(state().display_on);

        unsafe { ssd1306_deinit() };
        assert!(!state().initialized);
        assert!(!state().display_on);
        assert!(state().dev.is_null());
    }

    #[test]
    fn test_double_init_is_idempotent() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        let ptr = &cfg as *const OledSsd1306Config as *const c_void;
        assert_eq!(unsafe { ssd1306_init(ptr) }, ESP_OK);
        assert_eq!(unsafe { ssd1306_init(ptr) }, ESP_OK); // no-op
        unsafe { ssd1306_deinit() };
    }

    #[test]
    fn test_deinit_without_init_is_safe() {
        reset_state();
        unsafe { ssd1306_deinit() }; // must not panic
        assert!(!state().initialized);
    }

    // ── Flush ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_flush_before_init_returns_invalid_state() {
        reset_state();
        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let data = vec![0u8; 256];
        let ret = unsafe { ssd1306_flush(&area, data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_flush_null_area_returns_invalid_arg() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        let data = vec![0u8; 256];
        let ret = unsafe { ssd1306_flush(std::ptr::null(), data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { ssd1306_deinit() };
    }

    #[test]
    fn test_flush_null_data_returns_invalid_arg() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 0, y2: 0 };
        let ret = unsafe { ssd1306_flush(&area, std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { ssd1306_deinit() };
    }

    #[test]
    fn test_flush_full_frame_ok() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        // Full 128×64 frame, all white (RGB565 = 0xFFFF → red=31 > 8 → on)
        let area = HalArea { x1: 0, y1: 0, x2: 127, y2: 63 };
        let data = vec![0xFFu8; 128 * 64 * 2];
        let ret = unsafe { ssd1306_flush(&area, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        // All framebuffer bytes should now be 0xFF (all pixels on)
        for &byte in &state().fb {
            assert_eq!(byte, 0xFF, "all pixels should be on for full-white flush");
        }

        unsafe { ssd1306_deinit() };
    }

    #[test]
    fn test_flush_black_frame_clears_fb() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        // First flush white to set all bits
        let area = HalArea { x1: 0, y1: 0, x2: 127, y2: 63 };
        let white = vec![0xFFu8; 128 * 64 * 2];
        unsafe { ssd1306_flush(&area, white.as_ptr()) };

        // Now flush black (RGB565 = 0x0000 → red=0 → off)
        let black = vec![0x00u8; 128 * 64 * 2];
        let ret = unsafe { ssd1306_flush(&area, black.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        for &byte in &state().fb {
            assert_eq!(byte, 0x00, "all pixels should be off for full-black flush");
        }

        unsafe { ssd1306_deinit() };
    }

    #[test]
    fn test_flush_partial_region_updates_fb() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        // Flush a single white pixel at (0, 0) — should set bit 0 of fb[0]
        let area = HalArea { x1: 0, y1: 0, x2: 0, y2: 0 };
        let white_pixel = [0xFFu8, 0xFF]; // RGB565 white
        let ret = unsafe { ssd1306_flush(&area, white_pixel.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        // Page 0, column 0, bit 0 should be set
        assert_eq!(state().fb[0] & 0x01, 0x01, "bit 0 of fb[0] must be set for pixel (0,0)");

        unsafe { ssd1306_deinit() };
    }

    // ── set_brightness ────────────────────────────────────────────────────────

    #[test]
    fn test_set_brightness_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { ssd1306_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_set_brightness_after_init_ok() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };

        assert_eq!(unsafe { ssd1306_set_brightness(50) }, ESP_OK);
        assert_eq!(unsafe { ssd1306_set_brightness(0) }, ESP_OK);
        assert_eq!(unsafe { ssd1306_set_brightness(100) }, ESP_OK);
        assert_eq!(unsafe { ssd1306_set_brightness(200) }, ESP_OK); // clamped

        unsafe { ssd1306_deinit() };
    }

    // ── sleep ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sleep_before_init_returns_invalid_state() {
        reset_state();
        assert_eq!(unsafe { ssd1306_sleep(true) }, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_sleep_enter_and_wake() {
        reset_state();
        let cfg = OledSsd1306Config {
            i2c_bus: 1usize as *mut c_void,
            i2c_addr: 0x3C,
            pin_rst: GPIO_NUM_NC,
            pin_vext: GPIO_NUM_NC,
        };
        unsafe { ssd1306_init(&cfg as *const OledSsd1306Config as *const c_void) };
        assert!(state().display_on);

        assert_eq!(unsafe { ssd1306_sleep(true) }, ESP_OK);
        assert!(!state().display_on);

        assert_eq!(unsafe { ssd1306_sleep(false) }, ESP_OK);
        assert!(state().display_on);

        unsafe { ssd1306_deinit() };
    }

    // ── set_refresh_mode ──────────────────────────────────────────────────────

    #[test]
    fn test_set_refresh_mode_always_ok() {
        for mode in [
            HalDisplayRefreshMode::Full,
            HalDisplayRefreshMode::Partial,
            HalDisplayRefreshMode::Fast,
        ] {
            let ret = unsafe { ssd1306_set_refresh_mode(mode) };
            assert_eq!(ret, ESP_OK);
        }
    }

    // ── Framebuffer geometry ──────────────────────────────────────────────────

    #[test]
    fn test_framebuffer_size_is_1024() {
        assert_eq!(OLED_FB_SIZE, 1024);
    }

    #[test]
    fn test_framebuffer_page_layout() {
        // Page 0 = rows 0..7, page 7 = rows 56..63
        assert_eq!(OLED_PAGES, 8);
        // Page N starts at byte N * 128
        for page in 0..OLED_PAGES {
            let fb_start = page * OLED_WIDTH as usize;
            assert_eq!(fb_start, page * 128);
        }
    }

    // ── Default I2C address ───────────────────────────────────────────────────

    #[test]
    fn test_default_i2c_addr_is_0x3c() {
        reset_state();
        assert_eq!(state().cfg.i2c_addr, 0x3C);
    }
}
