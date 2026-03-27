// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — ILI9341 LCD display driver (Rust)
//
// Wraps ESP-IDF's esp_lcd component behind the ThistleOS HAL display vtable.
// SPI wiring: MOSI, SCK on the shared SPI host; CS, DC, RST are passed via
// LcdIli9341Config.  Backlight is driven via LEDC PWM on pin_bl.
//
// The ILI9341 is very similar to the ST7789 — both are SPI TFT LCD panels.
// Key differences: ILI9341 has its own init command sequence, does NOT require
// color inversion (ST7789 does), and uses ILI9341-specific power control,
// gamma, and timing commands.

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalArea, HalDisplayDriver, HalDisplayRefreshMode, HalDisplayType};

// -- ESP error codes ----------------------------------------------------------

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// -- Panel geometry -----------------------------------------------------------

const LCD_WIDTH: u16 = 320;
const LCD_HEIGHT: u16 = 240;

// -- LEDC backlight constants -------------------------------------------------

const BL_LEDC_MODE: i32 = 1;     // LEDC_LOW_SPEED_MODE
const BL_LEDC_TIMER: i32 = 0;    // LEDC_TIMER_0
const BL_LEDC_CHANNEL: i32 = 0;  // LEDC_CHANNEL_0
const BL_LEDC_FREQ_HZ: u32 = 5000;
const BL_LEDC_DUTY_RES: u32 = 8; // LEDC_TIMER_8_BIT
const BL_LEDC_MAX_DUTY: u32 = 255;

// GPIO_NUM_NC sentinel (matches ESP-IDF GPIO_NUM_NC = -1)
const GPIO_NUM_NC: i32 = -1;

// -- ILI9341 command constants ------------------------------------------------

const ILI9341_CMD_POWER_CTRL_B: u8     = 0xCF;
const ILI9341_CMD_POWER_ON_SEQ: u8     = 0xED;
const ILI9341_CMD_TIMING_CTRL_A: u8    = 0xE8;
const ILI9341_CMD_POWER_CTRL_A: u8     = 0xCB;
const ILI9341_CMD_PUMP_RATIO: u8       = 0xF7;
const ILI9341_CMD_TIMING_CTRL_B: u8    = 0xEA;
const ILI9341_CMD_POWER_CTRL1: u8      = 0xC0;
const ILI9341_CMD_POWER_CTRL2: u8      = 0xC1;
const ILI9341_CMD_VCOM_CTRL1: u8       = 0xC5;
const ILI9341_CMD_VCOM_CTRL2: u8       = 0xC7;
const ILI9341_CMD_MEM_ACCESS: u8       = 0x36;
const ILI9341_CMD_PIXEL_FMT: u8        = 0x3A;
const ILI9341_CMD_FRAME_RATE: u8       = 0xB1;
const ILI9341_CMD_DISP_FUNC: u8        = 0xB6;
const ILI9341_CMD_3GAMMA_DIS: u8       = 0xF2;
const ILI9341_CMD_GAMMA_SET: u8        = 0x26;
const ILI9341_CMD_POS_GAMMA: u8        = 0xE0;
const ILI9341_CMD_NEG_GAMMA: u8        = 0xE1;
const ILI9341_CMD_SLEEP_IN: u8         = 0x10;
const ILI9341_CMD_SLEEP_OUT: u8        = 0x11;
const ILI9341_CMD_DISPLAY_OFF: u8      = 0x28;
const ILI9341_CMD_DISPLAY_ON: u8       = 0x29;
const ILI9341_CMD_ENTRY_MODE: u8       = 0xB7;

// -- Configuration struct -----------------------------------------------------

/// Configuration for the ILI9341 LCD driver.
///
/// The layout must match `lcd_ili9341_config_t` from the C header so that a
/// pointer to this struct can be passed directly from C board-init code.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct LcdIli9341Config {
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
    /// SPI clock speed in Hz (0 = use default 26 MHz)
    pub spi_clock_hz: i32,
}

