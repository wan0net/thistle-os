// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — GDEQ031T10 3.1" 320x240 B/W e-paper driver (Rust)
// Controller: UC8253 (compatible)
//
// SPI wiring assumed: MOSI, SCK wired to the SPI host; CS, DC, RST, BUSY are
// discrete GPIOs supplied through EpaperConfig.
//
// This is the Rust rewrite of components/drv_epaper_gdeq031t10/.
// The original C driver remains in the tree as fallback; this version is
// an alternative that will eventually replace it.

use std::cell::UnsafeCell;
use std::os::raw::c_void;

use crate::hal_registry::{HalArea, HalDisplayDriver, HalDisplayRefreshMode, HalDisplayType};

// ── ESP error codes ───────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_TIMEOUT: i32 = 0x107;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// ── Panel geometry ────────────────────────────────────────────────────────────
// T-Deck Pro is held portrait (like BlackBerry). Native panel matches:
// 240 columns × 320 rows. No rotation needed.

const EPD_WIDTH: usize = 240;
const EPD_HEIGHT: usize = 320;
const EPD_FB_BYTES: usize = EPD_WIDTH * EPD_HEIGHT / 8; // 1-bit packed = 9600 bytes

// ── UC8253 command codes ──────────────────────────────────────────────────────

const CMD_PANEL_SETTING: u8 = 0x00;
#[allow(dead_code)]
const CMD_POWER_SETTING: u8 = 0x01;
const CMD_POWER_OFF: u8 = 0x02;
const CMD_POWER_ON: u8 = 0x04;
#[allow(dead_code)]
const CMD_BOOSTER_SOFT_START: u8 = 0x06;
const CMD_DEEP_SLEEP: u8 = 0x07;
const CMD_DATA_START_TRANSMISSION: u8 = 0x10; // old frame
const CMD_NEW_DATA_TRANSMISSION: u8 = 0x13;   // new frame
#[allow(dead_code)]
const CMD_DATA_STOP: u8 = 0x11;
const CMD_DISPLAY_REFRESH: u8 = 0x12;
#[allow(dead_code)]
const CMD_LUT_FULL: u8 = 0x20;
#[allow(dead_code)]
const CMD_LUT_PARTIAL: u8 = 0x21;
#[allow(dead_code)]
const CMD_PLL_CONTROL: u8 = 0x30;
#[allow(dead_code)]
const CMD_TEMPERATURE_SENSOR: u8 = 0x40;
const CMD_VCOM_DATA_INTERVAL: u8 = 0x50;
#[allow(dead_code)]
const CMD_TCON_SETTING: u8 = 0x60;
#[allow(dead_code)]
const CMD_RESOLUTION_SETTING: u8 = 0x61;
#[allow(dead_code)]
const CMD_VCM_DC_SETTING: u8 = 0x82;

// Fast-refresh extra commands (GxEPD2 _Update_Part)
const CMD_CASCADE: u8 = 0xE0;
const CMD_TEMPERATURE_FORCED: u8 = 0xE5;

// ── LUT tables ────────────────────────────────────────────────────────────────
// Full-refresh LUT for GDEQ031T10 / UC8253 (66 bytes)

#[allow(dead_code)]
static LUT_FULL_UPDATE: [u8; 66] = [
    0x80, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x60, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x80, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x60, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x03, 0x00, 0x00, 0x02,
    0x09, 0x09, 0x00, 0x00, 0x02,
    0x03, 0x03, 0x00, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x15, 0x41, 0xA8, 0x32, 0x30, 0x0A,
];

// Partial-refresh LUT (faster, some ghosting)
#[allow(dead_code)]
static LUT_PARTIAL_UPDATE: [u8; 66] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x0A, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
    0x15, 0x41, 0xA8, 0x32, 0x30, 0x0A,
];

// ── Configuration struct (must match C epaper_gdeq031t10_config_t layout) ─────

/// Configuration for the GDEQ031T10 e-paper driver.
///
/// The layout matches `epaper_gdeq031t10_config_t` from the C header so that a
/// pointer to this struct can be passed directly from C board-init code.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct EpaperConfig {
    /// SPI host device (e.g. SPI2_HOST = 1)
    pub spi_host: i32,
    /// GPIO for chip select
    pub pin_cs: i32,
    /// GPIO for data/command
    pub pin_dc: i32,
    /// GPIO for reset (-1 = not connected)
    pub pin_rst: i32,
    /// GPIO for busy signal
    pub pin_busy: i32,
    /// SPI clock speed in Hz (0 = use default 4 MHz)
    pub spi_clock_hz: i32,
}

impl Default for EpaperConfig {
    fn default() -> Self {
        EpaperConfig {
            spi_host: 1,
            pin_cs: -1,
            pin_dc: -1,
            pin_rst: -1,
            pin_busy: -1,
            spi_clock_hz: 4_000_000,
        }
    }
}

