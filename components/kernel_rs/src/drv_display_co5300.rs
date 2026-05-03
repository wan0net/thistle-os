// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CO5300 QSPI AMOLED display driver (Rust)
//
// Rebuilds the watch AMOLED path used by vendor Arduino_GFX examples:
// Arduino_ESP32QSPI + Arduino_CO5300. The command sequence mirrors
// Arduino_GFX's CO5300 init flow while exposing ThistleOS' HAL display vtable.

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalArea, HalDisplayDriver, HalDisplayRefreshMode, HalDisplayType};

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

const GPIO_NUM_NC: i32 = -1;
const DEFAULT_WIDTH: u16 = 410;
const DEFAULT_HEIGHT: u16 = 502;
const DEFAULT_SPI_CLOCK_HZ: i32 = 40_000_000;

const CO5300_SWRESET: u8 = 0x01;
const CO5300_SLPIN: u8 = 0x10;
const CO5300_SLPOUT: u8 = 0x11;
const CO5300_INVOFF: u8 = 0x20;
const CO5300_DISPOFF: u8 = 0x28;
const CO5300_DISPON: u8 = 0x29;
const CO5300_CASET: u8 = 0x2A;
const CO5300_PASET: u8 = 0x2B;
const CO5300_RAMWR: u8 = 0x2C;
const CO5300_MADCTL: u8 = 0x36;
const CO5300_PIXFMT: u8 = 0x3A;
const CO5300_BRIGHTNESS: u8 = 0x51;
const CO5300_CTRL_DISPLAY1: u8 = 0x53;
const CO5300_CE: u8 = 0x58;
const CO5300_HBM_BRIGHTNESS: u8 = 0x63;
const CO5300_SPI_MODE_CTL: u8 = 0xC4;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DisplayCo5300Config {
    pub spi_host: i32,
    pub pin_cs: i32,
    pub pin_sclk: i32,
    pub pin_sdio0: i32,
    pub pin_sdio1: i32,
    pub pin_sdio2: i32,
    pub pin_sdio3: i32,
    pub pin_rst: i32,
    pub pin_te: i32,
    pub pin_power: i32,
    pub spi_clock_hz: i32,
    pub width: u16,
    pub height: u16,
    pub x_offset: u16,
    pub y_offset: u16,
    pub flags: u32,
}

struct DisplayState {
    cfg: DisplayCo5300Config,
    io: *mut c_void,
    initialized: bool,
    brightness: u8,
}

unsafe impl Send for DisplayState {}
unsafe impl Sync for DisplayState {}

impl DisplayState {
    const fn new() -> Self {
        DisplayState {
            cfg: DisplayCo5300Config {
                spi_host: 0,
                pin_cs: GPIO_NUM_NC,
                pin_sclk: GPIO_NUM_NC,
                pin_sdio0: GPIO_NUM_NC,
                pin_sdio1: GPIO_NUM_NC,
                pin_sdio2: GPIO_NUM_NC,
                pin_sdio3: GPIO_NUM_NC,
                pin_rst: GPIO_NUM_NC,
                pin_te: GPIO_NUM_NC,
                pin_power: GPIO_NUM_NC,
                spi_clock_hz: 0,
                width: 0,
                height: 0,
                x_offset: 0,
                y_offset: 0,
                flags: 0,
            },
            io: std::ptr::null_mut(),
            initialized: false,
            brightness: 0xD0,
        }
    }
}

struct GlobalState {
    inner: UnsafeCell<DisplayState>,
}

unsafe impl Sync for GlobalState {}

static STATE: GlobalState = GlobalState {
    inner: UnsafeCell::new(DisplayState::new()),
};

fn state() -> &'static DisplayState {
    unsafe { &*STATE.inner.get() }
}

fn state_mut() -> &'static mut DisplayState {
    unsafe { &mut *STATE.inner.get() }
}

#[cfg(target_os = "espidf")]
mod platform {
    use std::os::raw::c_void;

    #[repr(C)]
    pub struct GpioConfig {
        pub pin_bit_mask: u64,
        pub mode: u32,
        pub pull_up_en: u32,
        pub pull_down_en: u32,
        pub intr_type: u32,
    }

