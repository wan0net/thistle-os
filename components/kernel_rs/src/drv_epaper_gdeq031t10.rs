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
const CMD_POWER_SETTING: u8 = 0x01;
const CMD_POWER_OFF: u8 = 0x02;
const CMD_POWER_ON: u8 = 0x04;
const CMD_BOOSTER_SOFT_START: u8 = 0x06;
const CMD_RESOLUTION_SETTING: u8 = 0x61;
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

// ── Xtensa register-window-safe static buffers ────────────────────────────────
//
// On Xtensa LX7 (ESP32-S3), CALL8 rotates the register window by 8. When 4
// outstanding CALL8 frames are live, the oldest frame's registers are spilled
// ("overflow") to a save area on the callee's stack — and restored
// ("underflow") on RETW. If any intermediate callee uses that save-area slot
// as a local variable, the spilled value is corrupted and the register emerges
// from underflow with garbage.
//
// Stack-allocated addresses (e.g. `&cmd as *const u8`) and heap pointers
// stored in struct fields are vulnerable: they sit in registers that can be
// evicted and corrupted mid–call chain.
//
// Static globals have **link-time constant addresses** encoded in `l32r`
// literals. Even after corruption of live registers, an `l32r` instruction
// re-reads the literal from the literal pool — which is read-only and never
// corrupted by window overflow/underflow. These buffers are therefore immune.
//
// `.dram0.bss` placement: forces internal DRAM (never PSRAM) so that:
//   • The SPI polling-transmit function can safely read via CPU data bus
//   • The addresses stay in the range accessible to all peripheral DMA engines

// ── Wrapper for static buffers ──────────────────────────────────────────────
// Using `static` + `UnsafeCell` (same pattern as the `STATE` global) instead
// of `static mut` avoids the `.dram0.bss` section that ESP-IDF v6's linker
// script does not accept as an input pattern (`--orphan-handling=error`).
// `static` zero-initialized values go to `.bss` which IS matched by the
// linker script's `*(.bss .bss.*)` glob.

struct StaticCell<T>(UnsafeCell<T>);
// SAFETY: only accessed from a single FreeRTOS task (the render task).
unsafe impl<T> Sync for StaticCell<T> {}

/// Single-byte command staging buffer — replaces stack `&cmd` in epaper_send_cmd.
static SPI_CMD_BUF: StaticCell<u8> = StaticCell(UnsafeCell::new(0));

/// Current framebuffer (240×320 / 8 = 9600 bytes, 1-bit packed).
static FB: StaticCell<[u8; EPD_FB_BYTES]> = StaticCell(UnsafeCell::new([0u8; EPD_FB_BYTES]));

/// Previous framebuffer — used for differential (ghost-free) refresh.
static FB_OLD: StaticCell<[u8; EPD_FB_BYTES]> = StaticCell(UnsafeCell::new([0u8; EPD_FB_BYTES]));

#[inline]
unsafe fn fb_ptr() -> *mut u8 { (*FB.0.get()).as_mut_ptr() }
#[inline]
unsafe fn fb_old_ptr() -> *mut u8 { (*FB_OLD.0.get()).as_mut_ptr() }
#[inline]
unsafe fn spi_cmd_ptr() -> *mut u8 { SPI_CMD_BUF.0.get() }

// ── Driver state ──────────────────────────────────────────────────────────────

struct EpaperState {
    spi: *mut c_void,           // SPI device handle
    cfg: EpaperConfig,
    refresh_mode: HalDisplayRefreshMode,
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
//
// All GPIO, SPI, and delay operations go through GCC-compiled C shims
// (epaper_spi_shim.c).  Direct Rust→IDF calls are avoided because the LLVM
// Xtensa backend may not reserve the mandatory 16-byte WindowOverflow8 save
// area at the top of each frame.  When the call chain from FreeRTOS to
// gdeq031t10_init is already 7 CALL8 frames deep, adding IDF's 3–4 more
// frames triggers overflow into Rust-frame locals.  GCC shims have correctly
// sized frames, so overflow always writes to the reserved top-16 bytes.
//
// `spi_bus_add_device` / `spi_bus_remove_device` are still called directly
// from Rust (once during init/deinit only) using the full ESP-IDF struct ABI.

#[cfg(target_os = "espidf")]
mod platform {
    use std::os::raw::c_void;