// ── Driver state ──────────────────────────────────────────────────────────────

struct EpaperState {
    spi: *mut c_void,           // SPI device handle
    cfg: EpaperConfig,
    refresh_mode: HalDisplayRefreshMode,
    fb: *mut u8,                // Current framebuffer (EPD_FB_BYTES)
    fb_old: *mut u8,            // Previous framebuffer (EPD_FB_BYTES, for differential refresh)
    initialized: bool,
    power_on: bool,
    first_refresh_done: bool,
}

// SAFETY: Only mutated during single-threaded board init / driver calls.
unsafe impl Send for EpaperState {}
unsafe impl Sync for EpaperState {}

impl EpaperState {
    const fn new() -> Self {
        EpaperState {
            spi: std::ptr::null_mut(),
            cfg: EpaperConfig {
                spi_host: 0,
                pin_cs: 0,
                pin_dc: 0,
                pin_rst: -1,
                pin_busy: 0,
                spi_clock_hz: 0,
            },
            refresh_mode: HalDisplayRefreshMode::Full,
            fb: std::ptr::null_mut(),
            fb_old: std::ptr::null_mut(),
            initialized: false,
            power_on: false,
            first_refresh_done: false,
        }
    }
}

struct GlobalEpaperState {
    inner: UnsafeCell<EpaperState>,
}

// SAFETY: Only mutated during single-threaded board init.
unsafe impl Sync for GlobalEpaperState {}

static STATE: GlobalEpaperState = GlobalEpaperState {
    inner: UnsafeCell::new(EpaperState::new()),
};

#[inline]
fn state() -> &'static EpaperState {
    unsafe { &*STATE.inner.get() }
}

#[inline]
fn state_mut() -> &'static mut EpaperState {
    unsafe { &mut *STATE.inner.get() }
}

// ── ESP-IDF platform bindings ─────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
mod platform {
    use std::os::raw::c_void;

    // SPI transaction struct layout (matches spi_transaction_t in ESP-IDF)
    // We only need the fields we use; the rest are zeroed.
    #[repr(C)]
    pub struct SpiTransaction {
        pub flags: u32,
        pub cmd: u16,
        pub addr: u64,
        pub length: usize,   // Total data length, in bits
        pub rxlength: usize, // Total data length received, in bits (0 = length)
        pub user: *mut c_void,
        pub tx_buffer: *const u8,
        pub rx_buffer: *mut u8,
    }

    // GPIO config struct layout (matches gpio_config_t in ESP-IDF)
    #[repr(C)]
    pub struct GpioConfig {
        pub pin_bit_mask: u64,
        pub mode: u32,         // gpio_mode_t
        pub pull_up_en: u32,   // gpio_pullup_t
        pub pull_down_en: u32, // gpio_pulldown_t
        pub intr_type: u32,    // gpio_int_type_t
    }

    // SPI device interface config (matches spi_device_interface_config_t in ESP-IDF)
    #[repr(C)]
    pub struct SpiDeviceInterfaceConfig {
        pub command_bits: u8,
        pub address_bits: u8,
        pub dummy_bits: u8,
        pub mode: u8,
        pub duty_cycle_pos: u8,
        pub cs_ena_pretrans: u8,
        pub cs_ena_posttrans: u8,
        pub clock_speed_hz: i32,
        pub input_delay_ns: i32,
        pub spics_io_num: i32,
        pub flags: u32,
        pub queue_size: i32,
        pub pre_cb: *const c_void,
        pub post_cb: *const c_void,
    }

    // GPIO mode and pull constants
    pub const GPIO_MODE_OUTPUT: u32 = 3;
    pub const GPIO_MODE_INPUT: u32 = 1;
    pub const GPIO_PULLUP_DISABLE: u32 = 0;
    pub const GPIO_PULLUP_ENABLE: u32 = 1;
    pub const GPIO_PULLDOWN_DISABLE: u32 = 0;
    pub const GPIO_INTR_DISABLE: u32 = 0;

    // heap_caps flags
    pub const MALLOC_CAP_DMA: u32 = 1 << 2;  // = 4
    pub const MALLOC_CAP_8BIT: u32 = 1 << 1; // = 2

    // FreeRTOS tick rate (portTICK_PERIOD_MS = 1 on ESP32 default 1000 Hz)
    pub const TICKS_PER_MS: u32 = 1;