    #[repr(C)]
    pub struct SpiBusConfig {
        pub mosi_io_num: i32,
        pub miso_io_num: i32,
        pub sclk_io_num: i32,
        pub quadwp_io_num: i32,
        pub quadhd_io_num: i32,
        pub max_transfer_sz: i32,
        pub flags: u32,
        pub intr_flags: i32,
    }

    #[repr(C)]
    pub struct PanelIoSpiConfig {
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

    extern "C" {
        pub fn spi_bus_initialize(host: i32, cfg: *const SpiBusConfig, dma_chan: i32) -> i32;
        pub fn esp_lcd_new_panel_io_spi(
            bus: *mut c_void,
            cfg: *const PanelIoSpiConfig,
            out_io: *mut *mut c_void,
        ) -> i32;
        pub fn esp_lcd_panel_io_del(io: *mut c_void) -> i32;
        pub fn esp_lcd_panel_io_tx_param(
            io: *mut c_void,
            lcd_cmd: i32,
            param: *const c_void,
            param_size: usize,
        ) -> i32;
        pub fn esp_lcd_panel_io_tx_color(
            io: *mut c_void,
            lcd_cmd: i32,
            color: *const c_void,
            color_size: usize,
        ) -> i32;
        pub fn gpio_config(cfg: *const GpioConfig) -> i32;
        pub fn gpio_set_level(pin: i32, level: u32) -> i32;
        pub fn vTaskDelay(ticks: u32);
    }
}

#[cfg(not(target_os = "espidf"))]
mod platform {
    use std::os::raw::c_void;

    #[repr(C)]
    pub struct GpioConfig {
        pub pin_bit_mask: u64,
        pub mode: u32,
        pub pull_up_en: u32,
        pub pull_down_en: u32,
        pub intr_type: u32,
    }

    #[repr(C)]
    pub struct SpiBusConfig {
        pub mosi_io_num: i32,
        pub miso_io_num: i32,
        pub sclk_io_num: i32,
        pub quadwp_io_num: i32,
        pub quadhd_io_num: i32,
        pub max_transfer_sz: i32,
        pub flags: u32,
        pub intr_flags: i32,
    }

    #[repr(C)]
    pub struct PanelIoSpiConfig {
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

    pub unsafe fn spi_bus_initialize(_host: i32, _cfg: *const SpiBusConfig, _dma_chan: i32) -> i32 { 0 }
    pub unsafe fn esp_lcd_new_panel_io_spi(
        _bus: *mut c_void,
        _cfg: *const PanelIoSpiConfig,
        out_io: *mut *mut c_void,
    ) -> i32 {
        *out_io = 1usize as *mut c_void;
        0
    }
    pub unsafe fn esp_lcd_panel_io_del(_io: *mut c_void) -> i32 { 0 }
    pub unsafe fn esp_lcd_panel_io_tx_param(
        _io: *mut c_void,
        _lcd_cmd: i32,
        _param: *const c_void,
        _param_size: usize,
    ) -> i32 { 0 }
    pub unsafe fn esp_lcd_panel_io_tx_color(
        _io: *mut c_void,
        _lcd_cmd: i32,
        _color: *const c_void,
        _color_size: usize,
    ) -> i32 { 0 }
    pub unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 { 0 }
    pub unsafe fn gpio_set_level(_pin: i32, _level: u32) -> i32 { 0 }
    pub unsafe fn vTaskDelay(_ticks: u32) {}
}

fn ms_to_ticks(ms: u32) -> u32 {
    ms
}

unsafe fn delay_ms(ms: u32) {
    platform::vTaskDelay(ms_to_ticks(ms));
}

unsafe fn tx_cmd(cmd: u8) -> i32 {
    platform::esp_lcd_panel_io_tx_param(state().io, cmd as i32, std::ptr::null(), 0)
}

unsafe fn tx_u8(cmd: u8, val: u8) -> i32 {
    platform::esp_lcd_panel_io_tx_param(
        state().io,
        cmd as i32,
        (&val as *const u8).cast::<c_void>(),
        1,
    )
}

unsafe fn tx_u16_pair(cmd: u8, start: u16, end: u16) -> i32 {
    let data = [
        (start >> 8) as u8,
        start as u8,
        (end >> 8) as u8,
        end as u8,
    ];
    platform::esp_lcd_panel_io_tx_param(
        state().io,
        cmd as i32,
        data.as_ptr().cast::<c_void>(),
        data.len(),
    )
}

unsafe fn hw_reset_or_sw_reset() -> i32 {
    let cfg = state().cfg;
    if cfg.pin_rst != GPIO_NUM_NC {
        let gpio = platform::GpioConfig {
            pin_bit_mask: 1u64 << cfg.pin_rst,
            mode: 2,
            pull_up_en: 0,
            pull_down_en: 0,
            intr_type: 0,
        };
        let ret = platform::gpio_config(&gpio);
        if ret != ESP_OK {
            return ret;
        }
        platform::gpio_set_level(cfg.pin_rst, 1);
        delay_ms(10);
        platform::gpio_set_level(cfg.pin_rst, 0);
        delay_ms(200);
        platform::gpio_set_level(cfg.pin_rst, 1);
        delay_ms(200);
        ESP_OK
    } else {
        let ret = tx_cmd(CO5300_SWRESET);
        delay_ms(200);
        ret
    }
}

unsafe fn co5300_init_sequence() -> i32 {
    let mut ret = hw_reset_or_sw_reset();
    if ret != ESP_OK { return ret; }

    ret = tx_cmd(CO5300_SLPOUT);
    if ret != ESP_OK { return ret; }
    delay_ms(120);

    for (cmd, val) in [
        (0xFE, 0x00),
        (CO5300_SPI_MODE_CTL, 0x80),
        (CO5300_MADCTL, 0x00),
        (CO5300_PIXFMT, 0x55),
        (CO5300_CTRL_DISPLAY1, 0x20),
        (CO5300_HBM_BRIGHTNESS, 0xFF),
        (CO5300_BRIGHTNESS, state().brightness),
        (CO5300_CE, 0x00),
    ] {
        ret = tx_u8(cmd, val);
        if ret != ESP_OK { return ret; }
    }

    ret = tx_cmd(CO5300_DISPON);
    if ret != ESP_OK { return ret; }
    ret = tx_cmd(CO5300_INVOFF);
    if ret != ESP_OK { return ret; }
    delay_ms(10);
    ESP_OK
}

unsafe extern "C" fn co5300_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let s = state_mut();
    if s.initialized {
        return ESP_OK;
    }

