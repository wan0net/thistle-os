// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TP4065B power/battery driver (Rust)
//
// Rust port of components/drv_power_tp4065b/src/drv_power_tp4065b.c.
//
// The TP4065B is a single-cell LiPo charger.  Battery voltage is read via an
// ADC channel with a 2:1 voltage divider in front of it.  Charge status is
// reported on an open-drain GPIO pin that is pulled low while charging.
//
// Hardware path: ADC oneshot API (ESP-IDF ≥ 5.0) + GPIO input.
// On non-ESP32 targets (host tests, simulator) all hardware calls are stubbed.

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalPowerDriver, HalPowerInfo, HalPowerState};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Driver constants ─────────────────────────────────────────────────────────

/// Number of ADC samples to average for stable voltage readings.
const ADC_SAMPLES: usize = 8;

/// Voltage divider ratio: battery voltage is halved before the ADC pin.
const VDIV_RATIO: u32 = 2;

/// Absolute maximum plausible battery voltage (noise / floating-pin guard).
const BAT_MV_MAX: u32 = 4300;

// ── LiPo 1S discharge curve ──────────────────────────────────────────────────
//
// 11-point table copied verbatim from the C driver.  Each row is
// (voltage_mv, battery_percent).  The table is sorted descending by voltage.

struct LipoPoint {
    mv: u16,
    pct: u8,
}

static LIPO_CURVE: [LipoPoint; 11] = [
    LipoPoint { mv: 4200, pct: 100 },
    LipoPoint { mv: 4060, pct:  90 },
    LipoPoint { mv: 3980, pct:  80 },
    LipoPoint { mv: 3920, pct:  70 },
    LipoPoint { mv: 3870, pct:  60 },
    LipoPoint { mv: 3820, pct:  50 },
    LipoPoint { mv: 3750, pct:  40 },
    LipoPoint { mv: 3700, pct:  30 },
    LipoPoint { mv: 3620, pct:  20 },
    LipoPoint { mv: 3500, pct:  10 },
    LipoPoint { mv: 3000, pct:   0 },
];

/// Map a calibrated battery voltage (mV) to a charge percentage using linear
/// interpolation of the LiPo discharge curve.
///
/// This is pure Rust and has no hardware dependency — it is safe to test on
/// any host target.
pub fn voltage_to_percent(mv: u16) -> u8 {
    // Above the top breakpoint → 100 %
    if mv >= LIPO_CURVE[0].mv {
        return 100;
    }
    // Below the bottom breakpoint → 0 %
    if mv <= LIPO_CURVE[LIPO_CURVE.len() - 1].mv {
        return 0;
    }

    // Find the two surrounding breakpoints and interpolate linearly.
    for i in 0..(LIPO_CURVE.len() - 1) {
        let upper = &LIPO_CURVE[i];
        let lower = &LIPO_CURVE[i + 1];

        if mv <= upper.mv && mv >= lower.mv {
            let mv_range = (upper.mv - lower.mv) as u32;
            let pct_range = (upper.pct - lower.pct) as u32;
            let mv_above_lower = (mv - lower.mv) as u32;
            // Weighted interpolation, rounded to nearest integer — mirrors the
            // C driver's rounding formula exactly.
            let pct = lower.pct
                + ((mv_above_lower * pct_range + mv_range / 2) / mv_range) as u8;
            return pct;
        }
    }

    0
}

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────
//
// Only declared when building for the real target.  The stub section below
// provides equivalent no-op / synthetic implementations for host tests and the
// SDL2 simulator.

/// gpio_config_t layout (mirrors driver/gpio.h).
#[repr(C)]
struct GpioConfig {
    pin_bit_mask: u64,
    mode: u32,
    pull_up_en: u32,
    pull_down_en: u32,
    intr_type: u32,
}

// GPIO constants from driver/gpio.h
const GPIO_MODE_INPUT: u32 = 1;
const GPIO_PULLUP_ENABLE: u32 = 1;
const GPIO_PULLDOWN_DISABLE: u32 = 0;
const GPIO_INTR_DISABLE: u32 = 0;

