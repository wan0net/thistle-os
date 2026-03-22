// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ST7789 LCD display driver (Rust)
//
// Rust port of components/drv_lcd_st7789/src/drv_lcd_st7789.c.
//
// Wraps ESP-IDF's built-in esp_lcd component (esp_lcd_new_panel_st7789) behind
// the ThistleOS HAL display vtable.  SPI wiring: MOSI, SCK on the shared SPI
// host; CS, DC, RST are passed via LcdSt7789Config.  Backlight is driven via
// LEDC PWM on pin_bl.
//
// The original C driver remains in the tree as a fallback; this version will
// eventually replace it.

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalArea, HalDisplayDriver, HalDisplayRefreshMode, HalDisplayType};

// ── ESP error codes ───────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// ── Panel geometry ────────────────────────────────────────────────────────────

const LCD_WIDTH: u16 = 320;
const LCD_HEIGHT: u16 = 240;

// ── LEDC backlight constants ──────────────────────────────────────────────────

const BL_LEDC_MODE: i32 = 1;     // LEDC_LOW_SPEED_MODE
const BL_LEDC_TIMER: i32 = 0;    // LEDC_TIMER_0
const BL_LEDC_CHANNEL: i32 = 0;  // LEDC_CHANNEL_0
const BL_LEDC_FREQ_HZ: u32 = 5000;
const BL_LEDC_DUTY_RES: u32 = 8; // LEDC_TIMER_8_BIT
const BL_LEDC_MAX_DUTY: u32 = 255;

// GPIO_NUM_NC sentinel (matches ESP-IDF GPIO_NUM_NC = -1)
const GPIO_NUM_NC: i32 = -1;

// ── Configuration struct ──────────────────────────────────────────────────────

/// Configuration for the ST7789 LCD driver.
///
/// The layout must match `lcd_st7789_config_t` from the C header so that a
/// pointer to this struct can be passed directly from C board-init code.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct LcdSt7789Config {
    /// SPI host device (e.g. SPI2_HOST = 1)
    pub spi_host: i32,
    /// GPIO for chip select
    pub pin_cs: i32,
    /// GPIO for data/command
    pub pin_dc: i32,
    /// GPIO for reset (-1 = not connected)
    pub pin_rst: i32,
    /// GPIO for backlight PWM (-1 = no backlight)
    pub pin_bl: i32,
    /// SPI clock speed in Hz (0 = use default 40 MHz)
    pub spi_clock_hz: i32,
}

impl Default for LcdSt7789Config {
    fn default() -> Self {
        LcdSt7789Config {
            spi_host: 1,
            pin_cs: -1,
            pin_dc: -1,
            pin_rst: -1,
            pin_bl: -1,
            spi_clock_hz: 40_000_000,
        }
    }
}

// ── Driver state ──────────────────────────────────────────────────────────────

struct LcdState {
    cfg: LcdSt7789Config,
    io: *mut c_void,       // esp_lcd_panel_io_handle_t
    panel: *mut c_void,    // esp_lcd_panel_handle_t
    initialized: bool,
    brightness: u8,        // last non-zero brightness (for sleep/wake)
}

// SAFETY: Only mutated during single-threaded board init / driver calls.
unsafe impl Send for LcdState {}
unsafe impl Sync for LcdState {}

impl LcdState {
    const fn new() -> Self {
        LcdState {
            cfg: LcdSt7789Config {
                spi_host: 0,
                pin_cs: 0,
                pin_dc: 0,
                pin_rst: -1,
                pin_bl: -1,
                spi_clock_hz: 0,
            },
            io: std::ptr::null_mut(),
            panel: std::ptr::null_mut(),
            initialized: false,
            brightness: 0,
        }
    }
}

struct GlobalLcdState {
    inner: UnsafeCell<LcdState>,
}

// SAFETY: Only mutated during single-threaded board init.
unsafe impl Sync for GlobalLcdState {}

static STATE: GlobalLcdState = GlobalLcdState {
    inner: UnsafeCell::new(LcdState::new()),
};

#[inline]
fn state() -> &'static LcdState {
    unsafe { &*STATE.inner.get() }
}

#[inline]
fn state_mut() -> &'static mut LcdState {
    unsafe { &mut *STATE.inner.get() }
}