    // SPI device interface config (matches spi_device_interface_config_t in ESP-IDF v6)
    #[repr(C)]
    pub struct SpiDeviceInterfaceConfig {
        pub command_bits: u8,
        pub address_bits: u8,
        pub dummy_bits: u8,
        pub mode: u8,
        pub clock_source: i32,
        pub duty_cycle_pos: u16,
        pub cs_ena_pretrans: u16,
        pub cs_ena_posttrans: u8,
        pub clock_speed_hz: i32,
        pub input_delay_ns: i32,
        pub sample_point: i32,
        pub spics_io_num: i32,
        pub flags: u32,
        pub queue_size: i32,
        pub pre_cb: *const c_void,
        pub post_cb: *const c_void,
    }

    extern "C" {
        pub fn spi_bus_add_device(
            host: i32,
            cfg: *const SpiDeviceInterfaceConfig,
            handle: *mut *mut c_void,
        ) -> i32;
        pub fn spi_bus_remove_device(handle: *mut c_void) -> i32;
    }
}

// ── GCC-compiled shim bindings (epaper_spi_shim.c) ───────────────────────────
//
// These C functions replace all direct Rust→IDF calls for GPIO, SPI, and delay.
// GCC generates frames that correctly reserve 16 bytes at the top for the
// Xtensa WindowOverflow8 save area, preventing register corruption.

#[cfg(target_os = "espidf")]
extern "C" {
    // GPIO configuration
    fn epaper_gpio_config_outputs(pin_cs: i32, pin_dc: i32, pin_rst: i32) -> i32;
    fn epaper_gpio_config_busy(pin_busy: i32) -> i32;
    // GPIO set/get (pin<0 is a no-op / returns 0)
    fn epaper_gpio_set(pin: i32, level: u32);
    fn epaper_gpio_get(pin: i32) -> i32;
    // FreeRTOS delay
    fn epaper_delay_ms(ms: u32);
    // SPI send (CS and DC toggled inside the shim)
    fn epaper_spi_cmd(spi: *mut c_void, pin_cs: i32, pin_dc: i32, cmd: u8) -> i32;
    fn epaper_spi_data(
        spi: *mut c_void,
        pin_cs: i32,
        pin_dc: i32,
        data: *const u8,
        len: usize,
    ) -> i32;
    fn esp_log_write(level: i32, tag: *const u8, fmt: *const u8, ...);
}

#[cfg(target_os = "espidf")]
macro_rules! epd_log {
    ($fmt:expr) => {
        unsafe { esp_log_write(3, b"epaper\0".as_ptr(), concat!($fmt, "\0").as_ptr()) }
    };
}

// ── Low-level helpers ─────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
#[inline(always)]
unsafe fn delay_ms(ms: u32) {
    epaper_delay_ms(ms);
}

#[cfg(not(target_os = "espidf"))]
fn delay_ms(_ms: u32) {}

#[cfg(target_os = "espidf")]
#[inline(always)]
unsafe fn gpio_set(pin: i32, level: u32) {
    epaper_gpio_set(pin, level);
}

#[cfg(not(target_os = "espidf"))]
fn gpio_set(_pin: i32, _level: u32) {}

#[cfg(target_os = "espidf")]
#[inline(always)]
unsafe fn gpio_read(pin: i32) -> i32 {
    epaper_gpio_get(pin)
}

#[cfg(not(target_os = "espidf"))]
fn gpio_read(_pin: i32) -> i32 {
    0
}

// ── SPI transmit helpers ──────────────────────────────────────────────────────
//
// These call the GCC-compiled shims in epaper_spi_shim.c.
// The shims handle CS/DC GPIO toggling and the SPI transfer internally,
// so from Rust each "send" is a single CALL8 (one additional frame).

/// Transmit a single command byte (DC=0).
#[inline(always)]
unsafe fn epaper_send_cmd(cmd: u8) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let s = state();
        epaper_spi_cmd(s.spi, s.cfg.pin_cs, s.cfg.pin_dc, cmd)
    }
    #[cfg(not(target_os = "espidf"))]
    { let _ = cmd; ESP_OK }
}