#[cfg(target_os = "espidf")]
extern "C" {
    fn adc_oneshot_new_unit(cfg: *const c_void, handle: *mut *mut c_void) -> i32;
    fn adc_oneshot_del_unit(handle: *mut c_void) -> i32;
    fn adc_oneshot_config_channel(handle: *mut c_void, channel: i32, cfg: *const c_void) -> i32;
    fn adc_oneshot_read(handle: *mut c_void, channel: i32, raw: *mut i32) -> i32;
    fn adc_cali_raw_to_voltage(handle: *mut c_void, raw: i32, voltage: *mut i32) -> i32;
    fn gpio_config(cfg: *const GpioConfig) -> i32;
    fn gpio_get_level(pin: u32) -> i32;
}

// ── Stub implementations (simulator / host tests) ────────────────────────────
//
// The stub ADC returns a fixed raw value that corresponds to roughly 3700 mV
// after divider correction (raw ≈ 2300 at 3.3 V Vref, 12-bit).  This gives
// tests a predictable, non-zero voltage to exercise the percent lookup.

/// Synthetic mid-range raw ADC value used by stubs.  Resolves to:
///   adc_mv  ≈ 2300 * 3300 / 4095 ≈ 1853 mV (at ADC pin)
///   bat_mv  = 1853 * 2           = 3706 mV  → ~31 %
const STUB_RAW_VALUE: i32 = 2300;