// ── ESP-IDF platform bindings ─────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod platform {
    use std::os::raw::c_void;

    // ledc_timer_config_t layout (matches ESP-IDF)
    #[repr(C)]
    pub struct LedcTimerConfig {
        pub speed_mode: i32,
        pub timer_num: i32,
        pub duty_resolution: u32,
        pub freq_hz: u32,
        pub clk_cfg: u32, // LEDC_AUTO_CLK = 0
    }

    // ledc_channel_config_t layout (matches ESP-IDF)
    #[repr(C)]
    pub struct LedcChannelConfig {
        pub gpio_num: i32,
        pub speed_mode: i32,
        pub channel: i32,
        pub intr_type: u32, // LEDC_INTR_DISABLE = 0
        pub timer_sel: i32,
        pub duty: u32,
        pub hpoint: i32,
        pub flags: u32,
    }

    // esp_lcd_panel_io_spi_config_t layout (matches ESP-IDF)
    #[repr(C)]
    pub struct EspLcdPanelIoSpiConfig {
        pub dc_gpio_num: i32,
        pub cs_gpio_num: i32,
        pub pclk_hz: u32,
        pub lcd_cmd_bits: i32,
        pub lcd_param_bits: i32,
        pub spi_mode: u32,
        pub trans_queue_depth: usize,
        pub on_color_trans_done: *const c_void,
        pub user_ctx: *mut c_void,
        pub flags: u32,
    }

    // esp_lcd_panel_dev_config_t layout (matches ESP-IDF)
    // LCD_RGB_ELEMENT_ORDER_RGB = 0, bits_per_pixel = 16
    #[repr(C)]
    pub struct EspLcdPanelDevConfig {
        pub reset_gpio_num: i32,
        pub rgb_ele_order: u32, // LCD_RGB_ELEMENT_ORDER_RGB = 0
        pub data_endian: u32,   // LCD_RGB_DATA_ENDIAN_BIG = 0
        pub bits_per_pixel: u32,
        pub flags: u32,
        pub vendor_config: *mut c_void,
    }

    extern "C" {
        // esp_lcd
        pub fn esp_lcd_new_panel_io_spi(
            bus: *mut c_void,
            cfg: *const EspLcdPanelIoSpiConfig,
            io: *mut *mut c_void,
        ) -> i32;
        pub fn esp_lcd_new_panel_st7789(
            io: *mut c_void,
            cfg: *const EspLcdPanelDevConfig,
            panel: *mut *mut c_void,
        ) -> i32;
        pub fn esp_lcd_panel_reset(panel: *mut c_void) -> i32;
        pub fn esp_lcd_panel_init(panel: *mut c_void) -> i32;
        pub fn esp_lcd_panel_invert_color(panel: *mut c_void, invert: bool) -> i32;
        pub fn esp_lcd_panel_disp_on_off(panel: *mut c_void, on: bool) -> i32;
        pub fn esp_lcd_panel_draw_bitmap(
            panel: *mut c_void,
            x1: i32,
            y1: i32,
            x2: i32,
            y2: i32,
            data: *const u8,
        ) -> i32;
        pub fn esp_lcd_panel_del(panel: *mut c_void) -> i32;
        pub fn esp_lcd_panel_io_del(io: *mut c_void) -> i32;

        // LEDC
        pub fn ledc_timer_config(cfg: *const LedcTimerConfig) -> i32;
        pub fn ledc_channel_config(cfg: *const LedcChannelConfig) -> i32;
        pub fn ledc_set_duty(mode: i32, channel: i32, duty: u32) -> i32;
        pub fn ledc_update_duty(mode: i32, channel: i32) -> i32;
    }
}

// ── Backlight helpers ─────────────────────────────────────────────────────────

