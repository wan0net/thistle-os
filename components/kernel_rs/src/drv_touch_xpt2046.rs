// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — XPT2046 resistive touch controller driver (Rust)
//
// The XPT2046 is a 12-bit ADC resistive touch controller connected via SPI.
// Unlike capacitive controllers (CST328/CST816), resistive touch requires
// coordinate calibration to map raw ADC values to screen pixels.
//
// Used on the CYD board with a dedicated SPI bus (MOSI=32, MISO=39, CLK=25,
// CS=33, IRQ=36).
//
// SPI protocol:
//   - Control byte: [S=1][A2:A0][MODE=0][SER/DFR=0][PD1:PD0]
//   - Read X: send 0xD0, read 12-bit result
//   - Read Y: send 0x90, read 12-bit result
//   - Read Z1: send 0xB0, read 12-bit result
//   - Read Z2: send 0xC0, read 12-bit result
//   - 12-bit value: ((byte1 << 8) | byte2) >> 3

use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hal_registry::{HalInputCb, HalInputDriver, HalInputEvent, HalInputEventData,
                          HalInputEventType, HalInputTouchData};

// ── ESP error codes ─────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── XPT2046 SPI control bytes ───────────────────────────────────────────────

const XPT2046_CMD_READ_X: u8 = 0xD0;  // Channel 101, 12-bit, differential
const XPT2046_CMD_READ_Y: u8 = 0x90;  // Channel 001, 12-bit, differential
const XPT2046_CMD_READ_Z1: u8 = 0xB0; // Channel 011, 12-bit, differential
const XPT2046_CMD_READ_Z2: u8 = 0xC0; // Channel 100, 12-bit, differential

// Number of samples to average for noise reduction
const SAMPLE_COUNT: usize = 4;

// GPIO constants
const GPIO_NUM_NC: i32 = -1;

// ── SPI bus / device config layouts (mirrors ESP-IDF structs) ───────────────

#[repr(C)]
struct SpiBusConfig {
    mosi_io_num: i32,
    miso_io_num: i32,
    sclk_io_num: i32,
    quadwp_io_num: i32,
    quadhd_io_num: i32,
    data4_io_num: i32,
    data5_io_num: i32,
    data6_io_num: i32,
    data7_io_num: i32,
    max_transfer_sz: i32,
    flags: u32,
    isr_cpu_id: i32,
    intr_flags: i32,
}