impl Default for LcdIli9341Config {
    fn default() -> Self {
        LcdIli9341Config {
            spi_host: 1,
            pin_cs: -1,
            pin_dc: -1,
            pin_rst: -1,
            pin_bl: -1,
            spi_clock_hz: 26_000_000,
        }
    }
}

// -- Driver state -------------------------------------------------------------

struct LcdState {
    cfg: LcdIli9341Config,
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
            cfg: LcdIli9341Config {
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

// -- ESP-IDF platform bindings ------------------------------------------------

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
        pub fn esp_lcd_panel_io_tx_param(
            io: *mut c_void,
            lcd_cmd: i32,
            param: *const u8,
            param_size: usize,
        ) -> i32;

        // LEDC
        pub fn ledc_timer_config(cfg: *const LedcTimerConfig) -> i32;
        pub fn ledc_channel_config(cfg: *const LedcChannelConfig) -> i32;
        pub fn ledc_set_duty(mode: i32, channel: i32, duty: u32) -> i32;
        pub fn ledc_update_duty(mode: i32, channel: i32) -> i32;

        // FreeRTOS
        pub fn vTaskDelay(ticks: u32);
    }
}

// -- Backlight helpers --------------------------------------------------------

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

// -- esp_lcd wrappers ---------------------------------------------------------

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

/// Create panel handle using the ST7789 generic driver (works for ILI9341 with
/// custom init commands sent separately via panel_io_tx_param).
unsafe fn lcd_new_panel(
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

/// Send a command with parameter bytes to the LCD controller via SPI IO.
unsafe fn lcd_io_tx_param(io: *mut c_void, cmd: u8, params: &[u8]) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        return platform::esp_lcd_panel_io_tx_param(
            io,
            cmd as i32,
            params.as_ptr(),
            params.len(),
        );
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (io, cmd, params);
        ESP_OK
    }
}

/// Delay for approximately `ms` milliseconds.
/// On ESP-IDF, uses vTaskDelay (assumes configTICK_RATE_HZ = 100, so 1 tick = 10ms).
/// On simulator, this is a no-op.
#[inline]
unsafe fn delay_ms(ms: u32) {
    #[cfg(target_os = "espidf")]
    {
        // Round up: (ms + 9) / 10 gives ceiling ticks at 100 Hz
        let ticks = (ms + 9) / 10;
        platform::vTaskDelay(ticks);
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = ms;
    }
}

// -- ILI9341 init command sequence --------------------------------------------