/// Initialise the LEDC timer and channel for backlight PWM.
/// No-op if pin_bl == GPIO_NUM_NC.
unsafe fn bl_init(pin_bl: i32) -> i32 {
    if pin_bl == GPIO_NUM_NC {
        return ESP_OK;
    }

    #[cfg(target_os = "espidf")]
    {
        let timer_cfg = platform::LedcTimerConfig {
            speed_mode: BL_LEDC_MODE,
            timer_num: BL_LEDC_TIMER,
            duty_resolution: BL_LEDC_DUTY_RES,
            freq_hz: BL_LEDC_FREQ_HZ,
            clk_cfg: 0, // LEDC_AUTO_CLK
        };
        let ret = platform::ledc_timer_config(&timer_cfg);
        if ret != ESP_OK {
            return ret;
        }

        let ch_cfg = platform::LedcChannelConfig {
            gpio_num: pin_bl,
            speed_mode: BL_LEDC_MODE,
            channel: BL_LEDC_CHANNEL,
            intr_type: 0, // LEDC_INTR_DISABLE
            timer_sel: BL_LEDC_TIMER,
            duty: 0,
            hpoint: 0,
            flags: 0,
        };
        let ret = platform::ledc_channel_config(&ch_cfg);
        if ret != ESP_OK {
            return ret;
        }
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = pin_bl;
    }

    ESP_OK
}

/// Set the backlight duty cycle (percent 0-100).
/// No-op if pin_bl == GPIO_NUM_NC.
unsafe fn bl_set_duty(pin_bl: i32, percent: u8) -> i32 {
    if pin_bl == GPIO_NUM_NC {
        return ESP_OK;
    }

    let duty = (percent as u32) * BL_LEDC_MAX_DUTY / 100;

    #[cfg(target_os = "espidf")]
    {
        let ret = platform::ledc_set_duty(BL_LEDC_MODE, BL_LEDC_CHANNEL, duty);
        if ret != ESP_OK {
            return ret;
        }
        return platform::ledc_update_duty(BL_LEDC_MODE, BL_LEDC_CHANNEL);
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = duty;
        ESP_OK
    }
}

// ── esp_lcd wrappers ──────────────────────────────────────────────────────────

/// Create SPI panel IO handle.
unsafe fn lcd_new_panel_io_spi(
    spi_host: i32,
    pin_dc: i32,
    pin_cs: i32,
    clock_hz: u32,
    io_out: *mut *mut c_void,
) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let io_cfg = platform::EspLcdPanelIoSpiConfig {
            dc_gpio_num: pin_dc,
            cs_gpio_num: pin_cs,
            pclk_hz: clock_hz,
            lcd_cmd_bits: 8,
            lcd_param_bits: 8,
            spi_mode: 0,
            trans_queue_depth: 10,
            on_color_trans_done: std::ptr::null(),
            user_ctx: std::ptr::null_mut(),
            flags: 0,
        };
        return platform::esp_lcd_new_panel_io_spi(spi_host as *mut c_void, &io_cfg, io_out);
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (spi_host, pin_dc, pin_cs, clock_hz);
        // Simulator: return a dummy non-null handle
        *io_out = 1usize as *mut c_void;
        ESP_OK
    }
}

/// Create ST7789 panel handle.
unsafe fn lcd_new_panel_st7789(
    io: *mut c_void,
    pin_rst: i32,
    panel_out: *mut *mut c_void,
) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let panel_cfg = platform::EspLcdPanelDevConfig {
            reset_gpio_num: pin_rst,
            rgb_ele_order: 0, // LCD_RGB_ELEMENT_ORDER_RGB
            data_endian: 0,
            bits_per_pixel: 16,
            flags: 0,
            vendor_config: std::ptr::null_mut(),
        };
        return platform::esp_lcd_new_panel_st7789(io, &panel_cfg, panel_out);
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (io, pin_rst);
        // Simulator: return a dummy non-null handle
        *panel_out = 2usize as *mut c_void;
        ESP_OK
    }
}

unsafe fn lcd_panel_reset(panel: *mut c_void) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_reset(panel); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = panel; ESP_OK }
}

unsafe fn lcd_panel_init(panel: *mut c_void) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_init(panel); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = panel; ESP_OK }
}

unsafe fn lcd_panel_invert_color(panel: *mut c_void, invert: bool) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_invert_color(panel, invert); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = (panel, invert); ESP_OK }
}

unsafe fn lcd_panel_disp_on_off(panel: *mut c_void, on: bool) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_disp_on_off(panel, on); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = (panel, on); ESP_OK }
}