#[repr(C)]
struct SpiDeviceInterfaceConfig {
    command_bits: u8,
    address_bits: u8,
    dummy_bits: u8,
    mode: u8,
    clock_source: u32,
    duty_cycle_pos: u16,
    cs_ena_pretrans: u16,
    cs_ena_posttrans: u8,
    clock_speed_hz: i32,
    input_delay_ns: i32,
    spics_io_num: i32,
    flags: u32,
    queue_size: i32,
    pre_cb: Option<unsafe extern "C" fn(*mut c_void)>,
    post_cb: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
struct SpiTransaction {
    flags: u32,
    cmd: u16,
    addr: u64,
    length: usize,     // Total data length, in bits
    rxlength: usize,   // Receive length, in bits
    user: *mut c_void,
    tx_buffer: *const u8,
    rx_buffer: *mut u8,
}

// GPIO config layout (mirrors gpio_config_t)

#[repr(C)]
struct GpioConfig {
    pin_bit_mask: u64,
    mode: u32,
    pull_up_en: u32,
    pull_down_en: u32,
    intr_type: u32,
}

const GPIO_MODE_INPUT: u32 = 1;
const GPIO_PULLUP_ENABLE: u32 = 1;
const GPIO_PULLDOWN_DISABLE: u32 = 0;
const GPIO_INTR_DISABLE: u32 = 0;
const GPIO_INTR_NEGEDGE: u32 = 2;

// ── Configuration struct (C-compatible) ──────────────────────────────────────

/// Configuration passed to `xpt2046_init`. Describes the SPI bus pins,
/// calibration parameters, and axis orientation for the resistive touch panel.
#[repr(C)]
pub struct TouchXpt2046Config {
    /// SPI host number (e.g. SPI2_HOST=1, SPI3_HOST=2).
    pub spi_host: i32,
    /// Chip-select GPIO pin.
    pub pin_cs: i32,
    /// Interrupt / pen-down GPIO pin (active low). -1 to disable.
    pub pin_irq: i32,
    /// SPI MOSI GPIO pin (dedicated bus).
    pub pin_mosi: i32,
    /// SPI MISO GPIO pin.
    pub pin_miso: i32,
    /// SPI clock GPIO pin.
    pub pin_sclk: i32,
    /// Screen width in pixels (for calibration mapping).
    pub max_x: i32,
    /// Screen height in pixels (for calibration mapping).
    pub max_y: i32,
    /// Calibration: minimum raw X ADC value (default 300).
    pub cal_x_min: i32,
    /// Calibration: maximum raw X ADC value (default 3800).
    pub cal_x_max: i32,
    /// Calibration: minimum raw Y ADC value (default 200).
    pub cal_y_min: i32,
    /// Calibration: maximum raw Y ADC value (default 3700).
    pub cal_y_max: i32,
    /// Minimum Z pressure to register a touch (default 100).
    pub pressure_threshold: i32,
    /// Swap X and Y axes after calibration.
    pub swap_xy: bool,
    /// Invert the X axis after calibration.
    pub invert_x: bool,
    /// Invert the Y axis after calibration.
    pub invert_y: bool,
}

// ── Driver state ─────────────────────────────────────────────────────────────

struct TouchState {
    spi_dev: *mut c_void,        // spi_device_handle_t
    cfg: TouchXpt2046Config,
    cb: HalInputCb,
    cb_data: *mut c_void,
    irq_pending: AtomicBool,
    touching: bool,
    last_x: u16,
    last_y: u16,
    initialized: bool,
    pending_down: bool,          // debounce: first valid reading pending confirmation
    pending_x: i32,              // debounce: raw X from first reading
    pending_y: i32,              // debounce: raw Y from first reading
}

// SAFETY: The state is guarded by the single-threaded init / poll contract
// that mirrors the original C drivers.  ISR sets irq_pending via an atomic.
unsafe impl Send for TouchState {}
unsafe impl Sync for TouchState {}

impl TouchState {
    const fn new() -> Self {
        TouchState {
            spi_dev: std::ptr::null_mut(),
            cfg: TouchXpt2046Config {
                spi_host: 1, // SPI2_HOST
                pin_cs: 33,
                pin_irq: 36,
                pin_mosi: 32,
                pin_miso: 39,
                pin_sclk: 25,
                max_x: 320,
                max_y: 240,
                cal_x_min: 300,
                cal_x_max: 3800,
                cal_y_min: 200,
                cal_y_max: 3700,
                pressure_threshold: 100,
                swap_xy: false,
                invert_x: false,
                invert_y: false,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            irq_pending: AtomicBool::new(false),
            touching: false,
            last_x: 0,
            last_y: 0,
            initialized: false,
            pending_down: false,
            pending_x: 0,
            pending_y: 0,
        }
    }
}

static mut S_TOUCH: TouchState = TouchState::new();

// ── Simulator / test injectable touch state ─────────────────────────────────

#[cfg(not(target_os = "espidf"))]
struct SimTouchState {
    raw_x: i32,
    raw_y: i32,
    pressure: i32,
    irq_active: bool,
}

#[cfg(not(target_os = "espidf"))]
static mut SIM_TOUCH: SimTouchState = SimTouchState {
    raw_x: 0,
    raw_y: 0,
    pressure: 0,
    irq_active: false,
};

/// Inject simulated touch data for testing calibration and coordinate mapping.
///
/// # Safety
/// Only available on non-ESP-IDF targets. Must be called from test code with
/// single-threaded test execution.
#[cfg(not(target_os = "espidf"))]
pub unsafe fn inject_touch(raw_x: i32, raw_y: i32, pressure: i32) {
    SIM_TOUCH.raw_x = raw_x;
    SIM_TOUCH.raw_y = raw_y;
    SIM_TOUCH.pressure = pressure;
    SIM_TOUCH.irq_active = pressure > 0;
}

/// Clear simulated touch data.
#[cfg(not(target_os = "espidf"))]
pub unsafe fn clear_injected_touch() {
    SIM_TOUCH.raw_x = 0;
    SIM_TOUCH.raw_y = 0;
    SIM_TOUCH.pressure = 0;
    SIM_TOUCH.irq_active = false;
}

// ── ESP-IDF FFI ─────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
extern "C" {
    fn spi_bus_initialize(host: i32, bus_cfg: *const SpiBusConfig, dma_chan: i32) -> i32;
    fn spi_bus_add_device(
        host: i32,
        dev_cfg: *const SpiDeviceInterfaceConfig,
        handle: *mut *mut c_void,
    ) -> i32;
    fn spi_bus_remove_device(handle: *mut c_void) -> i32;
    fn spi_bus_free(host: i32) -> i32;
    fn spi_device_polling_transmit(handle: *mut c_void, trans: *mut SpiTransaction) -> i32;

    // GPIO (driver/gpio.h)
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_get_level(pin: i32) -> i32;
    fn gpio_isr_handler_add(
        pin: i32,
        handler: unsafe extern "C" fn(*mut c_void),
        arg: *mut c_void,
    ) -> i32;
    fn gpio_isr_handler_remove(pin: i32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;

    // Timer (esp_timer.h)
    fn esp_timer_get_time() -> i64;
}

// ── Stub implementations (simulator / host tests) ────────────────────────────

#[cfg(not(target_os = "espidf"))]
unsafe fn spi_bus_initialize(
    _host: i32,
    _bus_cfg: *const SpiBusConfig,
    _dma_chan: i32,
) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn spi_bus_add_device(
    _host: i32,
    _dev_cfg: *const SpiDeviceInterfaceConfig,
    handle: *mut *mut c_void,
) -> i32 {
    *handle = 1usize as *mut c_void;
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn spi_bus_remove_device(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn spi_bus_free(_host: i32) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn spi_device_polling_transmit(
    _handle: *mut c_void,
    trans: *mut SpiTransaction,
) -> i32 {
    // Read the command byte from tx_buffer to determine which channel is
    // being queried, then return the appropriate simulated value.
    let tx = (*trans).tx_buffer;
    if !tx.is_null() {
        let cmd = *tx;
        let val: i32 = match cmd {
            XPT2046_CMD_READ_X => SIM_TOUCH.raw_x,
            XPT2046_CMD_READ_Y => SIM_TOUCH.raw_y,
            XPT2046_CMD_READ_Z1 => {
                if SIM_TOUCH.pressure > 0 { SIM_TOUCH.pressure } else { 0 }
            }
            XPT2046_CMD_READ_Z2 => {
                // Z2 is inverse-related to pressure; for simulation
                // return 4095 - pressure to make the pressure formula work.
                if SIM_TOUCH.pressure > 0 { 4095 - SIM_TOUCH.pressure } else { 4095 }
            }
            _ => 0,
        };
        // Encode as 12-bit value in upper bits of two bytes: value << 3
        let encoded = (val << 3) & 0xFFFF;
        let rx = (*trans).rx_buffer;
        if !rx.is_null() && (*trans).rxlength >= 16 {
            *rx = ((encoded >> 8) & 0xFF) as u8;
            *rx.add(1) = (encoded & 0xFF) as u8;
        }
    }
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_get_level(_pin: i32) -> i32 {
    // Return 0 (active low = touched) when simulated touch is active.
    if SIM_TOUCH.irq_active { 0 } else { 1 }
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

// ── SPI read helpers ────────────────────────────────────────────────────────

/// Read a single 12-bit ADC value from the XPT2046 by sending a control byte.
///
/// # Safety
/// S_TOUCH.spi_dev must be a valid SPI device handle.
unsafe fn xpt2046_read_channel(cmd: u8) -> i32 {
    let tx_buf: [u8; 1] = [cmd];
    let mut rx_buf: [u8; 2] = [0; 2];

    let mut trans = SpiTransaction {
        flags: 0,
        cmd: 0,
        addr: 0,
        length: 8,       // 1 byte = 8 bits TX
        rxlength: 16,    // 2 bytes = 16 bits RX
        user: std::ptr::null_mut(),
        tx_buffer: tx_buf.as_ptr(),
        rx_buffer: rx_buf.as_mut_ptr(),
    };

    let ret = spi_device_polling_transmit(S_TOUCH.spi_dev, &mut trans);
    if ret != ESP_OK {
        return -1;
    }

    // 12-bit value in upper bits: ((byte0 << 8) | byte1) >> 3
    let raw = (((rx_buf[0] as i32) << 8) | (rx_buf[1] as i32)) >> 3;
    raw & 0xFFF // Mask to 12 bits
}

/// Read raw X and Y positions, interleaved and averaged over SAMPLE_COUNT
/// readings. Samples outside 50..=4045 are rejected as outliers.
///
/// Returns `(-1, -1)` if no valid samples remain after rejection.
///
/// # Safety
/// S_TOUCH.spi_dev must be valid.
unsafe fn read_raw_xy() -> (i32, i32) {
    let mut x_sum: i32 = 0;
    let mut y_sum: i32 = 0;
    let mut x_count: i32 = 0;
    let mut y_count: i32 = 0;

    for _ in 0..SAMPLE_COUNT {
        let x = xpt2046_read_channel(XPT2046_CMD_READ_X);
        let y = xpt2046_read_channel(XPT2046_CMD_READ_Y);
        // Outlier rejection: valid 12-bit range excluding edges
        if x >= 50 && x <= 4045 {
            x_sum += x;
            x_count += 1;
        }
        if y >= 50 && y <= 4045 {
            y_sum += y;
            y_count += 1;
        }
    }

    let x_avg = if x_count > 0 { x_sum / x_count } else { -1 };
    let y_avg = if y_count > 0 { y_sum / y_count } else { -1 };
    (x_avg, y_avg)
}

/// Read touch pressure (Z). Returns a positive value proportional to pressure.
/// Uses the datasheet formula: pressure = Z1 + (4095 - Z2).
/// Higher Z1 with lower Z2 means more pressure.
///
/// # Safety
/// S_TOUCH.spi_dev must be valid.
unsafe fn read_pressure() -> i32 {
    let z1 = xpt2046_read_channel(XPT2046_CMD_READ_Z1);
    let z2 = xpt2046_read_channel(XPT2046_CMD_READ_Z2);
    if z1 < 0 || z2 < 0 {
        return 0;
    }
    (z1 + (4095 - z2)).max(0)
}

/// Check if the touch panel is currently being pressed, using the IRQ pin
/// (active low) or pressure reading as fallback.
///
/// # Safety
/// Reads GPIO state.
unsafe fn is_touched() -> bool {
    if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
        return gpio_get_level(S_TOUCH.cfg.pin_irq) == 0;
    }
    // Fallback: read pressure
    read_pressure() > S_TOUCH.cfg.pressure_threshold
}

// ── Calibration ─────────────────────────────────────────────────────────────

/// Map a raw 12-bit ADC value to a screen coordinate using linear calibration.
///
/// Formula: screen = (raw - cal_min) * screen_size / (cal_max - cal_min)
/// Result is clamped to [0, screen_size - 1].
pub fn calibrate_point(raw: i32, cal_min: i32, cal_max: i32, screen_size: i32) -> i32 {
    if cal_max <= cal_min || screen_size <= 0 {
        return 0;
    }
    let range = cal_max - cal_min;
    let mapped = ((raw - cal_min) as i64 * screen_size as i64 / range as i64) as i32;
    // Clamp to valid screen range
    if mapped < 0 {
        0
    } else if mapped >= screen_size {
        screen_size - 1
    } else {
        mapped
    }
}

/// Apply full calibration pipeline: raw ADC -> calibrated -> swap/invert -> clamp.
///
/// # Safety
/// Reads from S_TOUCH.cfg.
unsafe fn calibrate_xy(raw_x: i32, raw_y: i32) -> (u16, u16) {
    let mut cx = calibrate_point(
        raw_x,
        S_TOUCH.cfg.cal_x_min,
        S_TOUCH.cfg.cal_x_max,
        S_TOUCH.cfg.max_x,
    );
    let mut cy = calibrate_point(
        raw_y,
        S_TOUCH.cfg.cal_y_min,
        S_TOUCH.cfg.cal_y_max,
        S_TOUCH.cfg.max_y,
    );

    // Axis transformations
    if S_TOUCH.cfg.swap_xy {
        std::mem::swap(&mut cx, &mut cy);
        // After swap, clamp to the swapped dimensions
        if cx >= S_TOUCH.cfg.max_x {
            cx = S_TOUCH.cfg.max_x - 1;
        }
        if cy >= S_TOUCH.cfg.max_y {
            cy = S_TOUCH.cfg.max_y - 1;
        }
    }

    if S_TOUCH.cfg.invert_x {
        cx = (S_TOUCH.cfg.max_x - 1) - cx;
    }
    if S_TOUCH.cfg.invert_y {
        cy = (S_TOUCH.cfg.max_y - 1) - cy;
    }

    // Final clamp
    if cx < 0 { cx = 0; }
    if cy < 0 { cy = 0; }
    if cx >= S_TOUCH.cfg.max_x { cx = S_TOUCH.cfg.max_x - 1; }
    if cy >= S_TOUCH.cfg.max_y { cy = S_TOUCH.cfg.max_y - 1; }

    (cx as u16, cy as u16)
}

// ── ISR ──────────────────────────────────────────────────────────────────────

/// GPIO ISR handler — sets `irq_pending` so the poll loop reads SPI.
///
/// # Safety
/// Called from interrupt context.
unsafe extern "C" fn xpt2046_isr_handler(_arg: *mut c_void) {
    S_TOUCH.irq_pending.store(true, Ordering::Relaxed);
}

// ── HAL vtable functions ────────────────────────────────────────────────────

/// Initialise the XPT2046 resistive touch controller.
///
/// `config` must point to a `TouchXpt2046Config`.
///
/// # Safety
/// Called from C via the HAL vtable; `config` must be valid.
unsafe extern "C" fn xpt2046_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if S_TOUCH.initialized {
        return ESP_OK;
    }

    // Copy config
    let src = &*(config as *const TouchXpt2046Config);
    S_TOUCH.cfg.spi_host = src.spi_host;
    S_TOUCH.cfg.pin_cs = src.pin_cs;
    S_TOUCH.cfg.pin_irq = src.pin_irq;
    S_TOUCH.cfg.pin_mosi = src.pin_mosi;
    S_TOUCH.cfg.pin_miso = src.pin_miso;
    S_TOUCH.cfg.pin_sclk = src.pin_sclk;
    S_TOUCH.cfg.max_x = src.max_x;
    S_TOUCH.cfg.max_y = src.max_y;
    S_TOUCH.cfg.cal_x_min = src.cal_x_min;
    S_TOUCH.cfg.cal_x_max = src.cal_x_max;
    S_TOUCH.cfg.cal_y_min = src.cal_y_min;
    S_TOUCH.cfg.cal_y_max = src.cal_y_max;
    S_TOUCH.cfg.pressure_threshold = src.pressure_threshold;
    S_TOUCH.cfg.swap_xy = src.swap_xy;
    S_TOUCH.cfg.invert_x = src.invert_x;
    S_TOUCH.cfg.invert_y = src.invert_y;

    S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
    S_TOUCH.touching = false;
    S_TOUCH.pending_down = false;
    S_TOUCH.pending_x = 0;
    S_TOUCH.pending_y = 0;

    // ── Initialise dedicated SPI bus ────────────────────────────────────
    let bus_cfg = SpiBusConfig {
        mosi_io_num: S_TOUCH.cfg.pin_mosi,
        miso_io_num: S_TOUCH.cfg.pin_miso,
        sclk_io_num: S_TOUCH.cfg.pin_sclk,
        quadwp_io_num: -1,
        quadhd_io_num: -1,
        data4_io_num: -1,
        data5_io_num: -1,
        data6_io_num: -1,
        data7_io_num: -1,
        max_transfer_sz: 32,
        flags: 0,
        isr_cpu_id: 0,
        intr_flags: 0,
    };

    // DMA channel 0 = auto-select on ESP-IDF v5+
    let ret = spi_bus_initialize(S_TOUCH.cfg.spi_host, &bus_cfg, 0);
    // ESP_ERR_INVALID_STATE (0x103) means bus already initialised — that is OK.
    if ret != ESP_OK && ret != ESP_ERR_INVALID_STATE {
        return ret;
    }

    // ── Add SPI device ──────────────────────────────────────────────────
    let dev_cfg = SpiDeviceInterfaceConfig {
        command_bits: 0,
        address_bits: 0,
        dummy_bits: 0,
        mode: 0, // SPI mode 0 (CPOL=0, CPHA=0)
        clock_source: 0,
        duty_cycle_pos: 0,
        cs_ena_pretrans: 0,
        cs_ena_posttrans: 0,
        clock_speed_hz: 1_000_000, // 1 MHz — safe for XPT2046
        input_delay_ns: 0,
        spics_io_num: S_TOUCH.cfg.pin_cs,
        flags: 0,
        queue_size: 1,
        pre_cb: None,
        post_cb: None,
    };

    let ret = spi_bus_add_device(S_TOUCH.cfg.spi_host, &dev_cfg, &mut S_TOUCH.spi_dev);
    if ret != ESP_OK {
        spi_bus_free(S_TOUCH.cfg.spi_host);
        return ret;
    }

    // ── Optional IRQ pin ────────────────────────────────────────────────
    if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
        let irq_cfg = GpioConfig {
            pin_bit_mask: 1u64 << S_TOUCH.cfg.pin_irq,
            mode: GPIO_MODE_INPUT,
            pull_up_en: GPIO_PULLUP_ENABLE,
            pull_down_en: GPIO_PULLDOWN_DISABLE,
            intr_type: GPIO_INTR_NEGEDGE,
        };
        let ret = gpio_config(&irq_cfg);
        if ret != ESP_OK {
            spi_bus_remove_device(S_TOUCH.spi_dev);
            S_TOUCH.spi_dev = std::ptr::null_mut();
            spi_bus_free(S_TOUCH.cfg.spi_host);
            return ret;
        }

        gpio_install_isr_service(0); // idempotent

        let ret = gpio_isr_handler_add(
            S_TOUCH.cfg.pin_irq,
            xpt2046_isr_handler,
            std::ptr::null_mut(),
        );
        if ret != ESP_OK {
            spi_bus_remove_device(S_TOUCH.spi_dev);
            S_TOUCH.spi_dev = std::ptr::null_mut();
            spi_bus_free(S_TOUCH.cfg.spi_host);
            return ret;
        }
    }

    S_TOUCH.initialized = true;
    ESP_OK
}

/// De-initialise the XPT2046 driver and release SPI resources.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn xpt2046_deinit() {
    if !S_TOUCH.initialized {
        return;
    }

    if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
        gpio_isr_handler_remove(S_TOUCH.cfg.pin_irq);
    }

    spi_bus_remove_device(S_TOUCH.spi_dev);
    S_TOUCH.spi_dev = std::ptr::null_mut();
    spi_bus_free(S_TOUCH.cfg.spi_host);

    S_TOUCH.cb = None;
    S_TOUCH.cb_data = std::ptr::null_mut();
    S_TOUCH.touching = false;
    S_TOUCH.initialized = false;
}

/// Register the event callback.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn xpt2046_register_callback(cb: HalInputCb, user_data: *mut c_void) -> i32 {
    S_TOUCH.cb = cb;
    S_TOUCH.cb_data = user_data;
    ESP_OK
}

/// Poll the XPT2046 for touch events and dispatch callbacks.
///
/// # Safety
/// Called from C via the HAL vtable. Must be called periodically.
unsafe extern "C" fn xpt2046_poll() -> i32 {
    if !S_TOUCH.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // If IRQ pin configured and no interrupt pending and not mid-touch, skip.
    if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
        if !S_TOUCH.irq_pending.load(Ordering::Relaxed) && !S_TOUCH.touching {
            return ESP_OK;
        }
    }