/// Transmit data bytes (DC=1).
#[inline(always)]
unsafe fn epaper_send_data(data: *const u8, len: usize) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let s = state();
        epaper_spi_data(s.spi, s.cfg.pin_cs, s.cfg.pin_dc, data, len)
    }
    #[cfg(not(target_os = "espidf"))]
    { let _ = (data, len); ESP_OK }
}

/// Transmit a single data byte.
#[inline(always)]
unsafe fn epaper_send_data_byte(val: u8) -> i32 {
    #[cfg(target_os = "espidf")]
    {
        let s = state();
        // Use SPI_CMD_BUF as staging area so &val doesn't become a dangling
        // stack pointer from the caller's frame (defensive: the C shim copies
        // the byte from the pointer before returning, so &val is valid, but
        // using the static buffer is safer and avoids any frame-lifetime issue).
        *spi_cmd_ptr() = val;
        epaper_spi_data(s.spi, s.cfg.pin_cs, s.cfg.pin_dc, spi_cmd_ptr(), 1)
    }
    #[cfg(not(target_os = "espidf"))]
    { let _ = val; ESP_OK }
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
            #[cfg(target_os = "espidf")]
            epd_log!("I (?) epaper: BUSY timeout");
            return ESP_ERR_TIMEOUT;
        }
    }
    ESP_OK
}

// ── GPIO init helpers ─────────────────────────────────────────────────────────
// These call the GCC-compiled C shims which correctly handle the Xtensa ABI.

#[cfg(target_os = "espidf")]
unsafe fn configure_output_gpios(cfg: &EpaperConfig) -> i32 {
    epaper_gpio_config_outputs(cfg.pin_cs, cfg.pin_dc, cfg.pin_rst)
}