unsafe fn lcd_panel_draw_bitmap(
    panel: *mut c_void,
    x1: i32, y1: i32, x2: i32, y2: i32,
    data: *const u8,
) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_draw_bitmap(panel, x1, y1, x2, y2, data); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = (panel, x1, y1, x2, y2, data); ESP_OK }
}

unsafe fn lcd_panel_del(panel: *mut c_void) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_del(panel); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = panel; ESP_OK }
}

unsafe fn lcd_panel_io_del(io: *mut c_void) -> i32 {
    #[cfg(target_os = "espidf")]
    { return platform::esp_lcd_panel_io_del(io); }
    #[cfg(not(target_os = "espidf"))]
    { let _ = io; ESP_OK }
}

// ── Driver vtable functions ───────────────────────────────────────────────────

/// Initialise the ST7789 LCD.
///
/// `config` must point to an `LcdSt7789Config`-compatible struct.
pub unsafe extern "C" fn st7789_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let s = state_mut();
    if s.initialized {
        return ESP_OK; // already initialised
    }

    // Copy config
    let cfg = &*(config as *const LcdSt7789Config);
    s.cfg = *cfg;

    // Backlight init (off until display is ready)
    let ret = bl_init(s.cfg.pin_bl);
    if ret != ESP_OK {
        return ret;
    }

    // Determine SPI clock
    let clock_hz = if s.cfg.spi_clock_hz > 0 {
        s.cfg.spi_clock_hz as u32
    } else {
        40_000_000
    };

    // Create SPI panel IO handle
    let mut io: *mut c_void = std::ptr::null_mut();
    let ret = lcd_new_panel_io_spi(
        s.cfg.spi_host,
        s.cfg.pin_dc,
        s.cfg.pin_cs,
        clock_hz,
        &mut io,
    );
    if ret != ESP_OK {
        return ret;
    }
    s.io = io;

    // Create ST7789 panel handle
    let mut panel: *mut c_void = std::ptr::null_mut();
    let ret = lcd_new_panel_st7789(s.io, s.cfg.pin_rst, &mut panel);
    if ret != ESP_OK {
        lcd_panel_io_del(s.io);
        s.io = std::ptr::null_mut();
        return ret;
    }
    s.panel = panel;

    // Initialize panel sequence
    lcd_panel_reset(s.panel);
    lcd_panel_init(s.panel);

    // Most ST7789 TFT panels require colour inversion for correct colours
    lcd_panel_invert_color(s.panel, true);

    lcd_panel_disp_on_off(s.panel, true);

    // Turn backlight on at full brightness
    let ret = bl_set_duty(s.cfg.pin_bl, 100);
    if ret != ESP_OK {
        lcd_panel_del(s.panel);
        lcd_panel_io_del(s.io);
        s.panel = std::ptr::null_mut();
        s.io = std::ptr::null_mut();
        return ret;
    }

    s.initialized = true;
    s.brightness = 100;
    ESP_OK
}

/// De-initialise the ST7789 LCD.
pub unsafe extern "C" fn st7789_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }

    bl_set_duty(s.cfg.pin_bl, 0);

    lcd_panel_disp_on_off(s.panel, false);
    lcd_panel_del(s.panel);
    lcd_panel_io_del(s.io);

    s.panel = std::ptr::null_mut();
    s.io = std::ptr::null_mut();
    s.initialized = false;
}

/// Flush a rectangular region of pixel data to the display.
///
/// `area` defines the region; `color_data` is RGB565 pixel data.
/// esp_lcd_panel_draw_bitmap uses exclusive end coordinates, so we pass
/// area.x2 + 1 and area.y2 + 1.
pub unsafe extern "C" fn st7789_flush(area: *const HalArea, color_data: *const u8) -> i32 {
    let s = state();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if area.is_null() || color_data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let area = &*area;
    lcd_panel_draw_bitmap(
        s.panel,
        area.x1 as i32,
        area.y1 as i32,
        area.x2 as i32 + 1,
        area.y2 as i32 + 1,
        color_data,
    )
}