#[cfg(not(target_os = "espidf"))]
unsafe fn adc_oneshot_new_unit(
    _cfg: *const c_void,
    handle: *mut *mut c_void,
) -> i32 {
    *handle = 1usize as *mut c_void; // non-null sentinel
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn adc_oneshot_del_unit(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn adc_oneshot_config_channel(
    _handle: *mut c_void,
    _channel: i32,
    _cfg: *const c_void,
) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn adc_oneshot_read(
    _handle: *mut c_void,
    _channel: i32,
    raw: *mut i32,
) -> i32 {
    *raw = STUB_RAW_VALUE;
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn adc_cali_raw_to_voltage(
    _handle: *mut c_void,
    raw: i32,
    voltage: *mut i32,
) -> i32 {
    // Approximate the calibration curve: linear, Vref 3300 mV, 12-bit.
    *voltage = (raw as i64 * 3300 / 4095) as i32;
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_config(_cfg: *const GpioConfig) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn gpio_get_level(_pin: u32) -> i32 {
    // Not charging by default in the simulator.
    1
}

// ── ADC / calibration init helpers ───────────────────────────────────────────
//
// On a real ESP32-S3 the calibration scheme (curve-fitting vs line-fitting) is
// selected at compile time via Kconfig macros.  In Rust we can't query those
// macros directly, so we call the curve-fitting function first and fall back to
// line-fitting.  On non-ESP32 builds the stubs above handle both paths
// transparently because `adc_cali_raw_to_voltage` is all we call afterwards.

/// Try to create an ADC calibration handle.
///
/// Returns `(handle, success)`.  On failure the handle is null and the caller
/// should use the raw-approximation path.
///
/// # Safety
/// Calls ESP-IDF ADC calibration APIs.
#[cfg(target_os = "espidf")]
unsafe fn cali_init(unit: i32, channel: i32, atten: i32) -> (*mut c_void, bool) {
    // adc_cali_curve_fitting_config_t layout (ESP-IDF ≥ 5.0):
    //   unit_id  : u32
    //   chan     : u32
    //   atten    : u32
    //   bitwidth : u32
    #[repr(C)]
    struct CurveFitCfg { unit_id: u32, chan: u32, atten: u32, bitwidth: u32 }

    extern "C" {
        fn adc_cali_create_scheme_curve_fitting(
            cfg: *const CurveFitCfg,
            out: *mut *mut c_void,
        ) -> i32;
        fn adc_cali_create_scheme_line_fitting(
            cfg: *const LineFitCfg,
            out: *mut *mut c_void,
        ) -> i32;
    }

    // adc_cali_line_fitting_config_t layout:
    //   unit_id  : u32
    //   atten    : u32
    //   bitwidth : u32
    #[repr(C)]
    struct LineFitCfg { unit_id: u32, atten: u32, bitwidth: u32 }

    let mut handle: *mut c_void = std::ptr::null_mut();

    let curve_cfg = CurveFitCfg {
        unit_id:  unit as u32,
        chan:     channel as u32,
        atten:    atten as u32,
        bitwidth: 12, // ADC_BITWIDTH_12
    };
    if adc_cali_create_scheme_curve_fitting(&curve_cfg, &mut handle) == ESP_OK {
        return (handle, true);
    }

    let line_cfg = LineFitCfg {
        unit_id:  unit as u32,
        atten:    atten as u32,
        bitwidth: 12,
    };
    if adc_cali_create_scheme_line_fitting(&line_cfg, &mut handle) == ESP_OK {
        return (handle, true);
    }

    (std::ptr::null_mut(), false)
}

/// Delete an ADC calibration handle created by `cali_init`.
///
/// # Safety
/// `handle` must be a valid calibration handle or null.
#[cfg(target_os = "espidf")]
unsafe fn cali_deinit(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }

    extern "C" {
        fn adc_cali_delete_scheme_curve_fitting(handle: *mut c_void) -> i32;
        fn adc_cali_delete_scheme_line_fitting(handle: *mut c_void) -> i32;
    }

    // Try curve-fitting first; if it fails it wasn't that scheme so try line.
    if adc_cali_delete_scheme_curve_fitting(handle) != ESP_OK {
        adc_cali_delete_scheme_line_fitting(handle);
    }
}

// On non-ESP32 targets calibration is handled entirely by the stubs above.
#[cfg(not(target_os = "espidf"))]
unsafe fn cali_init(_unit: i32, _channel: i32, _atten: i32) -> (*mut c_void, bool) {
    (1usize as *mut c_void, true) // pretend calibration succeeded
}

#[cfg(not(target_os = "espidf"))]
unsafe fn cali_deinit(_handle: *mut c_void) {}

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible configuration for the TP4065B driver.
///
/// Must match `power_tp4065b_config_t` in the C header.
#[repr(C)]
pub struct PowerTp4065bConfig {
    /// ADC channel connected to the battery voltage divider (`adc_channel_t`).
    pub adc_channel: i32,
    /// GPIO number of the TP4065B CHRG pin (`gpio_num_t`).
    /// The pin is open-drain, pulled low while charging.
    pub pin_charge_status: i32,
}

// SAFETY: Config holds only primitive integers.
unsafe impl Send for PowerTp4065bConfig {}
unsafe impl Sync for PowerTp4065bConfig {}

// ── Driver state ─────────────────────────────────────────────────────────────

struct PowerState {
    cfg: PowerTp4065bConfig,
    adc_handle: *mut c_void,
    cali_handle: *mut c_void,
    initialized: bool,
    has_calibration: bool,
}

// SAFETY: The driver is initialised and used from the single-threaded HAL
// init / read path, mirroring the C driver's static state model.
unsafe impl Send for PowerState {}
unsafe impl Sync for PowerState {}

impl PowerState {
    const fn new() -> Self {
        PowerState {
            cfg: PowerTp4065bConfig {
                adc_channel: 0,
                pin_charge_status: -1,
            },
            adc_handle: std::ptr::null_mut(),
            cali_handle: std::ptr::null_mut(),
            initialized: false,
            has_calibration: false,
        }
    }
}

static mut S_PWR: PowerState = PowerState::new();

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Read the battery voltage in millivolts.
///
/// Averages `ADC_SAMPLES` readings, applies calibration if available,
/// then corrects for the 2:1 voltage divider.  Returns 0 on any error.
///
/// # Safety
/// Must be called after a successful `tp4065b_init`.
unsafe fn read_battery_mv() -> u16 {
    let pwr = &mut *(&raw mut S_PWR);

    if !pwr.initialized {
        return 0;
    }

    // --- 8-sample average ---
    let mut sum: i32 = 0;
    for _ in 0..ADC_SAMPLES {
        let mut raw: i32 = 0;
        let err = adc_oneshot_read(pwr.adc_handle, pwr.cfg.adc_channel, &mut raw);
        if err != ESP_OK {
            return 0;
        }
        sum += raw;
    }
    let avg_raw = sum / ADC_SAMPLES as i32;

    // --- Calibrated or raw mV ---
    let mut adc_mv: i32 = 0;

    if pwr.has_calibration {
        let err = adc_cali_raw_to_voltage(pwr.cali_handle, avg_raw, &mut adc_mv);
        if err != ESP_OK {
            // Calibration failed at runtime — fall back to raw approximation.
            pwr.has_calibration = false;
        }
    }

    if !pwr.has_calibration {
        // Approximate: 12-bit ADC, Vref ~3300 mV.
        adc_mv = (avg_raw as i64 * 3300 / 4095) as i32;
    }

    // --- Correct for the 2:1 voltage divider ---
    let mut bat_mv = adc_mv as u32 * VDIV_RATIO;

    // --- Clamp to sensible range (guards against noise / floating pin) ---
    if bat_mv > BAT_MV_MAX {
        bat_mv = BAT_MV_MAX;
    }

    bat_mv as u16
}

/// Read the charge-status GPIO.
///
/// Returns `true` while the TP4065B CHRG pin is low (actively charging).
///
/// # Safety
/// Must be called after a successful `tp4065b_init`.
unsafe fn is_charging_inner() -> bool {
    let pwr = &*(&raw const S_PWR);
    if !pwr.initialized {
        return false;
    }
    // CHRG is open-drain: low = charging
    gpio_get_level(pwr.cfg.pin_charge_status as u32) == 0
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the TP4065B driver.
///
/// `config` must point to a `PowerTp4065bConfig`.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let pwr = &mut *(&raw mut S_PWR);

    if pwr.initialized {
        // Idempotent — already up.
        return ESP_OK;
    }

    // --- Copy config ---
    let src = &*(config as *const PowerTp4065bConfig);
    pwr.cfg.adc_channel = src.adc_channel;
    pwr.cfg.pin_charge_status = src.pin_charge_status;

    // --- ADC oneshot unit for ADC1 ---
    // adc_oneshot_unit_init_cfg_t: { unit_id: u32, clk_src: u32, ulp_mode: u32 }
    // unit_id = ADC_UNIT_1 = 0 on ESP32-S3; clk_src = 0 (default); ulp_mode = 0.
    #[repr(C)]
    struct AdcUnitCfg { unit_id: u32, clk_src: u32, ulp_mode: u32 }
    let unit_cfg = AdcUnitCfg { unit_id: 0, clk_src: 0, ulp_mode: 0 };

    let err = adc_oneshot_new_unit(
        &unit_cfg as *const AdcUnitCfg as *const c_void,
        &mut pwr.adc_handle,
    );
    if err != ESP_OK {
        return err;
    }

    // --- Configure the battery-voltage channel ---
    // adc_oneshot_chan_cfg_t: { atten: u32, bitwidth: u32 }
    // ADC_ATTEN_DB_12 = 3 (full-scale ~3.3 V), ADC_BITWIDTH_12 = 12.
    #[repr(C)]
    struct AdcChanCfg { atten: u32, bitwidth: u32 }
    let chan_cfg = AdcChanCfg { atten: 3, bitwidth: 12 };

    let err = adc_oneshot_config_channel(
        pwr.adc_handle,
        pwr.cfg.adc_channel,
        &chan_cfg as *const AdcChanCfg as *const c_void,
    );
    if err != ESP_OK {
        adc_oneshot_del_unit(pwr.adc_handle);
        pwr.adc_handle = std::ptr::null_mut();
        return err;
    }

    // --- Calibration (best-effort) ---
    // ADC_UNIT_1 = 0, ADC_ATTEN_DB_12 = 3
    let (cali_handle, cali_ok) = cali_init(0, pwr.cfg.adc_channel, 3);
    pwr.cali_handle = cali_handle;
    pwr.has_calibration = cali_ok;

    // --- Charge-status GPIO: input with internal pull-up ---
    let gpio_cfg = GpioConfig {
        pin_bit_mask: 1u64 << (pwr.cfg.pin_charge_status as u64),
        mode: GPIO_MODE_INPUT,
        pull_up_en: GPIO_PULLUP_ENABLE,
        pull_down_en: GPIO_PULLDOWN_DISABLE,
        intr_type: GPIO_INTR_DISABLE,
    };
    let err = gpio_config(&gpio_cfg);
    if err != ESP_OK {
        cali_deinit(pwr.cali_handle);
        pwr.cali_handle = std::ptr::null_mut();
        pwr.has_calibration = false;
        adc_oneshot_del_unit(pwr.adc_handle);
        pwr.adc_handle = std::ptr::null_mut();
        return err;
    }

    pwr.initialized = true;
    ESP_OK
}

/// De-initialise the TP4065B driver and release hardware resources.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_deinit() {
    let pwr = &mut *(&raw mut S_PWR);

    if !pwr.initialized {
        return;
    }

    pwr.initialized = false;

    if pwr.has_calibration {
        cali_deinit(pwr.cali_handle);
        pwr.cali_handle = std::ptr::null_mut();
        pwr.has_calibration = false;
    }

    if !pwr.adc_handle.is_null() {
        adc_oneshot_del_unit(pwr.adc_handle);
        pwr.adc_handle = std::ptr::null_mut();
    }
}

/// Fill `*info` with the current battery voltage, charge percent, and state.
///
/// # Safety
/// Called from C via the HAL vtable; `info` must be a valid non-null pointer.
unsafe extern "C" fn tp4065b_get_info(info: *mut HalPowerInfo) -> i32 {
    if info.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let pwr = &*(&raw const S_PWR);
    if !pwr.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let mv = read_battery_mv();
    let pct = voltage_to_percent(mv);
    let charging = is_charging_inner();

    let state = if charging {
        HalPowerState::Charging
    } else if pct >= 99 {
        HalPowerState::Charged
    } else {
        HalPowerState::Discharging
    };

    (*info).voltage_mv = mv;
    (*info).percent = pct;
    (*info).state = state;

    ESP_OK
}

/// Return the current battery voltage in millivolts.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_get_battery_mv() -> u16 {
    read_battery_mv()
}

/// Return the current battery charge as a percentage (0–100).
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_get_battery_percent() -> u8 {
    let mv = read_battery_mv();
    voltage_to_percent(mv)
}

/// Return `true` while the TP4065B CHRG pin indicates active charging.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_is_charging() -> bool {
    is_charging_inner()
}