#[cfg(target_os = "espidf")]
unsafe fn configure_input_gpios(cfg: &EpaperConfig) -> i32 {
    epaper_gpio_config_busy(cfg.pin_busy)
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
        clock_source: 0, // 0 → ESP-IDF uses SPI_CLK_SRC_DEFAULT
        duty_cycle_pos: 0,
        cs_ena_pretrans: 0,
        cs_ena_posttrans: 0,
        clock_speed_hz: clock,
        input_delay_ns: 0,
        sample_point: 0, // SPI_SAMPLING_POINT_PHASE_0 (default)
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

    // Initialise both static framebuffers to white (0xFF = all bits = white for e-paper).
    // These are static globals with link-time constant addresses — no heap allocation needed.
    std::ptr::write_bytes(fb_ptr(), 0xFF, EPD_FB_BYTES);
    std::ptr::write_bytes(fb_old_ptr(), 0xFF, EPD_FB_BYTES);

    // Configure output GPIOs (CS, DC, RST)
    let mut ret = configure_output_gpios(&s.cfg);
    if ret != ESP_OK {
        return ret;
    }

    // Configure input GPIO (BUSY)
    ret = configure_input_gpios(&s.cfg);
    if ret != ESP_OK {
        return ret;
    }

    // CS defaults high
    gpio_set(s.cfg.pin_cs, 1);

    // Attach SPI device
    let (spi_handle, spi_ret) = add_spi_device(&s.cfg);
    if spi_ret != ESP_OK {
        return spi_ret;
    }
    s.spi = spi_handle;

    // UC8253 init sequence (full GxEPD2_310_GDEQ031T10 cold-start sequence)
    epaper_hw_reset(); // no-op if RST=-1; delays 20ms

    #[cfg(target_os = "espidf")]
    epd_log!("I (?) epaper: starting init sequence");

    // Soft reset via Panel Setting (bit 0 of PSR byte 1 = RST_N, 0 = reset)
    let mut ret = epaper_send_cmd(CMD_PANEL_SETTING);
    if ret != ESP_OK {
        #[cfg(target_os = "espidf")]
        epd_log!("E (?) epaper: panel_setting cmd failed");
        return fail_init();
    }
    ret = epaper_send_data_byte(0x1E); // PSR: soft reset active
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x0D);
    if ret != ESP_OK { return fail_init(); }
    delay_ms(10);

    // Power setting (VCOM, VGH, VGL, VDH, VDL) — required before CMD_POWER_ON
    ret = epaper_send_cmd(CMD_POWER_SETTING); // 0x01
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x07); // VDS_EN=1, VDG_EN=1
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x17); // VCOM_HV
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x3F); // VDH = +15V
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x3F); // VDL = -15V
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0xF1); // VSHR
    if ret != ESP_OK { return fail_init(); }

    // Booster soft start (boost converter startup — required for power-on)
    ret = epaper_send_cmd(CMD_BOOSTER_SOFT_START); // 0x06
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x17); // Phase A: 40ms, strength 6
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x17); // Phase B: 40ms, strength 6
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x17); // Phase C: strength 6
    if ret != ESP_OK { return fail_init(); }

    // Panel setting (operating config, no soft reset bit)
    ret = epaper_send_cmd(CMD_PANEL_SETTING);
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x1F); // KW mode, BWOTP
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x0D);
    if ret != ESP_OK { return fail_init(); }

    // Resolution setting: 240×320
    ret = epaper_send_cmd(CMD_RESOLUTION_SETTING); // 0x61
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0xF0); // HRES = 240
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x01); // VRES high byte: 320 = 0x140
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x40); // VRES low byte
    if ret != ESP_OK { return fail_init(); }

    // VCOM and data interval
    ret = epaper_send_cmd(CMD_VCOM_DATA_INTERVAL); // 0x50
    if ret != ESP_OK { return fail_init(); }
    ret = epaper_send_data_byte(0x97);
    if ret != ESP_OK { return fail_init(); }

    s.initialized = true;
    s.power_on = false;
    s.first_refresh_done = false;

    // Fill old frame with white, new frame with a test pattern (alternating 8-pixel
    // black/white stripes) so we can visually confirm the display is receiving data.
    std::ptr::write_bytes(fb_old_ptr(), 0xFF, EPD_FB_BYTES);
    {
        let fb = std::slice::from_raw_parts_mut(fb_ptr(), EPD_FB_BYTES);
        for (i, byte) in fb.iter_mut().enumerate() {
            // 8-row stripes: each row is 240/8 = 30 bytes; rows alternate black/white
            let row = (i / 30) % 2;
            *byte = if row == 0 { 0x00 } else { 0xFF };
        }
    }

    #[cfg(target_os = "espidf")]
    epd_log!("I (?) epaper: init complete — test pattern loaded");

    ESP_OK
}