/// Set backlight brightness (0–100 percent).
pub unsafe extern "C" fn st7789_set_brightness(percent: u8) -> i32 {
    let s = state_mut();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if s.cfg.pin_bl == GPIO_NUM_NC {
        return ESP_ERR_NOT_SUPPORTED;
    }

    let clamped = if percent > 100 { 100 } else { percent };
    let ret = bl_set_duty(s.cfg.pin_bl, clamped);
    if ret == ESP_OK && clamped > 0 {
        s.brightness = clamped;
    }
    ret
}

/// Enter or exit display sleep.
///
/// `enter = true`:  display off + backlight off.
/// `enter = false`: display on + restore backlight.
pub unsafe extern "C" fn st7789_sleep(enter: bool) -> i32 {
    let s = state();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if enter {
        lcd_panel_disp_on_off(s.panel, false);
        if s.cfg.pin_bl != GPIO_NUM_NC {
            bl_set_duty(s.cfg.pin_bl, 0);
        }
    } else {
        lcd_panel_disp_on_off(s.panel, true);
        if s.cfg.pin_bl != GPIO_NUM_NC {
            bl_set_duty(s.cfg.pin_bl, s.brightness);
        }
    }
    ESP_OK
}

/// Set refresh mode — LCD has no LUT-based modes; all modes accepted.
pub unsafe extern "C" fn st7789_set_refresh_mode(_mode: HalDisplayRefreshMode) -> i32 {
    ESP_OK
}

// ── Driver name ───────────────────────────────────────────────────────────────

static DRIVER_NAME: &[u8] = b"ST7789 (esp_lcd)\0";

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL display driver vtable for the ST7789 LCD.
///
/// `refresh` is None — LCD pushes pixels immediately; no deferred refresh
/// is needed.  This is the signal used by tk_wm to distinguish LCD from
/// e-paper.
static LCD_DRIVER: HalDisplayDriver = HalDisplayDriver {
    init: Some(st7789_init),
    deinit: Some(st7789_deinit),
    flush: Some(st7789_flush),
    refresh: None, // LCD: no deferred refresh needed
    set_brightness: Some(st7789_set_brightness),
    sleep: Some(st7789_sleep),
    set_refresh_mode: Some(st7789_set_refresh_mode),
    width: LCD_WIDTH,
    height: LCD_HEIGHT,
    display_type: HalDisplayType::Lcd,
    name: DRIVER_NAME.as_ptr() as *const c_char,
};