    s.cfg = *(config as *const DisplayCo5300Config);
    if s.cfg.width == 0 { s.cfg.width = DEFAULT_WIDTH; }
    if s.cfg.height == 0 { s.cfg.height = DEFAULT_HEIGHT; }
    if s.cfg.spi_clock_hz <= 0 { s.cfg.spi_clock_hz = DEFAULT_SPI_CLOCK_HZ; }

    if s.cfg.pin_power != GPIO_NUM_NC {
        let gpio = platform::GpioConfig {
            pin_bit_mask: 1u64 << s.cfg.pin_power,
            mode: 2,
            pull_up_en: 0,
            pull_down_en: 0,
            intr_type: 0,
        };
        let ret = platform::gpio_config(&gpio);
        if ret != ESP_OK { return ret; }
        platform::gpio_set_level(s.cfg.pin_power, 1);
        delay_ms(20);
    }

    let bus_cfg = platform::SpiBusConfig {
        mosi_io_num: s.cfg.pin_sdio0,
        miso_io_num: s.cfg.pin_sdio1,
        sclk_io_num: s.cfg.pin_sclk,
        quadwp_io_num: s.cfg.pin_sdio2,
        quadhd_io_num: s.cfg.pin_sdio3,
        max_transfer_sz: (s.cfg.width as i32) * 80 * 2,
        flags: 0,
        intr_flags: 0,
    };
    let _ = platform::spi_bus_initialize(s.cfg.spi_host, &bus_cfg, 1);

    let io_cfg = platform::PanelIoSpiConfig {
        dc_gpio_num: GPIO_NUM_NC,
        cs_gpio_num: s.cfg.pin_cs,
        pclk_hz: s.cfg.spi_clock_hz as u32,
        lcd_cmd_bits: 8,
        lcd_param_bits: 8,
        spi_mode: 0,
        trans_queue_depth: 10,
        on_color_trans_done: std::ptr::null(),
        user_ctx: std::ptr::null_mut(),
        flags: 1 << 6,
    };

    let ret = platform::esp_lcd_new_panel_io_spi(
        s.cfg.spi_host as usize as *mut c_void,
        &io_cfg,
        &mut s.io,
    );
    if ret != ESP_OK {
        s.io = std::ptr::null_mut();
        return ret;
    }