    let now_ms = (esp_timer_get_time() / 1000) as u32;

    if is_touched() {
        // Mask IRQ during SPI reads to prevent false triggers from PENIRQ
        // de-asserting during ADC conversion.
        #[cfg(target_os = "espidf")]
        if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
            gpio_isr_handler_remove(S_TOUCH.cfg.pin_irq);
        }

        // Read pressure first to validate touch
        let pressure = read_pressure();

        if pressure < S_TOUCH.cfg.pressure_threshold {
            // Re-enable IRQ before returning
            #[cfg(target_os = "espidf")]
            if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
                gpio_isr_handler_add(
                    S_TOUCH.cfg.pin_irq,
                    xpt2046_isr_handler,
                    std::ptr::null_mut(),
                );
            }

            // Below threshold — treat as no touch
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
            S_TOUCH.pending_down = false;
            S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
            return ESP_OK;
        }

        // Read raw ADC coordinates (interleaved X/Y with outlier rejection)
        let (raw_x, raw_y) = read_raw_xy();

        // Re-enable IRQ after SPI reads complete
        #[cfg(target_os = "espidf")]
        if S_TOUCH.cfg.pin_irq != GPIO_NUM_NC {
            gpio_isr_handler_add(
                S_TOUCH.cfg.pin_irq,
                xpt2046_isr_handler,
                std::ptr::null_mut(),
            );
        }

        if raw_x < 0 || raw_y < 0 {
            S_TOUCH.pending_down = false;
            S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
            return ESP_OK; // All samples rejected or SPI error, skip this cycle
        }

        // Pen-down debounce: require 2 consecutive valid readings before
        // reporting TouchDown. This filters unreliable first-contact reads.
        if !S_TOUCH.touching {
            if !S_TOUCH.pending_down {
                // First valid reading — store but don't emit yet
                S_TOUCH.pending_down = true;
                S_TOUCH.pending_x = raw_x;
                S_TOUCH.pending_y = raw_y;
                S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
                return ESP_OK;
            }
            // Second consecutive valid reading — confirm touch down
            S_TOUCH.pending_down = false;
        }

        // Calibrate and transform
        let (x, y) = calibrate_xy(raw_x, raw_y);

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
        // Not touched — emit TOUCH_UP if previously touching
        S_TOUCH.pending_down = false;
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
    init: Some(xpt2046_init),
    deinit: Some(xpt2046_deinit),
    register_callback: Some(xpt2046_register_callback),
    poll: Some(xpt2046_poll),
    name: b"XPT2046\0".as_ptr() as *const c_char,
    is_touch: true,
};