/// Send the ILI9341-specific initialization commands via SPI IO.
///
/// This configures power control, VCOM, gamma correction, pixel format,
/// frame rate, and enables the display.  Unlike ST7789, the ILI9341 does
/// NOT require color inversion.
unsafe fn ili9341_send_init_commands(io: *mut c_void) -> i32 {
    // Power control B
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POWER_CTRL_B, &[0x00, 0xAA, 0xE0]);
    if ret != ESP_OK { return ret; }

    // Power on sequence control
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POWER_ON_SEQ, &[0x67, 0x03, 0x12, 0x81]);
    if ret != ESP_OK { return ret; }

    // Driver timing control A
    let ret = lcd_io_tx_param(io, ILI9341_CMD_TIMING_CTRL_A, &[0x85, 0x00, 0x78]);
    if ret != ESP_OK { return ret; }

    // Power control A
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POWER_CTRL_A, &[0x39, 0x2C, 0x00, 0x34, 0x02]);
    if ret != ESP_OK { return ret; }

    // Pump ratio control
    let ret = lcd_io_tx_param(io, ILI9341_CMD_PUMP_RATIO, &[0x20]);
    if ret != ESP_OK { return ret; }

    // Driver timing control B
    let ret = lcd_io_tx_param(io, ILI9341_CMD_TIMING_CTRL_B, &[0x00, 0x00]);
    if ret != ESP_OK { return ret; }

    // Power control 1
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POWER_CTRL1, &[0x23]);
    if ret != ESP_OK { return ret; }

    // Power control 2
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POWER_CTRL2, &[0x11]);
    if ret != ESP_OK { return ret; }

    // VCOM control 1
    let ret = lcd_io_tx_param(io, ILI9341_CMD_VCOM_CTRL1, &[0x43, 0x4C]);
    if ret != ESP_OK { return ret; }

    // VCOM control 2
    let ret = lcd_io_tx_param(io, ILI9341_CMD_VCOM_CTRL2, &[0x86]);
    if ret != ESP_OK { return ret; }

    // Memory access control (landscape: row/col exchange + row addr order)
    let ret = lcd_io_tx_param(io, ILI9341_CMD_MEM_ACCESS, &[0x48]);
    if ret != ESP_OK { return ret; }

    // Pixel format: 16-bit RGB565
    let ret = lcd_io_tx_param(io, ILI9341_CMD_PIXEL_FMT, &[0x55]);
    if ret != ESP_OK { return ret; }

    // Frame rate control: 70 Hz
    let ret = lcd_io_tx_param(io, ILI9341_CMD_FRAME_RATE, &[0x00, 0x1B]);
    if ret != ESP_OK { return ret; }

    // Display function control
    let ret = lcd_io_tx_param(io, ILI9341_CMD_DISP_FUNC, &[0x08, 0x82, 0x27]);
    if ret != ESP_OK { return ret; }

    // Entry mode set
    let ret = lcd_io_tx_param(io, ILI9341_CMD_ENTRY_MODE, &[0x07]);
    if ret != ESP_OK { return ret; }

    // 3Gamma function disable
    let ret = lcd_io_tx_param(io, ILI9341_CMD_3GAMMA_DIS, &[0x00]);
    if ret != ESP_OK { return ret; }

    // Gamma curve selected
    let ret = lcd_io_tx_param(io, ILI9341_CMD_GAMMA_SET, &[0x01]);
    if ret != ESP_OK { return ret; }

    // Positive gamma correction (Espressif ILI9341 reference values)
    let ret = lcd_io_tx_param(io, ILI9341_CMD_POS_GAMMA, &[
        0x1F, 0x36, 0x36, 0x3A, 0x0C, 0x05, 0x4F, 0x87,
        0x3C, 0x08, 0x11, 0x35, 0x19, 0x13, 0x00,
    ]);
    if ret != ESP_OK { return ret; }

    // Negative gamma correction (Espressif ILI9341 reference values)
    let ret = lcd_io_tx_param(io, ILI9341_CMD_NEG_GAMMA, &[
        0x00, 0x2C, 0x2E, 0x3F, 0x0F, 0x04, 0x51, 0x76,
        0x43, 0x09, 0x12, 0x3B, 0x25, 0x22, 0x00,
    ]);
    if ret != ESP_OK { return ret; }

    // Sleep out
    let ret = lcd_io_tx_param(io, ILI9341_CMD_SLEEP_OUT, &[]);
    if ret != ESP_OK { return ret; }

    // ILI9341 datasheet requires >= 120ms delay after Sleep Out before Display On
    delay_ms(120);

    // Display on
    lcd_io_tx_param(io, ILI9341_CMD_DISPLAY_ON, &[])
}

// -- Driver vtable functions --------------------------------------------------