/// Return a pointer to the static ST7789 HAL display driver vtable.
///
/// Drop-in replacement for the C `drv_lcd_st7789_get()`.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn drv_lcd_st7789_get() -> *const HalDisplayDriver {
    &LCD_DRIVER as *const HalDisplayDriver
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal_registry::HalArea;

    /// Reset driver state between tests.
    fn reset_state() {
        *state_mut() = LcdState::new();
    }

    // ── Vtable metadata ───────────────────────────────────────────────────────

    #[test]
    fn test_driver_dimensions() {
        let drv = unsafe { &*drv_lcd_st7789_get() };
        assert_eq!(drv.width, 320);
        assert_eq!(drv.height, 240);
        assert_eq!(drv.display_type, HalDisplayType::Lcd);
    }

    #[test]
    fn test_driver_refresh_is_none() {
        let drv = unsafe { &*drv_lcd_st7789_get() };
        assert!(
            drv.refresh.is_none(),
            "LCD driver must have refresh=None to distinguish it from e-paper"
        );
    }

    #[test]
    fn test_driver_name() {
        let drv = unsafe { &*drv_lcd_st7789_get() };
        assert!(!drv.name.is_null());
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "ST7789 (esp_lcd)");
    }

    #[test]
    fn test_driver_pointer_stable() {
        let p1 = drv_lcd_st7789_get();
        let p2 = drv_lcd_st7789_get();
        assert_eq!(p1, p2);
        assert!(!p1.is_null());
    }

    // ── Init / deinit lifecycle ───────────────────────────────────────────────

    #[test]
    fn test_init_null_config() {
        reset_state();
        let ret = unsafe { st7789_init(std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_init_and_deinit() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        let ret = unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        assert!(state().initialized);
        assert_eq!(state().brightness, 100);

        unsafe { st7789_deinit() };
        assert!(!state().initialized);
        assert!(state().io.is_null());
        assert!(state().panel.is_null());
    }

    #[test]
    fn test_double_init_is_idempotent() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        let ptr = &cfg as *const LcdSt7789Config as *const c_void;
        let ret1 = unsafe { st7789_init(ptr) };
        let ret2 = unsafe { st7789_init(ptr) };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK); // second call is a no-op

        unsafe { st7789_deinit() };
    }

    #[test]
    fn test_deinit_without_init_is_safe() {
        reset_state();
        // Should not panic or crash
        unsafe { st7789_deinit() };
    }

    // ── Flush ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_flush_before_init_returns_invalid_state() {
        reset_state();
        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let data = vec![0u8; 256];
        let ret = unsafe { st7789_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_flush_null_area() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        let data = vec![0u8; 256];
        let ret = unsafe { st7789_flush(std::ptr::null(), data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { st7789_deinit() };
    }

    #[test]
    fn test_flush_null_data() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let ret = unsafe { st7789_flush(&area as *const HalArea, std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { st7789_deinit() };
    }

    #[test]
    fn test_flush_succeeds_after_init() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 63, y2: 63 };
        // 64×64 pixels × 2 bytes/pixel (RGB565)
        let data = vec![0u8; 64 * 64 * 2];
        let ret = unsafe { st7789_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        unsafe { st7789_deinit() };
    }

    // ── set_brightness ────────────────────────────────────────────────────────

    #[test]
    fn test_set_brightness_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { st7789_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_set_brightness_no_backlight_pin_returns_not_supported() {
        reset_state();
        let cfg = LcdSt7789Config {
            pin_bl: GPIO_NUM_NC,
            ..LcdSt7789Config::default()
        };
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        let ret = unsafe { st7789_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_NOT_SUPPORTED);

        unsafe { st7789_deinit() };
    }

    #[test]
    fn test_set_brightness_stores_nonzero_value() {
        reset_state();
        // Use a positive pin_bl so the code path reaches bl_set_duty
        let cfg = LcdSt7789Config {
            pin_bl: 10,
            ..LcdSt7789Config::default()
        };
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };
        assert_eq!(state().brightness, 100); // set to 100 on init

        let ret = unsafe { st7789_set_brightness(75) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 75);

        // Setting to 0 should not update brightness field
        let ret = unsafe { st7789_set_brightness(0) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 75, "brightness field must not change on zero");

        unsafe { st7789_deinit() };
    }

    #[test]
    fn test_set_brightness_clamped_to_100() {
        reset_state();
        let cfg = LcdSt7789Config {
            pin_bl: 10,
            ..LcdSt7789Config::default()
        };
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        // 200 > 100, should clamp and still return OK
        let ret = unsafe { st7789_set_brightness(200) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 100);

        unsafe { st7789_deinit() };
    }

    // ── sleep ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sleep_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { st7789_sleep(true) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_sleep_enter_and_wake() {
        reset_state();
        let cfg = LcdSt7789Config::default();
        unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };

        let ret = unsafe { st7789_sleep(true) };
        assert_eq!(ret, ESP_OK);

        let ret = unsafe { st7789_sleep(false) };
        assert_eq!(ret, ESP_OK);

        unsafe { st7789_deinit() };
    }

    // ── set_refresh_mode ──────────────────────────────────────────────────────

    #[test]
    fn test_set_refresh_mode_always_ok() {
        // set_refresh_mode is a no-op for LCD; always returns ESP_OK regardless
        // of initialisation state.
        for mode in [
            HalDisplayRefreshMode::Full,
            HalDisplayRefreshMode::Partial,
            HalDisplayRefreshMode::Fast,
        ] {
            let ret = unsafe { st7789_set_refresh_mode(mode) };
            assert_eq!(ret, ESP_OK);
        }
    }

    // ── SPI clock default ─────────────────────────────────────────────────────

    #[test]
    fn test_spi_clock_zero_defaults_to_40mhz() {
        reset_state();
        let cfg = LcdSt7789Config {
            spi_clock_hz: 0,
            ..LcdSt7789Config::default()
        };
        // Init should succeed; on the simulator the clock value is unused but
        // the logic must not fail.
        let ret = unsafe { st7789_init(&cfg as *const LcdSt7789Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        unsafe { st7789_deinit() };
    }
}