    extern "C" {
        // SPI
        pub fn spi_bus_add_device(
            host: i32,
            cfg: *const SpiDeviceInterfaceConfig,
            handle: *mut *mut c_void,
        ) -> i32;
        pub fn spi_bus_remove_device(handle: *mut c_void) -> i32;
        pub fn spi_device_polling_transmit(
            handle: *mut c_void,
            trans: *mut SpiTransaction,
        ) -> i32;

        // GPIO
        pub fn gpio_config(cfg: *const GpioConfig) -> i32;
        pub fn gpio_set_level(pin: u32, level: u32) -> i32;
        pub fn gpio_get_level(pin: u32) -> i32;

        // FreeRTOS
        pub fn vTaskDelay(ticks: u32);

        // Memory
        pub fn heap_caps_malloc(size: usize, caps: u32) -> *mut u8;
        pub fn heap_caps_free(ptr: *mut u8);
    }
}

// ── Low-level helpers — platform layer ───────────────────────────────────────

#[cfg(target_os = "espidf")]
unsafe fn alloc_fb() -> *mut u8 {
    platform::heap_caps_malloc(
        EPD_FB_BYTES,
        platform::MALLOC_CAP_DMA | platform::MALLOC_CAP_8BIT,
    )
}

#[cfg(target_os = "espidf")]
unsafe fn free_fb(ptr: *mut u8) {
    if !ptr.is_null() {
        platform::heap_caps_free(ptr);
    }
}

#[cfg(not(target_os = "espidf"))]
unsafe fn alloc_fb() -> *mut u8 {
    // Simulator: use Box<[u8]> and leak it (caller must free_fb to reclaim)
    let boxed = vec![0xFFu8; EPD_FB_BYTES].into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}

#[cfg(not(target_os = "espidf"))]
unsafe fn free_fb(ptr: *mut u8) {
    if !ptr.is_null() {
        // Re-box and drop
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, EPD_FB_BYTES));
    }
}

// FreeRTOS delay wrapper (ms → ticks)
#[cfg(target_os = "espidf")]
unsafe fn delay_ms(ms: u32) {
    platform::vTaskDelay(ms * platform::TICKS_PER_MS);
}

#[cfg(not(target_os = "espidf"))]
fn delay_ms(_ms: u32) {
    // Simulator: no actual delay
}

// GPIO helpers
#[cfg(target_os = "espidf")]
unsafe fn gpio_set(pin: i32, level: u32) {
    if pin >= 0 {
        platform::gpio_set_level(pin as u32, level);
    }
}

#[cfg(not(target_os = "espidf"))]
fn gpio_set(_pin: i32, _level: u32) {}

#[cfg(target_os = "espidf")]
unsafe fn gpio_read(pin: i32) -> i32 {
    if pin >= 0 {
        platform::gpio_get_level(pin as u32)
    } else {
        0
    }
}

#[cfg(not(target_os = "espidf"))]
fn gpio_read(_pin: i32) -> i32 {
    0 // Simulator: BUSY always low (idle)
}

// ── SPI transmit helpers ──────────────────────────────────────────────────────

const SPI_CHUNK: usize = 4096;

/// Transmit a single command byte (DC=0).
unsafe fn epaper_send_cmd(cmd: u8) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let s = state();
        gpio_set(s.cfg.pin_cs, 0); // select
        gpio_set(s.cfg.pin_dc, 0); // command mode

        let mut t = platform::SpiTransaction {
            flags: 0,
            cmd: 0,
            addr: 0,
            length: 8,
            rxlength: 0,
            user: std::ptr::null_mut(),
            tx_buffer: &cmd as *const u8,
            rx_buffer: std::ptr::null_mut(),
        };
        let ret = platform::spi_device_polling_transmit(s.spi, &mut t);
        gpio_set(s.cfg.pin_cs, 1); // deselect
        ret
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = cmd;
        ESP_OK
    }
}

/// Transmit data bytes (DC=1), in SPI_CHUNK-sized pieces.
unsafe fn epaper_send_data(data: *const u8, len: usize) -> i32 {
    if len == 0 {
        return ESP_OK;
    }

    #[cfg(target_os = "espidf")]
    {
        let s = state();
        gpio_set(s.cfg.pin_cs, 0); // select
        gpio_set(s.cfg.pin_dc, 1); // data mode

        let mut ret = ESP_OK;
        let mut sent = 0usize;
        while sent < len && ret == ESP_OK {
            let chunk = (len - sent).min(SPI_CHUNK);
            let mut t = platform::SpiTransaction {
                flags: 0,
                cmd: 0,
                addr: 0,
                length: chunk * 8,
                rxlength: 0,
                user: std::ptr::null_mut(),
                tx_buffer: data.add(sent),
                rx_buffer: std::ptr::null_mut(),
            };
            ret = platform::spi_device_polling_transmit(s.spi, &mut t);
            sent += chunk;
        }

        gpio_set(s.cfg.pin_cs, 1); // deselect
        ret
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (data, len);
        ESP_OK
    }
}

/// Transmit a single data byte.
unsafe fn epaper_send_data_byte(val: u8) -> i32 {
    epaper_send_data(&val as *const u8, 1)
}

// ── Hardware reset ────────────────────────────────────────────────────────────