/// Initialise the ILI9341 LCD.
///
/// `config` must point to an `LcdIli9341Config`-compatible struct.
pub unsafe extern "C" fn ili9341_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let s = state_mut();
    if s.initialized {
        return ESP_OK; // already initialised
    }

    // Copy config
    let cfg = &*(config as *const LcdIli9341Config);
    s.cfg = *cfg;

    // Backlight init (off until display is ready)
    let ret = bl_init(s.cfg.pin_bl);
    if ret != ESP_OK {
        return ret;
    }

    // Determine SPI clock (ILI9341 default: 26 MHz)
    let clock_hz = if s.cfg.spi_clock_hz > 0 {
        s.cfg.spi_clock_hz as u32
    } else {
        26_000_000
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

    // Create panel handle (using generic esp_lcd panel driver)
    let mut panel: *mut c_void = std::ptr::null_mut();
    let ret = lcd_new_panel(s.io, s.cfg.pin_rst, &mut panel);
    if ret != ESP_OK {
        lcd_panel_io_del(s.io);
        s.io = std::ptr::null_mut();
        return ret;
    }
    s.panel = panel;

    // Hardware reset (toggles RST pin via esp_lcd)
    let ret = lcd_panel_reset(s.panel);
    if ret != ESP_OK {
        lcd_panel_del(s.panel);
        lcd_panel_io_del(s.io);
        s.panel = std::ptr::null_mut();
        s.io = std::ptr::null_mut();
        return ret;
    }

    // Send ILI9341-specific initialization commands.
    // We intentionally skip lcd_panel_init() — that would run the ST7789
    // built-in init sequence which overwrites our ILI9341-specific commands.
    // Our custom init commands handle everything (power, gamma, timing, sleep
    // out, display on).
    let ret = ili9341_send_init_commands(s.io);
    if ret != ESP_OK {
        lcd_panel_del(s.panel);
        lcd_panel_io_del(s.io);
        s.panel = std::ptr::null_mut();
        s.io = std::ptr::null_mut();
        return ret;
    }

    // ILI9341 does NOT need color inversion (unlike ST7789)

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

/// De-initialise the ILI9341 LCD.
pub unsafe extern "C" fn ili9341_deinit() {
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
pub unsafe extern "C" fn ili9341_flush(area: *const HalArea, color_data: *const u8) -> i32 {
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

/// Set backlight brightness (0-100 percent).
pub unsafe extern "C" fn ili9341_set_brightness(percent: u8) -> i32 {
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
/// `enter = true`:  Display Off (0x28) + Sleep In (0x10) + backlight off.
/// `enter = false`: Sleep Out (0x11) + 120ms delay + Display On (0x29) + restore backlight.
pub unsafe extern "C" fn ili9341_sleep(enter: bool) -> i32 {
    let s = state();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if enter {
        // Display Off, then Sleep In
        let ret = lcd_io_tx_param(s.io, ILI9341_CMD_DISPLAY_OFF, &[]);
        if ret != ESP_OK { return ret; }
        let ret = lcd_io_tx_param(s.io, ILI9341_CMD_SLEEP_IN, &[]);
        if ret != ESP_OK { return ret; }
        if s.cfg.pin_bl != GPIO_NUM_NC {
            bl_set_duty(s.cfg.pin_bl, 0);
        }
    } else {
        // Sleep Out, wait 120ms, then Display On
        let ret = lcd_io_tx_param(s.io, ILI9341_CMD_SLEEP_OUT, &[]);
        if ret != ESP_OK { return ret; }
        delay_ms(120);
        let ret = lcd_io_tx_param(s.io, ILI9341_CMD_DISPLAY_ON, &[]);
        if ret != ESP_OK { return ret; }
        if s.cfg.pin_bl != GPIO_NUM_NC {
            bl_set_duty(s.cfg.pin_bl, s.brightness);
        }
    }
    ESP_OK
}

/// Set refresh mode -- LCD has no LUT-based modes; all modes accepted.
pub unsafe extern "C" fn ili9341_set_refresh_mode(_mode: HalDisplayRefreshMode) -> i32 {
    ESP_OK
}

// -- Driver name --------------------------------------------------------------

static DRIVER_NAME: &[u8] = b"ILI9341 (esp_lcd)\0";

// -- HAL vtable ---------------------------------------------------------------

/// Static HAL display driver vtable for the ILI9341 LCD.
///
/// `refresh` is None -- LCD pushes pixels immediately; no deferred refresh
/// is needed.  This is the signal used by tk_wm to distinguish LCD from
/// e-paper.
static LCD_DRIVER: HalDisplayDriver = HalDisplayDriver {
    init: Some(ili9341_init),
    deinit: Some(ili9341_deinit),
    flush: Some(ili9341_flush),
    refresh: None, // LCD: no deferred refresh needed
    set_brightness: Some(ili9341_set_brightness),
    sleep: Some(ili9341_sleep),
    set_refresh_mode: Some(ili9341_set_refresh_mode),
    width: LCD_WIDTH,
    height: LCD_HEIGHT,
    display_type: HalDisplayType::Lcd,
    name: DRIVER_NAME.as_ptr() as *const c_char,
};

/// Return a pointer to the static ILI9341 HAL display driver vtable.
///
/// Drop-in replacement for a C `drv_lcd_ili9341_get()`.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn drv_lcd_ili9341_get() -> *const HalDisplayDriver {
    &LCD_DRIVER as *const HalDisplayDriver
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal_registry::HalArea;

    /// Reset driver state between tests.
    fn reset_state() {
        *state_mut() = LcdState::new();
    }

    // -- Vtable metadata ------------------------------------------------------

    #[test]
    fn test_driver_dimensions() {
        let drv = unsafe { &*drv_lcd_ili9341_get() };
        assert_eq!(drv.width, 320);
        assert_eq!(drv.height, 240);
        assert_eq!(drv.display_type, HalDisplayType::Lcd);
    }

    #[test]
    fn test_driver_refresh_is_none() {
        let drv = unsafe { &*drv_lcd_ili9341_get() };
        assert!(
            drv.refresh.is_none(),
            "LCD driver must have refresh=None to distinguish it from e-paper"
        );
    }

    #[test]
    fn test_driver_name() {
        let drv = unsafe { &*drv_lcd_ili9341_get() };
        assert!(!drv.name.is_null());
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "ILI9341 (esp_lcd)");
    }

    #[test]
    fn test_driver_pointer_stable() {
        let p1 = drv_lcd_ili9341_get();
        let p2 = drv_lcd_ili9341_get();
        assert_eq!(p1, p2);
        assert!(!p1.is_null());
    }

    #[test]
    fn test_vtable_all_slots_populated() {
        let drv = unsafe { &*drv_lcd_ili9341_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.flush.is_some());
        assert!(drv.set_brightness.is_some());
        assert!(drv.sleep.is_some());
        assert!(drv.set_refresh_mode.is_some());
    }

    // -- Init / deinit lifecycle ----------------------------------------------

    #[test]
    fn test_init_null_config() {
        reset_state();
        let ret = unsafe { ili9341_init(std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_init_and_deinit() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        let ret = unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        assert!(state().initialized);
        assert_eq!(state().brightness, 100);

        unsafe { ili9341_deinit() };
        assert!(!state().initialized);
        assert!(state().io.is_null());
        assert!(state().panel.is_null());
    }

    #[test]
    fn test_double_init_is_idempotent() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        let ptr = &cfg as *const LcdIli9341Config as *const c_void;
        let ret1 = unsafe { ili9341_init(ptr) };
        let ret2 = unsafe { ili9341_init(ptr) };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK); // second call is a no-op

        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_deinit_without_init_is_safe() {
        reset_state();
        // Should not panic or crash
        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_init_stores_config() {
        reset_state();
        let cfg = LcdIli9341Config {
            spi_host: 2,
            pin_cs: 5,
            pin_dc: 6,
            pin_rst: 7,
            pin_bl: 8,
            spi_clock_hz: 20_000_000,
        };
        let ret = unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().cfg.spi_host, 2);
        assert_eq!(state().cfg.pin_cs, 5);
        assert_eq!(state().cfg.pin_dc, 6);
        assert_eq!(state().cfg.pin_rst, 7);
        assert_eq!(state().cfg.pin_bl, 8);
        assert_eq!(state().cfg.spi_clock_hz, 20_000_000);

        unsafe { ili9341_deinit() };
    }

    // -- Flush ----------------------------------------------------------------

    #[test]
    fn test_flush_before_init_returns_invalid_state() {
        reset_state();
        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let data = vec![0u8; 256];
        let ret = unsafe { ili9341_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_flush_null_area() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        let data = vec![0u8; 256];
        let ret = unsafe { ili9341_flush(std::ptr::null(), data.as_ptr()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_flush_null_data() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let ret = unsafe { ili9341_flush(&area as *const HalArea, std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_flush_succeeds_after_init() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 63, y2: 63 };
        // 64x64 pixels x 2 bytes/pixel (RGB565)
        let data = vec![0u8; 64 * 64 * 2];
        let ret = unsafe { ili9341_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        unsafe { ili9341_deinit() };
    }

    // -- set_brightness -------------------------------------------------------

    #[test]
    fn test_set_brightness_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { ili9341_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_set_brightness_no_backlight_pin_returns_not_supported() {
        reset_state();
        let cfg = LcdIli9341Config {
            pin_bl: GPIO_NUM_NC,
            ..LcdIli9341Config::default()
        };
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        let ret = unsafe { ili9341_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_NOT_SUPPORTED);

        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_set_brightness_stores_nonzero_value() {
        reset_state();
        // Use a positive pin_bl so the code path reaches bl_set_duty
        let cfg = LcdIli9341Config {
            pin_bl: 10,
            ..LcdIli9341Config::default()
        };
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };
        assert_eq!(state().brightness, 100); // set to 100 on init

        let ret = unsafe { ili9341_set_brightness(75) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 75);

        // Setting to 0 should not update brightness field
        let ret = unsafe { ili9341_set_brightness(0) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 75, "brightness field must not change on zero");

        unsafe { ili9341_deinit() };
    }

    #[test]
    fn test_set_brightness_clamped_to_100() {
        reset_state();
        let cfg = LcdIli9341Config {
            pin_bl: 10,
            ..LcdIli9341Config::default()
        };
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        // 200 > 100, should clamp and still return OK
        let ret = unsafe { ili9341_set_brightness(200) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().brightness, 100);

        unsafe { ili9341_deinit() };
    }

    // -- sleep ----------------------------------------------------------------

    #[test]
    fn test_sleep_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { ili9341_sleep(true) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_sleep_enter_and_wake() {
        reset_state();
        let cfg = LcdIli9341Config::default();
        unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };

        let ret = unsafe { ili9341_sleep(true) };
        assert_eq!(ret, ESP_OK);

        let ret = unsafe { ili9341_sleep(false) };
        assert_eq!(ret, ESP_OK);

        unsafe { ili9341_deinit() };
    }

    // -- set_refresh_mode -----------------------------------------------------

    #[test]
    fn test_set_refresh_mode_always_ok() {
        // set_refresh_mode is a no-op for LCD; always returns ESP_OK regardless
        // of initialisation state.
        for mode in [
            HalDisplayRefreshMode::Full,
            HalDisplayRefreshMode::Partial,
            HalDisplayRefreshMode::Fast,
        ] {
            let ret = unsafe { ili9341_set_refresh_mode(mode) };
            assert_eq!(ret, ESP_OK);
        }
    }

    // -- SPI clock default ----------------------------------------------------

    #[test]
    fn test_spi_clock_zero_defaults_to_26mhz() {
        reset_state();
        let cfg = LcdIli9341Config {
            spi_clock_hz: 0,
            ..LcdIli9341Config::default()
        };
        // Init should succeed; on the simulator the clock value is unused but
        // the logic must not fail.
        let ret = unsafe { ili9341_init(&cfg as *const LcdIli9341Config as *const c_void) };
        assert_eq!(ret, ESP_OK);
        unsafe { ili9341_deinit() };
    }

    // -- Config default -------------------------------------------------------

    #[test]
    fn test_config_default_values() {
        let cfg = LcdIli9341Config::default();
        assert_eq!(cfg.spi_host, 1);
        assert_eq!(cfg.pin_cs, -1);
        assert_eq!(cfg.pin_dc, -1);
        assert_eq!(cfg.pin_rst, -1);
        assert_eq!(cfg.pin_bl, -1);
        assert_eq!(cfg.spi_clock_hz, 26_000_000);
    }
}