/// Sleep / wake control — no-op for the TP4065B.
///
/// The ADC oneshot unit consumes no power when idle and the GPIO input is
/// inherently low-power.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn tp4065b_sleep(_enter: bool) -> i32 {
    ESP_OK
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL power driver vtable for the TP4065B.
///
/// Returned by `drv_power_tp4065b_get()` and passed to `hal_power_register()`.
static POWER_DRIVER: HalPowerDriver = HalPowerDriver {
    init: Some(tp4065b_init),
    deinit: Some(tp4065b_deinit),
    get_info: Some(tp4065b_get_info),
    get_battery_mv: Some(tp4065b_get_battery_mv),
    get_battery_percent: Some(tp4065b_get_battery_percent),
    is_charging: Some(tp4065b_is_charging),
    sleep: Some(tp4065b_sleep),
    name: b"TP4065B\0".as_ptr() as *const c_char,
};

/// Return the TP4065B power driver vtable.
///
/// Drop-in replacement for the C `drv_power_tp4065b_get()`.
///
/// # Safety
/// Returns a pointer to a program-lifetime static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_power_tp4065b_get() -> *const HalPowerDriver {
    &POWER_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the global driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_PWR) = PowerState::new();
    }

    // ── voltage_to_percent: boundary and breakpoint tests ────────────────────

    #[test]
    fn test_vtp_top_clamp() {
        // Any voltage at or above 4200 mV should return 100 %.
        assert_eq!(voltage_to_percent(4200), 100);
        assert_eq!(voltage_to_percent(4300), 100);
        assert_eq!(voltage_to_percent(u16::MAX), 100);
    }

    #[test]
    fn test_vtp_bottom_clamp() {
        // Any voltage at or below 3000 mV should return 0 %.
        assert_eq!(voltage_to_percent(3000), 0);
        assert_eq!(voltage_to_percent(2999), 0);
        assert_eq!(voltage_to_percent(0), 0);
    }

    #[test]
    fn test_vtp_exact_breakpoints() {
        // Exact breakpoint values must match their table entries.
        assert_eq!(voltage_to_percent(4200), 100);
        assert_eq!(voltage_to_percent(4060),  90);
        assert_eq!(voltage_to_percent(3980),  80);
        assert_eq!(voltage_to_percent(3920),  70);
        assert_eq!(voltage_to_percent(3870),  60);
        assert_eq!(voltage_to_percent(3820),  50);
        assert_eq!(voltage_to_percent(3750),  40);
        assert_eq!(voltage_to_percent(3700),  30);
        assert_eq!(voltage_to_percent(3620),  20);
        assert_eq!(voltage_to_percent(3500),  10);
        assert_eq!(voltage_to_percent(3000),   0);
    }

    #[test]
    fn test_vtp_midpoint_3750() {
        // 3750 mV is an exact breakpoint → 40 %
        assert_eq!(voltage_to_percent(3750), 40);
    }

    #[test]
    fn test_vtp_interpolation_between_4060_and_4200() {
        // Segment: 4060 mV (90 %) to 4200 mV (100 %) — range 140 mV / 10 pct.
        // At 4130 mV: mv_above_lower = 4130 - 4060 = 70
        //   pct = 90 + (70 * 10 + 70) / 140 = 90 + 770/140 = 90 + 5 = 95
        assert_eq!(voltage_to_percent(4130), 95);
    }

    #[test]
    fn test_vtp_interpolation_3820_to_3870() {
        // Segment: 3820 mV (50 %) to 3870 mV (60 %) — range 50 mV / 10 pct.
        // At 3845 mV: mv_above_lower = 25
        //   pct = 50 + (25 * 10 + 25) / 50 = 50 + 275/50 = 50 + 5 = 55
        assert_eq!(voltage_to_percent(3845), 55);
    }

    #[test]
    fn test_vtp_interpolation_3700_to_3750() {
        // Segment: 3700 mV (30 %) to 3750 mV (40 %) — range 50 mV / 10 pct.
        // At 3725 mV: mv_above_lower = 25
        //   pct = 30 + (25 * 10 + 25) / 50 = 30 + 275/50 = 30 + 5 = 35
        assert_eq!(voltage_to_percent(3725), 35);
    }

    #[test]
    fn test_vtp_interpolation_3500_to_3620() {
        // Segment: 3500 mV (10 %) to 3620 mV (20 %) — range 120 mV / 10 pct.
        // At 3560 mV: mv_above_lower = 60
        //   pct = 10 + (60 * 10 + 60) / 120 = 10 + 660/120 = 10 + 5 = 15
        assert_eq!(voltage_to_percent(3560), 15);
    }

    #[test]
    fn test_vtp_interpolation_3000_to_3500() {
        // Segment: 3000 mV (0 %) to 3500 mV (10 %) — range 500 mV / 10 pct.
        // At 3250 mV: mv_above_lower = 250
        //   pct = 0 + (250 * 10 + 250) / 500 = 2750/500 = 5
        assert_eq!(voltage_to_percent(3250), 5);
    }

    // ── Monotonicity check ────────────────────────────────────────────────────

    #[test]
    fn test_vtp_monotonic() {
        // percentage must be non-decreasing as voltage increases over [3000, 4200].
        let mut prev: u8 = 0;
        let mut mv: u16 = 3000;
        while mv <= 4200 {
            let pct = voltage_to_percent(mv);
            assert!(
                pct >= prev,
                "monotonicity violated at {} mV: {} < {}",
                mv,
                pct,
                prev
            );
            prev = pct;
            mv = mv.saturating_add(10);
        }
    }

    // ── Vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_non_null() {
        let p = drv_power_tp4065b_get();
        assert!(!p.is_null());
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_power_tp4065b_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.get_info.is_some());
        assert!(drv.get_battery_mv.is_some());
        assert!(drv.get_battery_percent.is_some());
        assert!(drv.is_charging.is_some());
        assert!(drv.sleep.is_some());
        assert!(!drv.name.is_null());
    }

    #[test]
    fn test_vtable_name_is_tp4065b() {
        let drv = unsafe { &*drv_power_tp4065b_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "TP4065B");
    }

    // ── Init / deinit ─────────────────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(tp4065b_init(std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert!(!(*(&raw const S_PWR)).initialized);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig {
                adc_channel: 3,
                pin_charge_status: 4,
            };
            let ret = tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_PWR)).initialized);

            tp4065b_deinit();
            assert!(!(*(&raw const S_PWR)).initialized);
            assert!((*(&raw const S_PWR)).adc_handle.is_null());
        }
    }

    #[test]
    fn test_double_init_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig {
                adc_channel: 3,
                pin_charge_status: 4,
            };
            let p = &cfg as *const PowerTp4065bConfig as *const c_void;
            assert_eq!(tp4065b_init(p), ESP_OK);
            assert_eq!(tp4065b_init(p), ESP_OK); // second call is a no-op
            assert!((*(&raw const S_PWR)).initialized);
            tp4065b_deinit();
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            tp4065b_deinit(); // must not panic
            assert!(!(*(&raw const S_PWR)).initialized);
        }
    }

    // ── Read functions after init ─────────────────────────────────────────────

    #[test]
    fn test_get_battery_mv_after_init() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig {
                adc_channel: 3,
                pin_charge_status: 4,
            };
            assert_eq!(
                tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void),
                ESP_OK
            );
            let mv = tp4065b_get_battery_mv();
            // Stub raw = 2300 → adc_mv ≈ 1853 → bat_mv = 3706
            assert!(mv > 0, "expected non-zero battery voltage from stub");
            assert!(mv <= 4300, "voltage must be clamped to 4300 mV");
            tp4065b_deinit();
        }
    }

    #[test]
    fn test_get_battery_mv_before_init_returns_zero() {
        unsafe {
            reset_state();
            assert_eq!(tp4065b_get_battery_mv(), 0);
        }
    }

    #[test]
    fn test_get_battery_percent_after_init() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig {
                adc_channel: 3,
                pin_charge_status: 4,
            };
            assert_eq!(
                tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void),
                ESP_OK
            );
            let pct = tp4065b_get_battery_percent();
            assert!(pct <= 100, "percentage must be ≤ 100");
            tp4065b_deinit();
        }
    }

    #[test]
    fn test_is_charging_before_init_returns_false() {
        unsafe {
            reset_state();
            assert!(!tp4065b_is_charging());
        }
    }

    #[test]
    fn test_is_charging_after_init_returns_false_on_stub() {
        // Stub gpio_get_level always returns 1 (not charging).
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig {
                adc_channel: 3,
                pin_charge_status: 4,
            };
            assert_eq!(
                tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void),
                ESP_OK
            );
            assert!(!tp4065b_is_charging());
            tp4065b_deinit();
        }
    }

    #[test]
    fn test_sleep_is_noop() {
        unsafe {
            reset_state();
            assert_eq!(tp4065b_sleep(true), ESP_OK);
            assert_eq!(tp4065b_sleep(false), ESP_OK);
        }
    }

    // ── get_info ──────────────────────────────────────────────────────────────

    #[test]
    fn test_get_info_null_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig { adc_channel: 3, pin_charge_status: 4 };
            tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void);
            assert_eq!(tp4065b_get_info(std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
            tp4065b_deinit();
        }
    }

    #[test]
    fn test_get_info_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let mut info = HalPowerInfo {
                voltage_mv: 0,
                percent: 0,
                state: HalPowerState::NoBattery,
            };
            assert_eq!(tp4065b_get_info(&mut info), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_get_info_after_init_populates_fields() {
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig { adc_channel: 3, pin_charge_status: 4 };
            assert_eq!(
                tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void),
                ESP_OK
            );

            let mut info = HalPowerInfo {
                voltage_mv: 0,
                percent: 0,
                state: HalPowerState::NoBattery,
            };
            assert_eq!(tp4065b_get_info(&mut info), ESP_OK);

            assert!(info.voltage_mv > 0);
            assert!(info.percent <= 100);
            // Stub is not charging → Discharging or Charged (≥99 %)
            assert!(
                info.state == HalPowerState::Discharging
                    || info.state == HalPowerState::Charged
            );

            tp4065b_deinit();
        }
    }

    // ── percent consistency ───────────────────────────────────────────────────

    #[test]
    fn test_get_battery_percent_consistent_with_mv() {
        // The percent returned by get_battery_percent() must equal
        // voltage_to_percent(get_battery_mv()).
        unsafe {
            reset_state();
            let cfg = PowerTp4065bConfig { adc_channel: 3, pin_charge_status: 4 };
            assert_eq!(
                tp4065b_init(&cfg as *const PowerTp4065bConfig as *const c_void),
                ESP_OK
            );
            let mv = tp4065b_get_battery_mv();
            let pct_direct = tp4065b_get_battery_percent();
            let pct_computed = voltage_to_percent(mv);
            assert_eq!(pct_direct, pct_computed);
            tp4065b_deinit();
        }
    }
}