unsafe fn epaper_hw_reset() {
    let s = state();
    if s.cfg.pin_rst < 0 {
        // RST not connected — skip hardware reset, just wait
        delay_ms(20);
        return;
    }
    gpio_set(s.cfg.pin_rst, 0);
    delay_ms(10);
    gpio_set(s.cfg.pin_rst, 1);
    delay_ms(10);
}

// ── Wait for BUSY to go low (display idle) ────────────────────────────────────

/// Polls the BUSY pin until it goes low (idle) or timeout_ms elapses.
///
/// Returns ESP_OK on success, ESP_ERR_TIMEOUT on failure.
unsafe fn epaper_wait_busy(timeout_ms: u32) -> i32 {
    let s = state();
    let mut elapsed: u32 = 0;
    while gpio_read(s.cfg.pin_busy) != 0 {
        delay_ms(10);
        elapsed += 10;
        if elapsed >= timeout_ms {
            return ESP_ERR_TIMEOUT;
        }
    }
    ESP_OK
}

// ── GPIO init helpers (ESP-IDF only) ─────────────────────────────────────────

#[cfg(target_os = "espidf")]
unsafe fn configure_output_gpios(cfg: &EpaperConfig) -> i32 {
    let mut mask: u64 = (1u64 << cfg.pin_cs) | (1u64 << cfg.pin_dc);
    if cfg.pin_rst >= 0 {
        mask |= 1u64 << cfg.pin_rst;
    }
    let io_conf = platform::GpioConfig {
        pin_bit_mask: mask,
        mode: platform::GPIO_MODE_OUTPUT,
        pull_up_en: platform::GPIO_PULLUP_DISABLE,
        pull_down_en: platform::GPIO_PULLDOWN_DISABLE,
        intr_type: platform::GPIO_INTR_DISABLE,
    };
    platform::gpio_config(&io_conf)
}

#[cfg(target_os = "espidf")]
unsafe fn configure_input_gpios(cfg: &EpaperConfig) -> i32 {
    let busy_conf = platform::GpioConfig {
        pin_bit_mask: 1u64 << cfg.pin_busy,
        mode: platform::GPIO_MODE_INPUT,
        pull_up_en: platform::GPIO_PULLUP_ENABLE,
        pull_down_en: platform::GPIO_PULLDOWN_DISABLE,
        intr_type: platform::GPIO_INTR_DISABLE,
    };
    platform::gpio_config(&busy_conf)
}

#[cfg(target_os = "espidf")]
unsafe fn add_spi_device(cfg: &EpaperConfig) -> (*mut c_void, i32) {
    let clock = if cfg.spi_clock_hz > 0 {
        cfg.spi_clock_hz
    } else {
        4_000_000
    };
    let dev_cfg = platform::SpiDeviceInterfaceConfig {
        command_bits: 0,
        address_bits: 0,
        dummy_bits: 0,
        mode: 0,
        duty_cycle_pos: 0,
        cs_ena_pretrans: 0,
        cs_ena_posttrans: 0,
        clock_speed_hz: clock,
        input_delay_ns: 0,
        spics_io_num: -1, // Manual CS — same as GxEPD2
        flags: 0,
        queue_size: 1,
        pre_cb: std::ptr::null(),
        post_cb: std::ptr::null(),
    };
    let mut spi_handle: *mut c_void = std::ptr::null_mut();
    let ret = platform::spi_bus_add_device(cfg.spi_host, &dev_cfg, &mut spi_handle);
    (spi_handle, ret)
}

#[cfg(not(target_os = "espidf"))]
unsafe fn configure_output_gpios(_cfg: &EpaperConfig) -> i32 { ESP_OK }

#[cfg(not(target_os = "espidf"))]
unsafe fn configure_input_gpios(_cfg: &EpaperConfig) -> i32 { ESP_OK }

#[cfg(not(target_os = "espidf"))]
unsafe fn add_spi_device(_cfg: &EpaperConfig) -> (*mut c_void, i32) {
    // Simulator: return a dummy non-null pointer (1usize cast) so the driver
    // doesn't think the handle is null.
    (1usize as *mut c_void, ESP_OK)
}

// ── Driver vtable functions ───────────────────────────────────────────────────