    let ret = co5300_init_sequence();
    if ret != ESP_OK {
        platform::esp_lcd_panel_io_del(s.io);
        s.io = std::ptr::null_mut();
        return ret;
    }

    s.initialized = true;
    ESP_OK
}

unsafe extern "C" fn co5300_deinit() {
    let s = state_mut();
    if !s.initialized {
        return;
    }
    let _ = tx_cmd(CO5300_DISPOFF);
    if !s.io.is_null() {
        platform::esp_lcd_panel_io_del(s.io);
    }
    s.io = std::ptr::null_mut();
    s.initialized = false;
}

unsafe extern "C" fn co5300_flush(area: *const HalArea, color_data: *const u8) -> i32 {
    if !state().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if area.is_null() || color_data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let a = *area;
    if a.x2 < a.x1 || a.y2 < a.y1 {
        return ESP_ERR_INVALID_ARG;
    }

    let cfg = state().cfg;
    let x1 = a.x1.saturating_add(cfg.x_offset);
    let x2 = a.x2.saturating_add(cfg.x_offset);
    let y1 = a.y1.saturating_add(cfg.y_offset);
    let y2 = a.y2.saturating_add(cfg.y_offset);

    let mut ret = tx_u16_pair(CO5300_CASET, x1, x2);
    if ret != ESP_OK { return ret; }
    ret = tx_u16_pair(CO5300_PASET, y1, y2);
    if ret != ESP_OK { return ret; }

    let pixels = ((a.x2 - a.x1 + 1) as usize) * ((a.y2 - a.y1 + 1) as usize);
    platform::esp_lcd_panel_io_tx_color(
        state().io,
        CO5300_RAMWR as i32,
        color_data.cast::<c_void>(),
        pixels * 2,
    )
}

unsafe extern "C" fn co5300_refresh() -> i32 {
    if state().initialized { ESP_OK } else { ESP_ERR_INVALID_STATE }
}

unsafe extern "C" fn co5300_set_brightness(percent: u8) -> i32 {
    if !state().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    let val = ((percent as u16 * 255) / 100).min(255) as u8;
    state_mut().brightness = val;
    tx_u8(CO5300_BRIGHTNESS, val)
}

unsafe extern "C" fn co5300_sleep(enter: bool) -> i32 {
    if !state().initialized {
        return ESP_ERR_INVALID_STATE;
    }
    let ret = if enter {
        let r = tx_cmd(CO5300_DISPOFF);
        if r != ESP_OK { return r; }
        tx_cmd(CO5300_SLPIN)
    } else {
        let r = tx_cmd(CO5300_SLPOUT);
        delay_ms(120);
        if r != ESP_OK { return r; }
        tx_cmd(CO5300_DISPON)
    };
    delay_ms(120);
    ret
}

unsafe extern "C" fn co5300_set_refresh_mode(_mode: HalDisplayRefreshMode) -> i32 {
    if state().initialized { ESP_OK } else { ESP_FAIL }
}

static CO5300_DRIVER: HalDisplayDriver = HalDisplayDriver {
    init: Some(co5300_init),
    deinit: Some(co5300_deinit),
    flush: Some(co5300_flush),
    refresh: Some(co5300_refresh),
    set_brightness: Some(co5300_set_brightness),
    sleep: Some(co5300_sleep),
    set_refresh_mode: Some(co5300_set_refresh_mode),
    width: DEFAULT_WIDTH,
    height: DEFAULT_HEIGHT,
    display_type: HalDisplayType::Lcd,
    name: b"CO5300 AMOLED\0".as_ptr() as *const c_char,
};

#[no_mangle]
pub extern "C" fn drv_display_co5300_get() -> *const HalDisplayDriver {
    &CO5300_DRIVER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_geometry_matches_watch_panel() {
        assert_eq!(CO5300_DRIVER.width, 410);
        assert_eq!(CO5300_DRIVER.height, 502);
    }

    #[test]
    fn config_layout_has_expected_defaults() {
        let cfg = DisplayState::new().cfg;
        assert_eq!(cfg.pin_cs, GPIO_NUM_NC);
        assert_eq!(cfg.spi_clock_hz, 0);
    }
}