/// Cleanup path during init failure — removes SPI device.
unsafe fn fail_init() -> i32 {
    let s = state_mut();
    #[cfg(target_os = "espidf")]
    if !s.spi.is_null() {
        platform::spi_bus_remove_device(s.spi);
    }
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

    // Static framebuffers (FB, FB_OLD) are not freed — they live for the duration of the program.
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
    let fb = std::slice::from_raw_parts_mut(fb_ptr(), EPD_FB_BYTES);

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

// ── Refresh helpers — #[inline(always)] to keep total CALL8 depth ≤ 7.
//
// Xtensa LX7 has 8 rotating register windows. Each CALL8 advances WindowBase.
// At 8 outstanding CALL8 frames, the next CALL8 wraps around and triggers
// WindowOverflow8, which saves the oldest frame's registers to a save area
// on the callee's stack. If that save area overlaps with the callee's locals
// (LLVM Xtensa backend bug or insufficient frame reservation), the restored
// registers are corrupted → StoreProhibited / InstrFetchProhibited crash.
//
// Observed call chain reaching 8 frames:
//   tk_render_task → gdeq031t10_refresh → refresh_helper →
//   epaper_send_cmd → gpio_set_level → IDF → IDF → ROM  (= 8 levels)
//
// Fix: inline everything into gdeq031t10_refresh so the SPI/GPIO calls
// are only 6 CALL8 levels deep from the FreeRTOS task.

/// Soft-reset sequence: CMD_PANEL_SETTING with reset bit, then 5 ms delay.
#[inline(always)]
unsafe fn refresh_panel_soft_reset() -> i32 {
    let mut err = epaper_send_cmd(CMD_PANEL_SETTING);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x1E);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x0D);
    if err != ESP_OK { return err; }
    delay_ms(5);
    ESP_OK
}

/// Operating-config panel setting (no reset bit).
#[inline(always)]
unsafe fn refresh_panel_config() -> i32 {
    let mut err = epaper_send_cmd(CMD_PANEL_SETTING);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(0x1F);
    if err != ESP_OK { return err; }
    epaper_send_data_byte(0x0D)
}

/// Send the old framebuffer (FB_OLD) via CMD_DATA_START_TRANSMISSION.
///
/// Accesses FB_OLD via its static address (link-time constant, l32r encoded)
/// — immune to Xtensa register window overflow/underflow corruption.
#[inline(always)]
unsafe fn refresh_send_old_fb() -> i32 {
    let err = epaper_send_cmd(CMD_DATA_START_TRANSMISSION);
    if err != ESP_OK { return err; }
    epaper_send_data(fb_old_ptr(), EPD_FB_BYTES)
}

/// Send the current framebuffer (FB) via CMD_NEW_DATA_TRANSMISSION.
///
/// Same rationale as `refresh_send_old_fb` — uses static address.
#[inline(always)]
unsafe fn refresh_send_new_fb() -> i32 {
    let err = epaper_send_cmd(CMD_NEW_DATA_TRANSMISSION);
    if err != ESP_OK { return err; }
    epaper_send_data(fb_ptr(), EPD_FB_BYTES)
}

/// VCOM + data-interval byte, plus optional fast-refresh cascade/temperature.
#[inline(always)]
unsafe fn refresh_vcom_and_fast(fast: bool) -> i32 {
    let mut err = epaper_send_cmd(CMD_VCOM_DATA_INTERVAL);
    if err != ESP_OK { return err; }
    err = epaper_send_data_byte(if fast { 0xD7 } else { 0x97 });
    if err != ESP_OK { return err; }
    if fast {
        err = epaper_send_cmd(CMD_CASCADE);
        if err != ESP_OK { return err; }
        err = epaper_send_data_byte(0x02);
        if err != ESP_OK { return err; }
        err = epaper_send_cmd(CMD_TEMPERATURE_FORCED);
        if err != ESP_OK { return err; }
        err = epaper_send_data_byte(0x79);
        if err != ESP_OK { return err; }
    }
    ESP_OK
}