/// Return a pointer to the XPT2046 HAL input driver vtable.
///
/// # Safety
/// May be called from C.  The returned pointer is valid for the program lifetime.
#[no_mangle]
pub extern "C" fn drv_touch_xpt2046_get_driver() -> *const HalInputDriver {
    &TOUCH_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests.
    unsafe fn reset_state() {
        S_TOUCH.spi_dev = std::ptr::null_mut();
        S_TOUCH.cfg.spi_host = 1;
        S_TOUCH.cfg.pin_cs = 33;
        S_TOUCH.cfg.pin_irq = GPIO_NUM_NC;
        S_TOUCH.cfg.pin_mosi = 32;
        S_TOUCH.cfg.pin_miso = 39;
        S_TOUCH.cfg.pin_sclk = 25;
        S_TOUCH.cfg.max_x = 320;
        S_TOUCH.cfg.max_y = 240;
        S_TOUCH.cfg.cal_x_min = 300;
        S_TOUCH.cfg.cal_x_max = 3800;
        S_TOUCH.cfg.cal_y_min = 200;
        S_TOUCH.cfg.cal_y_max = 3700;
        S_TOUCH.cfg.pressure_threshold = 100;
        S_TOUCH.cfg.swap_xy = false;
        S_TOUCH.cfg.invert_x = false;
        S_TOUCH.cfg.invert_y = false;
        S_TOUCH.cb = None;
        S_TOUCH.cb_data = std::ptr::null_mut();
        S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
        S_TOUCH.touching = false;
        S_TOUCH.last_x = 0;
        S_TOUCH.last_y = 0;
        S_TOUCH.initialized = false;
        S_TOUCH.pending_down = false;
        S_TOUCH.pending_x = 0;
        S_TOUCH.pending_y = 0;
        clear_injected_touch();
    }

    fn default_config() -> TouchXpt2046Config {
        TouchXpt2046Config {
            spi_host: 1,
            pin_cs: 33,
            pin_irq: GPIO_NUM_NC,
            pin_mosi: 32,
            pin_miso: 39,
            pin_sclk: 25,
            max_x: 320,
            max_y: 240,
            cal_x_min: 300,
            cal_x_max: 3800,
            cal_y_min: 200,
            cal_y_max: 3700,
            pressure_threshold: 100,
            swap_xy: false,
            invert_x: false,
            invert_y: false,
        }
    }

    // ── Vtable tests ────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_non_null() {
        let ptr = drv_touch_xpt2046_get_driver();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_vtable_fields() {
        let drv = unsafe { &*drv_touch_xpt2046_get_driver() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.poll.is_some());
        assert!(drv.is_touch);
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_vtable_name() {
        let drv = unsafe { &*drv_touch_xpt2046_get_driver() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "XPT2046");
    }

    // ── Init / deinit tests ─────────────────────────────────────────────

    #[test]
    fn test_init_null_config() {
        unsafe {
            reset_state();
            let ret = xpt2046_init(std::ptr::null());
            assert_eq!(ret, ESP_ERR_INVALID_ARG);
            assert!(!S_TOUCH.initialized);
        }
    }

    #[test]
    fn test_init_and_deinit() {
        unsafe {
            reset_state();
            let cfg = default_config();
            let ret = xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!(S_TOUCH.initialized);
            assert!(!S_TOUCH.spi_dev.is_null());

            xpt2046_deinit();
            assert!(!S_TOUCH.initialized);
            assert!(S_TOUCH.spi_dev.is_null());
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = default_config();
            let ptr = &cfg as *const TouchXpt2046Config as *const c_void;
            assert_eq!(xpt2046_init(ptr), ESP_OK);
            assert_eq!(xpt2046_init(ptr), ESP_OK);
            assert!(S_TOUCH.initialized);
            xpt2046_deinit();
        }
    }

    #[test]
    fn test_poll_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(xpt2046_poll(), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_register_callback() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            unsafe extern "C" fn dummy_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {}

            assert_eq!(
                xpt2046_register_callback(Some(dummy_cb), std::ptr::null_mut()),
                ESP_OK,
            );
            assert!(S_TOUCH.cb.is_some());
            xpt2046_deinit();
        }
    }

    // ── Calibration math tests ──────────────────────────────────────────

    #[test]
    fn test_calibrate_center() {
        // Middle of raw range -> middle of screen
        let x = calibrate_point(2050, 300, 3800, 320);
        let y = calibrate_point(1950, 200, 3700, 240);
        assert_eq!(x, 160); // (2050-300)*320/3500 = 1750*320/3500 = 160
        assert_eq!(y, 120); // (1950-200)*240/3500 = 1750*240/3500 = 120
    }

    #[test]
    fn test_calibrate_origin() {
        // At cal_min -> 0
        let x = calibrate_point(300, 300, 3800, 320);
        let y = calibrate_point(200, 200, 3700, 240);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_calibrate_max() {
        // At cal_max -> screen_size - 1
        let x = calibrate_point(3800, 300, 3800, 320);
        let y = calibrate_point(3700, 200, 3700, 240);
        assert_eq!(x, 319); // clamped to 320-1
        assert_eq!(y, 239); // clamped to 240-1
    }

    #[test]
    fn test_calibrate_below_min_clamps_to_zero() {
        // Below cal_min -> clamped to 0
        let x = calibrate_point(100, 300, 3800, 320);
        assert_eq!(x, 0);
    }

    #[test]
    fn test_calibrate_above_max_clamps() {
        // Above cal_max -> clamped to screen_size - 1
        let x = calibrate_point(4000, 300, 3800, 320);
        assert_eq!(x, 319);
    }

    #[test]
    fn test_calibrate_degenerate_range() {
        // cal_max == cal_min -> returns 0
        assert_eq!(calibrate_point(500, 300, 300, 320), 0);
        // Negative range -> returns 0
        assert_eq!(calibrate_point(500, 400, 300, 320), 0);
        // Zero screen size -> returns 0
        assert_eq!(calibrate_point(500, 300, 3800, 0), 0);
    }

    #[test]
    fn test_calibrate_quarter_points() {
        // 25% of range
        let x = calibrate_point(1175, 300, 3800, 320); // 300 + 875 = 1175
        assert_eq!(x, 80); // 875 * 320 / 3500 = 80

        // 75% of range
        let x = calibrate_point(2925, 300, 3800, 320); // 300 + 2625 = 2925
        assert_eq!(x, 240); // 2625 * 320 / 3500 = 240
    }

    // ── Axis swap / invert tests ────────────────────────────────────────

    #[test]
    fn test_invert_x() {
        unsafe {
            reset_state();
            S_TOUCH.cfg.invert_x = true;
            S_TOUCH.cfg.max_x = 320;
            S_TOUCH.cfg.max_y = 240;
            S_TOUCH.initialized = true;
            S_TOUCH.spi_dev = 1usize as *mut c_void;

            // Inject touch at raw center
            inject_touch(2050, 1950, 500);
            let (x, y) = calibrate_xy(2050, 1950);
            // Normal: x=160, inverted: 319-160 = 159
            assert_eq!(x, 159);
            assert_eq!(y, 120);

            reset_state();
        }
    }

    #[test]
    fn test_invert_y() {
        unsafe {
            reset_state();
            S_TOUCH.cfg.invert_y = true;
            S_TOUCH.cfg.max_x = 320;
            S_TOUCH.cfg.max_y = 240;
            S_TOUCH.initialized = true;

            let (x, y) = calibrate_xy(2050, 1950);
            assert_eq!(x, 160);
            // Normal: y=120, inverted: 239-120 = 119
            assert_eq!(y, 119);

            reset_state();
        }
    }

    #[test]
    fn test_swap_xy() {
        unsafe {
            reset_state();
            S_TOUCH.cfg.swap_xy = true;
            S_TOUCH.cfg.max_x = 320;
            S_TOUCH.cfg.max_y = 240;
            S_TOUCH.initialized = true;

            // Raw X -> calibrates to 160 (for 320 screen)
            // Raw Y -> calibrates to 120 (for 240 screen)
            // After swap: x=120, y=160
            let (x, y) = calibrate_xy(2050, 1950);
            assert_eq!(x, 120);
            assert_eq!(y, 160);

            reset_state();
        }
    }

    #[test]
    fn test_swap_and_invert() {
        unsafe {
            reset_state();
            S_TOUCH.cfg.swap_xy = true;
            S_TOUCH.cfg.invert_x = true;
            S_TOUCH.cfg.invert_y = true;
            S_TOUCH.cfg.max_x = 320;
            S_TOUCH.cfg.max_y = 240;
            S_TOUCH.initialized = true;

            let (x, y) = calibrate_xy(2050, 1950);
            // After swap: x=120, y=160
            // After invert_x: x = 319 - 120 = 199
            // After invert_y: y = 239 - 160 = 79
            assert_eq!(x, 199);
            assert_eq!(y, 79);

            reset_state();
        }
    }

    // ── Pressure threshold tests ────────────────────────────────────────

    #[test]
    fn test_poll_no_touch_no_callback() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );
            // No touch injected -> no callback fired
            clear_injected_touch();
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            xpt2046_deinit();
        }
    }

    #[test]
    fn test_poll_with_touch_fires_callback() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            static mut EVENT_COUNT: i32 = 0;
            static mut LAST_EVENT_TYPE: i32 = -1;
            static mut LAST_X: u16 = 0;
            static mut LAST_Y: u16 = 0;

            unsafe extern "C" fn test_cb(
                event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                EVENT_COUNT += 1;
                LAST_EVENT_TYPE = (*event).event_type as i32;
                LAST_X = (*event).data.touch.x;
                LAST_Y = (*event).data.touch.y;
            }

            EVENT_COUNT = 0;
            xpt2046_register_callback(Some(test_cb), std::ptr::null_mut());

            // Inject touch at center
            inject_touch(2050, 1950, 500);

            // First poll — debounce: pending but no event yet
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            assert_eq!(EVENT_COUNT, 0);

            // Second poll — debounce confirmed, TouchDown emitted
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(S_TOUCH.touching);
            assert_eq!(EVENT_COUNT, 1);
            assert_eq!(LAST_EVENT_TYPE, HalInputEventType::TouchDown as i32);
            assert_eq!(LAST_X, 160);
            assert_eq!(LAST_Y, 120);

            // Release touch
            clear_injected_touch();
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            assert_eq!(EVENT_COUNT, 2);
            assert_eq!(LAST_EVENT_TYPE, HalInputEventType::TouchUp as i32);

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_pressure_below_threshold_ignored() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            // Inject touch with low pressure. With formula pressure = z1 + (4095 - z2),
            // and sim returning z1=p, z2=4095-p, effective pressure = 2*p.
            // threshold=100, so p=40 gives effective=80 which is below threshold.
            inject_touch(2050, 1950, 40);
            assert_eq!(xpt2046_poll(), ESP_OK);
            // IRQ shows touched but pressure is below threshold
            // is_touched() returns true (gpio level 0), but pressure < threshold
            // so the touch should not register.
            assert!(!S_TOUCH.touching);

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_irq_skip_when_not_touching() {
        unsafe {
            reset_state();
            let mut cfg = default_config();
            cfg.pin_irq = 36; // Enable IRQ pin
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            // irq_pending = false, touching = false -> poll returns early
            S_TOUCH.irq_pending.store(false, Ordering::Relaxed);
            S_TOUCH.touching = false;
            clear_injected_touch();
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_coordinate_clamping_at_extremes() {
        // Raw value far below calibration minimum
        assert_eq!(calibrate_point(0, 300, 3800, 320), 0);
        // Raw value far above calibration maximum
        assert_eq!(calibrate_point(4095, 300, 3800, 320), 319);
    }

    #[test]
    fn test_deinit_without_init() {
        unsafe {
            reset_state();
            // Should not panic
            xpt2046_deinit();
            assert!(!S_TOUCH.initialized);
        }
    }

    // ── Outlier rejection tests ────────────────────────────────────────

    #[test]
    fn test_outlier_rejection_filters_edge_samples() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            // Values at exact boundaries (49 and 4046) should be rejected.
            // Inject raw values below 50 — read_raw_xy returns (-1, -1).
            inject_touch(30, 30, 500);
            let (rx, ry) = read_raw_xy();
            assert_eq!(rx, -1);
            assert_eq!(ry, -1);

            // Values at the valid boundary edges (50 and 4045) should pass.
            inject_touch(50, 4045, 500);
            let (rx, ry) = read_raw_xy();
            assert_eq!(rx, 50);
            assert_eq!(ry, 4045);

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_outlier_rejection_valid_center_values() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            // Normal center values should pass through unchanged.
            inject_touch(2050, 1950, 500);
            let (rx, ry) = read_raw_xy();
            assert_eq!(rx, 2050);
            assert_eq!(ry, 1950);

            xpt2046_deinit();
        }
    }

    // ── Pressure formula tests ─────────────────────────────────────────

    #[test]
    fn test_pressure_formula_z1_plus_inverted_z2() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            // Sim: z1 = pressure_val, z2 = 4095 - pressure_val
            // Formula: z1 + (4095 - z2) = p + (4095 - (4095 - p)) = 2*p
            inject_touch(2050, 1950, 500);
            let p = read_pressure();
            assert_eq!(p, 1000); // 500 + (4095 - 3595) = 500 + 500 = 1000

            // Zero pressure
            inject_touch(2050, 1950, 0);
            let p = read_pressure();
            // z1=0, z2=4095: 0 + (4095-4095) = 0
            assert_eq!(p, 0);

            xpt2046_deinit();
        }
    }

    // ── Debounce tests ──────────────────────────────────────────────────

    #[test]
    fn test_debounce_requires_two_polls_for_touch_down() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            static mut EVENT_COUNT: i32 = 0;
            static mut LAST_EVENT_TYPE: i32 = -1;

            unsafe extern "C" fn debounce_cb(
                event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                EVENT_COUNT += 1;
                LAST_EVENT_TYPE = (*event).event_type as i32;
            }

            EVENT_COUNT = 0;
            LAST_EVENT_TYPE = -1;
            xpt2046_register_callback(Some(debounce_cb), std::ptr::null_mut());

            inject_touch(2050, 1950, 500);

            // First poll: pending, no event
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(!S_TOUCH.touching);
            assert_eq!(EVENT_COUNT, 0);
            assert!(S_TOUCH.pending_down);

            // Second poll: confirmed, TouchDown emitted
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert!(S_TOUCH.touching);
            assert_eq!(EVENT_COUNT, 1);
            assert_eq!(LAST_EVENT_TYPE, HalInputEventType::TouchDown as i32);

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_debounce_clears_on_release_before_confirm() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            static mut EVENT_COUNT: i32 = 0;

            unsafe extern "C" fn debounce_clear_cb(
                _event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                EVENT_COUNT += 1;
            }

            EVENT_COUNT = 0;
            xpt2046_register_callback(Some(debounce_clear_cb), std::ptr::null_mut());

            // Touch then release before second poll
            inject_touch(2050, 1950, 500);
            assert_eq!(xpt2046_poll(), ESP_OK); // pending
            assert!(!S_TOUCH.touching);
            assert!(S_TOUCH.pending_down);

            clear_injected_touch();
            assert_eq!(xpt2046_poll(), ESP_OK); // release clears pending
            assert!(!S_TOUCH.touching);
            assert!(!S_TOUCH.pending_down);
            assert_eq!(EVENT_COUNT, 0); // No events fired at all

            xpt2046_deinit();
        }
    }

    #[test]
    fn test_touch_move_after_down_no_debounce() {
        unsafe {
            reset_state();
            let cfg = default_config();
            assert_eq!(
                xpt2046_init(&cfg as *const TouchXpt2046Config as *const c_void),
                ESP_OK,
            );

            static mut EVENT_COUNT: i32 = 0;
            static mut LAST_EVENT_TYPE: i32 = -1;

            unsafe extern "C" fn move_cb(
                event: *const HalInputEvent,
                _user_data: *mut c_void,
            ) {
                EVENT_COUNT += 1;
                LAST_EVENT_TYPE = (*event).event_type as i32;
            }

            EVENT_COUNT = 0;
            xpt2046_register_callback(Some(move_cb), std::ptr::null_mut());

            inject_touch(2050, 1950, 500);
            assert_eq!(xpt2046_poll(), ESP_OK); // debounce pending
            assert_eq!(xpt2046_poll(), ESP_OK); // TouchDown
            assert_eq!(EVENT_COUNT, 1);
            assert_eq!(LAST_EVENT_TYPE, HalInputEventType::TouchDown as i32);

            // Move — no debounce needed, fires immediately
            inject_touch(2100, 2000, 500);
            assert_eq!(xpt2046_poll(), ESP_OK);
            assert_eq!(EVENT_COUNT, 2);
            assert_eq!(LAST_EVENT_TYPE, HalInputEventType::TouchMove as i32);

            xpt2046_deinit();
        }
    }
}