/// Initialise the e-paper display.
///
/// `config` must point to an `EpaperConfig`-compatible struct.
pub unsafe extern "C" fn gdeq031t10_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let s = state_mut();
    if s.initialized {
        return ESP_OK; // already initialised
    }

    // Copy config
    let cfg = &*(config as *const EpaperConfig);
    s.cfg = *cfg;
    s.refresh_mode = HalDisplayRefreshMode::Full;

    // Allocate framebuffers
    s.fb = alloc_fb();
    s.fb_old = alloc_fb();
    if s.fb.is_null() || s.fb_old.is_null() {
        free_fb(s.fb);
        free_fb(s.fb_old);
        s.fb = std::ptr::null_mut();
        s.fb_old = std::ptr::null_mut();
        return ESP_ERR_NO_MEM;
    }

    // Initialise both framebuffers to white (0xFF = all bits set = white for e-paper)
    std::ptr::write_bytes(s.fb, 0xFF, EPD_FB_BYTES);
    std::ptr::write_bytes(s.fb_old, 0xFF, EPD_FB_BYTES);

    // Configure output GPIOs (CS, DC, RST)
    let mut ret = configure_output_gpios(&s.cfg);
    if ret != ESP_OK {
        free_fb(s.fb);
        free_fb(s.fb_old);
        s.fb = std::ptr::null_mut();
        s.fb_old = std::ptr::null_mut();
        return ret;
    }

    // Configure input GPIO (BUSY)
    ret = configure_input_gpios(&s.cfg);
    if ret != ESP_OK {
        free_fb(s.fb);
        free_fb(s.fb_old);
        s.fb = std::ptr::null_mut();
        s.fb_old = std::ptr::null_mut();
        return ret;
    }

    // CS defaults high
    gpio_set(s.cfg.pin_cs, 1);

    // Attach SPI device
    let (spi_handle, spi_ret) = add_spi_device(&s.cfg);
    if spi_ret != ESP_OK {
        free_fb(s.fb);
        free_fb(s.fb_old);
        s.fb = std::ptr::null_mut();
        s.fb_old = std::ptr::null_mut();
        return spi_ret;
    }
    s.spi = spi_handle;

    // UC8253 init sequence (mirrors GxEPD2_310_GDEQ031T10)
    epaper_hw_reset();

    // Soft reset via Panel Setting (reset bit set)
    let mut ret = epaper_send_cmd(CMD_PANEL_SETTING);
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x1E); // reset bit set
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x0D);
    if ret != ESP_OK { return fail_init(); }
    delay_ms(10);

    // Panel setting (actual operating config)
    ret = epaper_send_cmd(CMD_PANEL_SETTING);
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x1F); // KW mode, BWOTP
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x0D);
    if ret != ESP_OK { return fail_init(); }

    s.initialized = true;
    s.power_on = false;
    s.first_refresh_done = false;
    // Re-initialise framebuffers to white after hardware init
    std::ptr::write_bytes(s.fb, 0xFF, EPD_FB_BYTES);
    std::ptr::write_bytes(s.fb_old, 0xFF, EPD_FB_BYTES);

    ESP_OK
}

/// Cleanup path during init failure — removes SPI device and frees framebuffers.
unsafe fn fail_init() -> i32 {
    let s = state_mut();
    #[cfg(target_os = "espidf")]
    if !s.spi.is_null() {
        platform::spi_bus_remove_device(s.spi);
    }
    free_fb(s.fb);
    free_fb(s.fb_old);
    s.fb = std::ptr::null_mut();
    s.fb_old = std::ptr::null_mut();
    s.spi = std::ptr::null_mut();
    ESP_FAIL
}

/// De-initialise: deep sleep the panel then release resources.
pub unsafe extern "C" fn gdeq031t10_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }

    // Issue deep sleep command before removing device
    let _ = epaper_send_cmd(CMD_DEEP_SLEEP);
    let _ = epaper_send_data_byte(0xA5); // check code

    #[cfg(target_os = "espidf")]
    if !s.spi.is_null() {
        platform::spi_bus_remove_device(s.spi);
    }
    s.spi = std::ptr::null_mut();

    free_fb(s.fb);
    free_fb(s.fb_old);
    s.fb = std::ptr::null_mut();
    s.fb_old = std::ptr::null_mut();

    s.initialized = false;
}

/// Flush — copy pixel data from `color_data` into the in-memory framebuffer.
///
/// `area` defines the rectangular region being updated. Data format is
/// 1-bit packed (MSB first), matching the native panel and LVGL 1bpp mode.
/// The actual SPI transfer to the panel is deferred to `refresh()`.
pub unsafe extern "C" fn gdeq031t10_flush(area: *const HalArea, color_data: *const u8) -> i32 {
    let s = state();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if area.is_null() || color_data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let area = &*area;
    let x1 = area.x1 as usize;
    let y1 = area.y1 as usize;
    let mut x2 = area.x2 as usize;
    let mut y2 = area.y2 as usize;

    if x2 >= EPD_WIDTH  { x2 = EPD_WIDTH  - 1; }
    if y2 >= EPD_HEIGHT { y2 = EPD_HEIGHT - 1; }

    if x1 > x2 || y1 > y2 {
        return ESP_ERR_INVALID_ARG;
    }

    // Direct copy — no rotation. LVGL renders 240×320 portrait, matching native.
    // Updates only the in-memory framebuffer (fast). The actual panel refresh
    // is triggered separately via refresh().
    let src_w = x2 - x1 + 1;
    let fb = std::slice::from_raw_parts_mut(s.fb, EPD_FB_BYTES);

    for row in y1..=y2 {
        for col in x1..=x2 {
            let src_bit_idx = (row - y1) * src_w + (col - x1);
            let src_byte = *color_data.add(src_bit_idx / 8);
            let src_bit = (src_byte >> (7 - (src_bit_idx & 7))) & 1;

            let dst_bit_idx = row * EPD_WIDTH + col;
            let dst_byte_idx = dst_bit_idx / 8;
            let dst_mask = 0x80u8 >> (dst_bit_idx & 7);

            if src_bit != 0 {
                fb[dst_byte_idx] |= dst_mask;
            } else {
                fb[dst_byte_idx] &= !dst_mask;
            }
        }
    }

    ESP_OK
}