/// Issue CMD_POWER_ON and wait for BUSY to pulse HIGH then LOW.
/// Falls back to a fixed delay if BUSY never asserts (e.g. BUSY pin wiring issue).
#[inline(always)]
unsafe fn refresh_power_on() -> i32 {
    let err = epaper_send_cmd(CMD_POWER_ON);
    if err != ESP_OK { return err; }

    #[cfg(target_os = "espidf")]
    {
        let mut saw_high = false;
        let mut elapsed: u32 = 0;
        let mut last_b = gpio_read(state().cfg.pin_busy);
        esp_log_write(3, b"epaper\0".as_ptr(),
            b"I (?) epaper: POWER_ON wait start busy=%d\0".as_ptr(), last_b);
        while elapsed < 3_000 {
            delay_ms(1);
            elapsed += 1;
            let b = gpio_read(state().cfg.pin_busy);
            if b != last_b {
                esp_log_write(3, b"epaper\0".as_ptr(),
                    b"I (?) epaper: POWER_ON busy_chg t=%dms b=%d\0".as_ptr(),
                    elapsed, b);
                last_b = b;
            }
            if b != 0 { saw_high = true; }
            if saw_high && b == 0 { break; }
        }
        esp_log_write(3, b"epaper\0".as_ptr(),
            b"I (?) epaper: POWER_ON done saw_high=%d elapsed=%d\0".as_ptr(),
            saw_high as i32, elapsed);
        // Fallback: if BUSY never pulsed, give the controller extra time
        if !saw_high {
            delay_ms(200);
        }
    }
    #[cfg(not(target_os = "espidf"))]
    { let _ = epaper_wait_busy(5_000); }

    ESP_OK
}

/// Issue CMD_DISPLAY_REFRESH and wait for it to complete.
/// Falls back to a minimum fixed delay if BUSY never asserts.
#[inline(always)]
unsafe fn refresh_display_and_wait(fast: bool) -> i32 {
    let err = epaper_send_cmd(CMD_DISPLAY_REFRESH);
    if err != ESP_OK { return err; }

    #[cfg(target_os = "espidf")]
    {
        let min_delay: u32 = if fast { 500 } else { 5_000 };
        let max_delay: u32 = if fast { 5_000 } else { 15_000 };
        delay_ms(min_delay); // guaranteed minimum wait
        let mut elapsed: u32 = min_delay;
        let mut last_busy = gpio_read(state().cfg.pin_busy);
        esp_log_write(3, b"epaper\0".as_ptr(),
            b"I (?) epaper: REFRESH min_delay done busy=%d\0".as_ptr(), last_busy);
        // Continue polling until BUSY goes LOW (or timeout)
        while elapsed < max_delay {
            delay_ms(100);
            elapsed += 100;
            let b = gpio_read(state().cfg.pin_busy);
            if b != last_busy {
                esp_log_write(3, b"epaper\0".as_ptr(),
                    b"I (?) epaper: REFRESH busy_change %dms busy=%d\0".as_ptr(),
                    elapsed, b);
                last_busy = b;
            }
            if b == 0 { break; }
        }
        esp_log_write(3, b"epaper\0".as_ptr(),
            b"I (?) epaper: REFRESH done elapsed=%d\0".as_ptr(), elapsed);
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let timeout = if fast { 5_000 } else { 15_000 };
        let _ = epaper_wait_busy(timeout);
    }

    ESP_OK
}

/// Refresh — send the framebuffer to the panel and trigger a hardware update.
///
/// C-driver-proven sequence for the T-Deck Pro GDEQ031T10:
///   panel soft-reset → panel config → write old/new data → VCOM → POWER_ON → display refresh → POWER_OFF
///
/// The sequence is split into #[inline(always)] helpers to prevent Xtensa register
/// window overflow from corrupting cached function addresses (see helper comments).
pub unsafe extern "C" fn gdeq031t10_refresh() -> i32 {
    #[cfg(target_os = "espidf")]
    epd_log!("I (?) epaper: refresh entry");

    let s = state_mut();
    if !s.initialized {
        #[cfg(target_os = "espidf")]
        epd_log!("I (?) epaper: refresh called but not initialized");
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    {
        let busy_pin = s.cfg.pin_busy;
        let busy_val = gpio_read(busy_pin);
        esp_log_write(3, b"epaper\0".as_ptr(),
            b"I (?) epaper: refresh start busy_pin=%d busy_val=%d\0".as_ptr(),
            busy_pin, busy_val);
    }

    // Use fast mode unless: first refresh, or explicitly set to FULL
    let fast = s.first_refresh_done && s.refresh_mode != HalDisplayRefreshMode::Full;

    // Panel soft reset (as in C driver)
    let err = refresh_panel_soft_reset();
    if err != ESP_OK { return err; }

    // Panel setting (operating config)
    let err = refresh_panel_config();
    if err != ESP_OK { return err; }

    // Write old framebuffer via cmd 0x10 (previous frame) — BEFORE POWER_ON
    let err = refresh_send_old_fb();
    if err != ESP_OK { return err; }

    // Write new framebuffer via cmd 0x13 (current frame)
    let err = refresh_send_new_fb();
    if err != ESP_OK { return err; }

    // VCOM and data interval (+ fast-refresh extras if applicable)
    let err = refresh_vcom_and_fast(fast);
    if err != ESP_OK { return err; }

    // POWER_ON → wait/delay
    let err = refresh_power_on();
    if err != ESP_OK { return err; }

    // Display refresh → wait/delay
    let err = refresh_display_and_wait(fast);
    if err != ESP_OK { return err; }

    // Power off
    let err = epaper_send_cmd(CMD_POWER_OFF);
    if err != ESP_OK { return err; }
    #[cfg(target_os = "espidf")]
    delay_ms(300);
    #[cfg(not(target_os = "espidf"))]
    { let _ = epaper_wait_busy(5_000); }

    s.power_on = false;

    // Save current frame as "old" for next differential refresh
    std::ptr::copy_nonoverlapping(fb_ptr() as *const u8, fb_old_ptr(), EPD_FB_BYTES);

    if !s.first_refresh_done {
        s.first_refresh_done = true;
    }
    // After a full refresh, auto-switch back to fast for subsequent updates
    if !fast {
        s.refresh_mode = HalDisplayRefreshMode::Fast;
    }

    #[cfg(target_os = "espidf")]
    epd_log!("I (?) epaper: refresh done");

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
/// NOTE: Temporarily disabled so the C driver is used for hardware testing.
/// Re-enable by restoring the #[no_mangle] attribute.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[allow(dead_code)]
pub extern "C" fn drv_epaper_gdeq031t10_get_rs() -> *const HalDisplayDriver {
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
        *s = EpaperState::new();
        // Reset static framebuffers to zero
        unsafe {
            std::slice::from_raw_parts_mut(fb_ptr(), EPD_FB_BYTES).fill(0);
            std::slice::from_raw_parts_mut(fb_old_ptr(), EPD_FB_BYTES).fill(0);
        }
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
        let fb = unsafe { std::slice::from_raw_parts(fb_ptr(), EPD_FB_BYTES) };
        assert!(fb.iter().all(|&b| b == 0xFF));

        // Flush a small black area (0x00 = black pixels) in the top-left corner
        let area = HalArea { x1: 0, y1: 0, x2: 7, y2: 0 }; // 8 pixels wide, 1 row
        let data = [0x00u8; 1]; // 8 pixels, all black
        let ret = unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);

        // First byte of fb should now be 0x00 (all black)
        let fb = unsafe { std::slice::from_raw_parts(fb_ptr(), EPD_FB_BYTES) };
        assert_eq!(fb[0], 0x00);
        // The rest should still be white
        assert!(fb[1..].iter().all(|&b| b == 0xFF));

        // Flush a white area back
        let area = HalArea { x1: 0, y1: 0, x2: 7, y2: 0 };
        let data = [0xFFu8; 1];
        let ret = unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(ret, ESP_OK);
        let fb = unsafe { std::slice::from_raw_parts(fb_ptr(), EPD_FB_BYTES) };
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
        assert_eq!(unsafe { *fb_ptr() } & 0x80, 0x00, "pixel (0,0) should be black");

        // Set it back to white (bit 7 = 1)
        let data = [0x80u8]; // MSB = 1 = white
        unsafe { gdeq031t10_flush(&area as *const HalArea, data.as_ptr()) };
        assert_eq!(unsafe { *fb_ptr() } & 0x80, 0x80, "pixel (0,0) should be white");

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