/// Refresh — send the framebuffer to the panel and trigger a hardware update.
///
/// Called once after the UI has settled (debounced by the UI manager).
/// Full-frame refresh sequence mirrors GxEPD2:
///   soft reset → panel setting → write data → VCOM → power on → refresh → power off
pub unsafe extern "C" fn gdeq031t10_refresh() -> i32 {
    let s = state_mut();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Use fast mode unless: first refresh, or explicitly set to FULL
    let fast = s.first_refresh_done && s.refresh_mode != HalDisplayRefreshMode::Full;

    // Soft reset via panel setting register
    let mut err = epaper_send_cmd(CMD_PANEL_SETTING);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x1E);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x0D);
    if err != ESP_OK { return err; }
    delay_ms(5);

    // Panel setting (operating config)
    err = epaper_send_cmd(CMD_PANEL_SETTING);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x1F);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x0D);
    if err != ESP_OK { return err; }

    // Write old framebuffer via cmd 0x10 (previous frame)
    err = epaper_send_cmd(CMD_DATA_START_TRANSMISSION);
    if err != ESP_OK { return err; }
    err = epaper_send_data(s.fb_old, EPD_FB_BYTES);
    if err != ESP_OK { return err; }

    // Write new framebuffer via cmd 0x13 (current frame)
    err = epaper_send_cmd(CMD_NEW_DATA_TRANSMISSION);
    if err != ESP_OK { return err; }
    err = epaper_send_data(s.fb, EPD_FB_BYTES);
    if err != ESP_OK { return err; }

    // VCOM and data interval — different for full vs fast refresh
    // Full: 0x97 (no flicker reduction), Fast: 0xD7 (partial mode)
    err = epaper_send_cmd(CMD_VCOM_DATA_INTERVAL);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(if fast { 0xD7 } else { 0x97 });
    if err != ESP_OK { return err; }

    if fast {
        // Fast refresh: cascade + forced temperature (GxEPD2 _Update_Part)
        err = epaper_send_cmd(CMD_CASCADE);
        if err != ESP_OK { return err; }
        err = epaper_send_data_byte(0x02); // TSFIX
        if err != ESP_OK { return err; }
        err = epaper_send_cmd(CMD_TEMPERATURE_FORCED);
        if err != ESP_OK { return err; }
        err = epaper_send_data_byte(0x79); // temp=121°, faster LUT
        if err != ESP_OK { return err; }
    }

    // Power on
    err = epaper_send_cmd(CMD_POWER_ON);
    if err != ESP_OK { return err; }
    err = epaper_wait_busy(5_000);
    if err != ESP_OK { return err; }

    // Display refresh
    err = epaper_send_cmd(CMD_DISPLAY_REFRESH);
    if err != ESP_OK { return err; }
    let refresh_timeout = if fast { 5_000 } else { 15_000 };
    err = epaper_wait_busy(refresh_timeout);
    if err != ESP_OK { return err; }

    // Power off
    err = epaper_send_cmd(CMD_POWER_OFF);
    if err != ESP_OK { return err; }
    let _ = epaper_wait_busy(5_000); // best-effort
    s.power_on = false;

    // Save current frame as "old" for next differential refresh
    std::ptr::copy_nonoverlapping(s.fb, s.fb_old, EPD_FB_BYTES);

    if !s.first_refresh_done {
        s.first_refresh_done = true;
    }
    // After a full refresh, auto-switch back to fast for subsequent updates
    if !fast {
        s.refresh_mode = HalDisplayRefreshMode::Fast;
    }

    ESP_OK
}

/// Set brightness — not applicable to e-paper, always returns NOT_SUPPORTED.
pub unsafe extern "C" fn gdeq031t10_set_brightness(_percent: u8) -> i32 {
    ESP_ERR_NOT_SUPPORTED
}

/// Sleep / wake the panel.
///
/// `enter = true`:  power off + deep sleep.
/// `enter = false`: hardware reset + power on.
pub unsafe extern "C" fn gdeq031t10_sleep(enter: bool) -> i32 {
    let s = state();
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if enter {
        let ret = epaper_send_cmd(CMD_POWER_OFF);
        if ret != ESP_OK { return ret; }
        let ret = epaper_wait_busy(3_000);
        if ret != ESP_OK { return ret; }

        let ret = epaper_send_cmd(CMD_DEEP_SLEEP);
        if ret != ESP_OK { return ret; }
        epaper_send_data_byte(0xA5)
    } else {
        // Wake: hardware reset + re-issue power-on
        epaper_hw_reset();
        let ret = epaper_wait_busy(3_000);
        if ret != ESP_OK { return ret; }
        let ret = epaper_send_cmd(CMD_POWER_ON);
        if ret != ESP_OK { return ret; }
        epaper_wait_busy(3_000)
    }
}

/// Set the refresh mode (full vs fast/partial).
pub unsafe extern "C" fn gdeq031t10_set_refresh_mode(mode: HalDisplayRefreshMode) -> i32 {
    state_mut().refresh_mode = mode;
    ESP_OK
}

// ── Driver name (static C string) ─────────────────────────────────────────────

static DRIVER_NAME: &[u8] = b"GDEQ031T10\0";

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL display driver vtable for the GDEQ031T10.
///
/// This is the Rust equivalent of the C `gdeq031t10_driver` static.
static EPAPER_DRIVER: HalDisplayDriver = HalDisplayDriver {
    init: Some(gdeq031t10_init),
    deinit: Some(gdeq031t10_deinit),
    flush: Some(gdeq031t10_flush),
    refresh: Some(gdeq031t10_refresh),
    set_brightness: Some(gdeq031t10_set_brightness),
    sleep: Some(gdeq031t10_sleep),
    set_refresh_mode: Some(gdeq031t10_set_refresh_mode),
    width: EPD_WIDTH as u16,
    height: EPD_HEIGHT as u16,
    display_type: HalDisplayType::Epaper,
    name: DRIVER_NAME.as_ptr() as *const std::os::raw::c_char,
};

// SAFETY: EPAPER_DRIVER contains only raw pointers and fn pointers; Sync is safe.
// (HalDisplayDriver already derives Sync via hal_registry, but the statics
// in this module need a local guarantee too.)
// The blanket unsafe impl on HalDisplayDriver covers this.

/// Return a pointer to the static GDEQ031T10 HAL display driver vtable.
///
/// Drop-in replacement for the C `drv_epaper_gdeq031t10_get()`.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn drv_epaper_gdeq031t10_get() -> *const HalDisplayDriver {
    &EPAPER_DRIVER as *const HalDisplayDriver
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal_registry::HalArea;

    /// Reset driver state between tests.
    fn reset_state() {
        let s = state_mut();
        // Free any allocated framebuffers from a previous test run
        unsafe {
            free_fb(s.fb);
            free_fb(s.fb_old);
        }
        *s = EpaperState::new();
    }

    #[test]
    fn test_driver_name() {
        let drv = unsafe { &*drv_epaper_gdeq031t10_get() };
        assert!(!drv.name.is_null());
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "GDEQ031T10");
    }

    #[test]
    fn test_driver_dimensions() {
        let drv = unsafe { &*drv_epaper_gdeq031t10_get() };
        assert_eq!(drv.width, 240);
        assert_eq!(drv.height, 320);
        assert_eq!(drv.display_type, HalDisplayType::Epaper);
    }

    #[test]
    fn test_driver_pointer_stable() {
        let p1 = drv_epaper_gdeq031t10_get();
        let p2 = drv_epaper_gdeq031t10_get();
        assert_eq!(p1, p2);
        assert!(!p1.is_null());
    }

    #[test]
    fn test_flush_before_init_returns_invalid_state() {
        reset_state();
        let area = HalArea { x1: 0, y1: 0, x2: 10, y2: 10 };
        let data = vec![0u8; 16];
        let ret = unsafe {
            gdeq031t10_flush(&area as *const HalArea, data.as_ptr())
        };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_refresh_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { gdeq031t10_refresh() };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_sleep_before_init_returns_invalid_state() {
        reset_state();
        let ret = unsafe { gdeq031t10_sleep(true) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_init_null_config() {
        reset_state();
        let ret = unsafe { gdeq031t10_init(std::ptr::null()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_init_and_flush() {
        reset_state();
        let cfg = EpaperConfig {
            spi_host: 1,
            pin_cs: 5,
            pin_dc: 6,
            pin_rst: -1,
            pin_busy: 7,
            spi_clock_hz: 4_000_000,
        };
        let ret = unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };
        assert_eq!(ret, ESP_OK);
        assert!(state().initialized);

        // Check framebuffer was initialised to white
        let fb = unsafe { std::slice::from_raw_parts(state().fb, EPD_FB_BYTES) };
        assert!(fb.iter().all(|&b| b == 0xFF));

        // Flush a small black area (0x00 = black pixels) in the top-left corner
        let area = HalArea { x1: 0, y1: 0, x2: 7, y2: 0 }; // 8 pixels wide, 1 row
        let data = [0x00u8; 1]; // 8 pixels, all black
        let ret = unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        // First byte of fb should now be 0x00 (all black)
        let fb = unsafe { std::slice::from_raw_parts(state().fb, EPD_FB_BYTES) };
        assert_eq!(fb[0], 0x00);
        // The rest should still be white
        assert!(fb[1..].iter().all(|&b| b == 0xFF));

        // Flush a white area back
        let area = HalArea { x1: 0, y1: 0, x2: 7, y2: 0 };
        let data = [0xFFu8; 1];
        let ret = unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);
        let fb = unsafe { std::slice::from_raw_parts(state().fb, EPD_FB_BYTES) };
        assert_eq!(fb[0], 0xFF);

        // Cleanup
        unsafe { gdeq031t10_deinit() };
        assert!(!state().initialized);
    }

    #[test]
    fn test_flush_null_args() {
        reset_state();
        let cfg = EpaperConfig::default();
        unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };

        let area = HalArea { x1: 0, y1: 0, x2: 7, y2: 0 };
        let data = [0u8; 1];

        assert_eq!(
            unsafe { gdeq031t10_flush(std::ptr::null(), data.as_ptr()) },
            ESP_ERR_INVALID_ARG
        );
        assert_eq!(
            unsafe { gdeq031t10_flush(&area as *const HalArea, std::ptr::null()) },
            ESP_ERR_INVALID_ARG
        );

        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_flush_clips_to_display_bounds() {
        reset_state();
        let cfg = EpaperConfig::default();
        unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };

        // Area that extends beyond display bounds — should be clipped, not panic
        let area = HalArea {
            x1: 235,
            y1: 315,
            x2: 300, // > EPD_WIDTH
            y2: 400, // > EPD_HEIGHT
        };
        let data = vec![0x00u8; 64];
        let ret = unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_set_brightness_not_supported() {
        let ret = unsafe { gdeq031t10_set_brightness(50) };
        assert_eq!(ret, ESP_ERR_NOT_SUPPORTED);
    }

    #[test]
    fn test_set_refresh_mode() {
        reset_state();
        let cfg = EpaperConfig::default();
        unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };

        let ret = unsafe { gdeq031t10_set_refresh_mode(HalDisplayRefreshMode::Full) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().refresh_mode, HalDisplayRefreshMode::Full);

        let ret = unsafe { gdeq031t10_set_refresh_mode(HalDisplayRefreshMode::Fast) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(state().refresh_mode, HalDisplayRefreshMode::Fast);

        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_double_init_is_idempotent() {
        reset_state();
        let cfg = EpaperConfig::default();
        let ptr = &cfg as *const EpaperConfig as *const c_void;
        let ret1 = unsafe { gdeq031t10_init(ptr) };
        let ret2 = unsafe { gdeq031t10_init(ptr) };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK); // second call is a no-op

        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_deinit_without_init_is_safe() {
        reset_state();
        // Should not panic or crash
        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_fb_size_constant() {
        assert_eq!(EPD_FB_BYTES, 9600);
    }

    #[test]
    fn test_flush_single_pixel() {
        reset_state();
        let cfg = EpaperConfig::default();
        unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };

        // Set a single pixel at (0,0) to black (bit 7 of byte 0 = 0)
        let area = HalArea { x1: 0, y1: 0, x2: 0, y2: 0 };
        let data = [0x00u8]; // MSB = 0 = black
        unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        let fb = unsafe { std::slice::from_raw_parts(state().fb, EPD_FB_BYTES) };
        assert_eq!(fb[0] & 0x80, 0x00, "pixel (0,0) should be black");

        // Set it back to white (bit 7 = 1)
        let data = [0x80u8]; // MSB = 1 = white
        unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        let fb = unsafe { std::slice::from_raw_parts(state().fb, EPD_FB_BYTES) };
        assert_eq!(fb[0] & 0x80, 0x80, "pixel (0,0) should be white");

        unsafe { gdeq031t10_deinit() };
    }

    #[test]
    fn test_refresh_on_simulator() {
        // On the simulator, refresh should succeed (all SPI ops are stubs)
        reset_state();
        let cfg = EpaperConfig::default();
        unsafe { gdeq031t10_init(&cfg as *const EpaperConfig as *const c_void) };
        let ret = unsafe { gdeq031t10_refresh() };
        assert_eq!(ret, ESP_OK);

        // After a full refresh, mode should auto-switch to Fast
        assert_eq!(state().refresh_mode, HalDisplayRefreshMode::Fast);
        assert!(state().first_refresh_done);

        unsafe { gdeq031t10_deinit() };
    }
}
