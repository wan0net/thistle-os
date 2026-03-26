// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — U-blox MIA-M10Q GPS driver (Rust)
//
// Rust port of components/drv_gps_mia_m10q/src/drv_gps_mia_m10q.c.
//
// Communicates with the U-blox MIA-M10Q via UART. A background FreeRTOS task
// reads NMEA sentences byte-by-byte, parses $GNRMC and $GNGGA, and fires the
// registered callback on a valid fix. Supports UBX-RXM-PMREQ sleep/wake.

#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalGpsCb, HalGpsDriver, HalGpsPosition};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NO_MEM: i32 = 0x101;

// ── Driver constants ─────────────────────────────────────────────────────────

const UART_RX_BUF_SIZE: i32 = 1024;
const RX_TASK_STACK_SIZE: u32 = 4096;
const RX_TASK_PRIORITY: u32 = 5;
const KNOTS_TO_KMH: f32 = 1.852;

/// UBX-RXM-PMREQ: put the receiver into backup mode (indefinite).
/// Sync chars (0xB5, 0x62) + class 0x02, id 0x41 + 8-byte payload + checksum.
const UBX_RXM_PMREQ_BACKUP: [u8; 16] = [
    0xB5, 0x62, // sync chars
    0x02, 0x41, // class, id
    0x08, 0x00, // payload length = 8
    0x00, 0x00, 0x00, 0x00, // duration = 0 (indefinite)
    0x02, 0x00, 0x00, 0x00, // flags: backup
    0x4D, 0x3B, // checksum
];

// ── ESP-IDF FFI ──────────────────────────────────────────────────────────────

#[cfg(target_os = "espidf")]
extern "C" {
    fn uart_param_config(port: i32, cfg: *const c_void) -> i32;
    fn uart_set_pin(port: i32, tx: i32, rx: i32, rts: i32, cts: i32) -> i32;
    fn uart_driver_install(
        port: i32,
        rx_buf: i32,
        tx_buf: i32,
        queue_size: i32,
        queue: *mut *mut c_void,
        flags: i32,
    ) -> i32;
    fn uart_driver_delete(port: i32) -> i32;
    fn uart_read_bytes(port: i32, buf: *mut u8, len: u32, timeout: u32) -> i32;
    fn uart_write_bytes(port: i32, buf: *const u8, len: usize) -> i32;
    fn xTaskCreatePinnedToCore(
        task_fn: unsafe extern "C" fn(*mut c_void),
        name: *const u8,
        stack: u32,
        param: *mut c_void,
        prio: u32,
        handle: *mut *mut c_void,
        core_id: i32,
    ) -> i32;
    fn vTaskDelete(task: *mut c_void);
    fn vTaskDelay(ticks: u32);
}

// ── Simulator / host stubs ───────────────────────────────────────────────────

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_param_config(_port: i32, _cfg: *const c_void) -> i32 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_set_pin(_port: i32, _tx: i32, _rx: i32, _rts: i32, _cts: i32) -> i32 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_driver_install(
    _port: i32,
    _rx_buf: i32,
    _tx_buf: i32,
    _queue_size: i32,
    _queue: *mut *mut c_void,
    _flags: i32,
) -> i32 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_driver_delete(_port: i32) -> i32 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_read_bytes(_port: i32, _buf: *mut u8, _len: u32, _timeout: u32) -> i32 {
    // Never returns data; the RX task would spin until vTaskDelete is called.
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn uart_write_bytes(_port: i32, _buf: *const u8, _len: usize) -> i32 {
    0
}

#[cfg(not(target_os = "espidf"))]
unsafe fn xTaskCreatePinnedToCore(
    _task_fn: unsafe extern "C" fn(*mut c_void),
    _name: *const u8,
    _stack: u32,
    _param: *mut c_void,
    _prio: u32,
    handle: *mut *mut c_void,
    _core_id: i32,
) -> i32 {
    // Return a non-null sentinel handle so callers can tell it "worked".
    *handle = 1usize as *mut c_void;
    1 // pdPASS
}

#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelete(_task: *mut c_void) {}

#[cfg(not(target_os = "espidf"))]
unsafe fn vTaskDelay(_ticks: u32) {}

// ── uart_config_t layout (must match ESP-IDF struct on Xtensa) ───────────────

#[cfg(target_os = "espidf")]
#[repr(C)]
struct UartConfig {
    baud_rate: i32,
    data_bits: u32,  // UART_DATA_8_BITS = 3
    parity: u32,     // UART_PARITY_DISABLE = 0
    stop_bits: u32,  // UART_STOP_BITS_1 = 1
    flow_ctrl: u32,  // UART_HW_FLOWCTRL_DISABLE = 0
    rx_flow_ctrl_thresh: u8,
    source_clk: u32, // UART_SCLK_DEFAULT = 0
}

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible config for the MIA-M10Q driver.
/// Must match `gps_mia_m10q_config_t` in the C header.
#[repr(C)]
pub struct GpsMiaM10qConfig {
    /// uart_port_t (0 or 1)
    pub uart_num: i32,
    /// TX GPIO number (gpio_num_t)
    pub pin_tx: i32,
    /// RX GPIO number (gpio_num_t)
    pub pin_rx: i32,
    /// Baud rate; 0 defaults to 9600
    pub baud_rate: u32,
}

// SAFETY: Config holds only primitives; accessed from single-threaded
// board-init context before the RX task starts.
unsafe impl Send for GpsMiaM10qConfig {}
unsafe impl Sync for GpsMiaM10qConfig {}

// ── Driver state ─────────────────────────────────────────────────────────────

struct GpsState {
    cfg: GpsMiaM10qConfig,
    cb: Option<unsafe extern "C" fn(*const HalGpsPosition, *mut c_void)>,
    cb_data: *mut c_void,
    last_position: HalGpsPosition,
    rx_task: *mut c_void,
    initialized: bool,
    enabled: bool,
    nmea_buf: [u8; 256],
    nmea_idx: usize,
}

// SAFETY: The driver state is only mutated during single-threaded board-init
// and from the single RX task; last_position updates mirror C portENTER_CRITICAL
// semantics via the static_mut + unsafe convention used throughout this crate.
unsafe impl Send for GpsState {}
unsafe impl Sync for GpsState {}

impl GpsState {
    const fn new() -> Self {
        GpsState {
            cfg: GpsMiaM10qConfig {
                uart_num: 1,
                pin_tx: -1,
                pin_rx: -1,
                baud_rate: 9600,
            },
            cb: None,
            cb_data: std::ptr::null_mut(),
            last_position: HalGpsPosition {
                latitude: 0.0,
                longitude: 0.0,
                altitude_m: 0.0,
                speed_kmh: 0.0,
                heading_deg: 0.0,
                satellites: 0,
                fix_valid: false,
                timestamp: 0,
            },
            rx_task: std::ptr::null_mut(),
            initialized: false,
            enabled: false,
            nmea_buf: [0u8; 256],
            nmea_idx: 0,
        }
    }
}

static mut S_GPS: GpsState = GpsState::new();

// ── NMEA helpers (pure Rust, no external crates) ─────────────────────────────

/// Verify the NMEA XOR checksum.
///
/// The checksum covers all bytes strictly between `$` and `*`.
/// Returns `false` if the sentence is malformed or the checksum does not match.
pub fn nmea_verify_checksum(sentence: &[u8]) -> bool {
    if sentence.is_empty() || sentence[0] != b'$' {
        return false;
    }

    let inner = &sentence[1..]; // skip '$'
    let star_pos = match inner.iter().position(|&b| b == b'*') {
        Some(p) => p,
        None => return false,
    };

    let computed: u8 = inner[..star_pos].iter().fold(0u8, |acc, &b| acc ^ b);

    let hex = &inner[star_pos + 1..];
    if hex.len() < 2 {
        return false;
    }

    let received = match u8::from_str_radix(
        core::str::from_utf8(&hex[..2]).unwrap_or("XX"),
        16,
    ) {
        Ok(v) => v,
        Err(_) => return false,
    };

    computed == received
}

/// Convert an NMEA coordinate field (`ddmm.mmmm` or `dddmm.mmmm`) plus a
/// direction character (`N`, `S`, `E`, `W`) to signed decimal degrees.
///
/// Returns `0.0` for an empty or unparseable field.
pub fn nmea_parse_coord(field: &str, dir: &str) -> f64 {
    if field.is_empty() {
        return 0.0;
    }

    let raw: f64 = match field.parse() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };

    let deg = (raw / 100.0).floor();
    let mins = raw - deg * 100.0;
    let mut dd = deg + mins / 60.0;

    if let Some(c) = dir.chars().next() {
        if c == 'S' || c == 'W' {
            dd = -dd;
        }
    }

    dd
}

/// Convert NMEA UTC time (`hhmmss` or `hhmmss.ss`) and date (`ddmmyy`)
/// strings to a Unix timestamp (seconds since 1970-01-01 UTC).
///
/// Returns `0` if either string is too short or the date is invalid.
///
/// Uses a hand-rolled UTC epoch calculation that matches `mktime` behaviour on
/// an ESP32 where the TZ is always UTC.
pub fn nmea_to_timestamp(time_str: &str, date_str: &str) -> u32 {
    if time_str.len() < 6 || date_str.len() < 6 {
        return 0;
    }

    let tb = time_str.as_bytes();
    let db = date_str.as_bytes();

    let digit = |b: u8| -> u32 {
        if b.is_ascii_digit() { (b - b'0') as u32 } else { 0 }
    };

    let hour  = digit(tb[0]) * 10 + digit(tb[1]);
    let min   = digit(tb[2]) * 10 + digit(tb[3]);
    let sec   = digit(tb[4]) * 10 + digit(tb[5]);
    let mday  = digit(db[0]) * 10 + digit(db[1]);
    let month = digit(db[2]) * 10 + digit(db[3]); // 1-based
    let yy    = digit(db[4]) * 10 + digit(db[5]);
    // Match C mktime convention: yy is years since 1900, so yy ≥ 70 → 1970+,
    // yy < 70 → 2000+.  (Same rule as the C driver's `t.tm_year = yy + 100`.)
    let year  = if yy >= 70 { 1900 + yy } else { 2000 + yy };

    if mday == 0 || month == 0 || month > 12 {
        return 0;
    }

    const DAYS_IN_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let is_leap = |y: u32| (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);

    // Days from 1970-01-01 to start of `year`
    let mut days: u64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }

    // Days for completed months within `year`
    for m in 1..month {
        let extra = if m == 2 && is_leap(year) { 1 } else { 0 };
        days += DAYS_IN_MONTH[(m - 1) as usize] as u64 + extra;
    }

    // Days completed in current month (mday is 1-based)
    days += (mday - 1) as u64;

    let ts = days * 86400 + hour as u64 * 3600 + min as u64 * 60 + sec as u64;
    ts as u32
}

// ── NMEA sentence processors ─────────────────────────────────────────────────

/// Split `sentence` on commas, stripping the `*CS` suffix if present.
fn nmea_split(sentence: &str) -> Vec<&str> {
    let body = if let Some(pos) = sentence.find('*') {
        &sentence[..pos]
    } else {
        sentence
    };
    body.split(',').collect()
}

/// Parse a `$GNRMC` / `$GPRMC` sentence and update the global position.
///
/// Field layout (0-based after comma-split):
/// ```text
/// 0  $GNRMC
/// 1  UTC time  (hhmmss.ss)
/// 2  Status    (A=active, V=void)
/// 3  Latitude  (ddmm.mmmm)
/// 4  N/S
/// 5  Longitude (dddmm.mmmm)
/// 6  E/W
/// 7  Speed (knots)
/// 8  Course (degrees true)
/// 9  Date  (ddmmyy)
/// ```
///
/// # Safety
/// Writes to `S_GPS.last_position`.
unsafe fn process_gnrmc(fields: &[&str]) {
    if fields.len() < 10 {
        return;
    }

    let valid   = fields[2].starts_with('A');
    let lat     = nmea_parse_coord(fields[3], fields[4]);
    let lon     = nmea_parse_coord(fields[5], fields[6]);
    let speed   = if !fields[7].is_empty() {
        fields[7].parse::<f32>().unwrap_or(0.0) * KNOTS_TO_KMH
    } else {
        0.0
    };
    let heading = if !fields[8].is_empty() {
        fields[8].parse::<f32>().unwrap_or(0.0)
    } else {
        0.0
    };
    let ts = nmea_to_timestamp(fields[1], fields[9]);

    let gps = &mut *(&raw mut S_GPS);
    gps.last_position.latitude    = lat;
    gps.last_position.longitude   = lon;
    gps.last_position.speed_kmh   = speed;
    gps.last_position.heading_deg = heading;
    gps.last_position.fix_valid   = valid;
    if ts != 0 {
        gps.last_position.timestamp = ts;
    }
}

/// Parse a `$GNGGA` / `$GPGGA` sentence and update altitude + satellite count.
///
/// Field layout (0-based):
/// ```text
/// 0  $GNGGA
/// 1  UTC time
/// 2  Latitude
/// 3  N/S
/// 4  Longitude
/// 5  E/W
/// 6  Fix quality (0=no fix, 1=GPS, 2=DGPS)
/// 7  Satellites in use
/// 8  HDOP
/// 9  Altitude MSL (metres)
/// ```
///
/// # Safety
/// Writes to `S_GPS.last_position`.
unsafe fn process_gngga(fields: &[&str]) {
    if fields.len() < 10 {
        return;
    }

    let quality: i32    = fields[6].parse().unwrap_or(0);
    let satellites: i32 = fields[7].parse().unwrap_or(0);
    let altitude: f32   = fields[9].parse().unwrap_or(0.0);

    let gps = &mut *(&raw mut S_GPS);
    gps.last_position.satellites = satellites.max(0) as u8;
    gps.last_position.altitude_m = altitude;
    if quality == 0 {
        gps.last_position.fix_valid = false;
    }
}

/// Dispatch a complete NMEA sentence to the appropriate parser.
///
/// Validates the checksum first; silently drops sentences that fail.
/// After a valid `$GNRMC` update the registered callback is fired if the
/// fix is valid, matching the C driver's behaviour exactly.
///
/// # Safety
/// May mutate `S_GPS` via `process_gnrmc` / `process_gngga`.
unsafe fn nmea_process_sentence(sentence: &[u8]) {
    if !nmea_verify_checksum(sentence) {
        return;
    }

    let s = match core::str::from_utf8(sentence) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Strip trailing CR / LF / space
    let s = s.trim_end_matches(|c: char| c == '\r' || c == '\n' || c == ' ');

    let fields = nmea_split(s);
    if fields.is_empty() {
        return;
    }

    match fields[0] {
        "$GNRMC" | "$GPRMC" => {
            process_gnrmc(&fields);

            // Fire callback (position + validity are fresh after RMC).
            let gps = &*(&raw const S_GPS);
            if let Some(cb) = gps.cb {
                let snap = gps.last_position;
                if snap.fix_valid {
                    cb(&snap as *const HalGpsPosition, gps.cb_data);
                }
            }
        }
        "$GNGGA" | "$GPGGA" => {
            process_gngga(&fields);
        }
        _ => {}
    }
}

// ── UART RX task ─────────────────────────────────────────────────────────────

/// Background FreeRTOS task: reads UART byte-by-byte, assembles NMEA sentences
/// delimited by `$` (start) and `\n` (end), then dispatches to the parser.
///
/// # Safety
/// Accesses `S_GPS` directly. Must only be spawned via `xTaskCreate`.
pub unsafe extern "C" fn gps_rx_task(_arg: *mut c_void) {
    let mut byte: u8 = 0;

    loop {
        let len = uart_read_bytes(
            (*(&raw const S_GPS)).cfg.uart_num,
            &mut byte as *mut u8,
            1,
            100, // pdMS_TO_TICKS(100) with configTICK_RATE_HZ=1000
        );

        if len > 0 {
            let gps = &mut *(&raw mut S_GPS);

            if byte == b'$' {
                gps.nmea_idx = 0;
            }

            if gps.nmea_idx < gps.nmea_buf.len() - 1 {
                gps.nmea_buf[gps.nmea_idx] = byte;
                gps.nmea_idx += 1;
            }

            if byte == b'\n' {
                let end = gps.nmea_idx;
                let slice = &gps.nmea_buf[..end];
                nmea_process_sentence(slice);
                gps.nmea_idx = 0;
            }
        }
    }
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the MIA-M10Q UART peripheral.
///
/// Configures baud rate / pins / driver but does **not** start the RX task.
/// Call `mia_m10q_enable()` to begin receiving NMEA data.
///
/// # Safety
/// `config` must point to a valid `GpsMiaM10qConfig`.
unsafe extern "C" fn mia_m10q_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let gps = &mut *(&raw mut S_GPS);

    if gps.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let src = &*(config as *const GpsMiaM10qConfig);
    gps.cfg.uart_num  = src.uart_num;
    gps.cfg.pin_tx    = src.pin_tx;
    gps.cfg.pin_rx    = src.pin_rx;
    gps.cfg.baud_rate = src.baud_rate;

    let baud = if gps.cfg.baud_rate == 0 { 9600 } else { gps.cfg.baud_rate };

    #[cfg(target_os = "espidf")]
    {
        let uart_cfg = UartConfig {
            baud_rate: baud as i32,
            data_bits: 3, // UART_DATA_8_BITS
            parity:    0, // UART_PARITY_DISABLE
            stop_bits: 1, // UART_STOP_BITS_1
            flow_ctrl: 0, // UART_HW_FLOWCTRL_DISABLE
            rx_flow_ctrl_thresh: 0,
            source_clk: 0, // UART_SCLK_DEFAULT
        };
        let ret = uart_param_config(
            gps.cfg.uart_num,
            &uart_cfg as *const UartConfig as *const c_void,
        );
        if ret != ESP_OK {
            return ret;
        }
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = baud;
        let ret = uart_param_config(gps.cfg.uart_num, std::ptr::null());
        if ret != ESP_OK {
            return ret;
        }
    }

    let ret = uart_set_pin(
        gps.cfg.uart_num,
        gps.cfg.pin_tx,
        gps.cfg.pin_rx,
        -1, // UART_PIN_NO_CHANGE
        -1,
    );
    if ret != ESP_OK {
        return ret;
    }

    let ret = uart_driver_install(
        gps.cfg.uart_num,
        UART_RX_BUF_SIZE,
        0,
        0,
        std::ptr::null_mut(),
        0,
    );
    if ret != ESP_OK {
        return ret;
    }

    gps.last_position = HalGpsPosition {
        latitude: 0.0, longitude: 0.0, altitude_m: 0.0,
        speed_kmh: 0.0, heading_deg: 0.0, satellites: 0,
        fix_valid: false, timestamp: 0,
    };
    gps.nmea_idx    = 0;
    gps.rx_task     = std::ptr::null_mut();
    gps.enabled     = false;
    gps.initialized = true;

    ESP_OK
}

/// De-initialise the driver.
///
/// Stops the RX task if running and removes the UART driver.
///
/// # Safety
/// Must be called from the same context as `mia_m10q_init`.
unsafe extern "C" fn mia_m10q_deinit() {
    let gps = &mut *(&raw mut S_GPS);

    if !gps.initialized {
        return;
    }

    if !gps.rx_task.is_null() {
        vTaskDelete(gps.rx_task);
        gps.rx_task = std::ptr::null_mut();
    }

    uart_driver_delete(gps.cfg.uart_num);

    gps.initialized = false;
    gps.enabled     = false;
}

/// Start the background UART RX task.
///
/// # Safety
/// Driver must be initialised.
unsafe extern "C" fn mia_m10q_enable() -> i32 {
    let gps = &mut *(&raw mut S_GPS);

    if !gps.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if gps.enabled {
        return ESP_OK;
    }

    let rc = xTaskCreatePinnedToCore(
        gps_rx_task,
        b"gps_rx\0".as_ptr(),
        RX_TASK_STACK_SIZE,
        std::ptr::null_mut(),
        RX_TASK_PRIORITY,
        &mut gps.rx_task as *mut *mut c_void,
        1, // pin to core 1 (app core)
    );

    if rc != 1 {
        // pdPASS = 1
        return ESP_ERR_NO_MEM;
    }

    gps.enabled = true;
    ESP_OK
}

/// Stop the background RX task.
///
/// # Safety
/// Driver must be initialised.
unsafe extern "C" fn mia_m10q_disable() -> i32 {
    let gps = &mut *(&raw mut S_GPS);

    if !gps.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if !gps.enabled {
        return ESP_OK;
    }

    if !gps.rx_task.is_null() {
        vTaskDelete(gps.rx_task);
        gps.rx_task = std::ptr::null_mut();
    }

    gps.enabled = false;
    ESP_OK
}

/// Copy the last known GPS position into `*pos`.
///
/// Returns `ESP_OK` when the fix is valid, `ESP_ERR_INVALID_STATE` otherwise.
///
/// # Safety
/// `pos` must be a writable `HalGpsPosition`.
unsafe extern "C" fn mia_m10q_get_position(pos: *mut HalGpsPosition) -> i32 {
    if pos.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let gps = &*(&raw const S_GPS);
    *pos = gps.last_position;

    if (*pos).fix_valid { ESP_OK } else { ESP_ERR_INVALID_STATE }
}

/// Register a callback invoked on each valid fix.
///
/// # Safety
/// `cb` must remain valid until it is replaced or the driver is de-initialised.
unsafe extern "C" fn mia_m10q_register_callback(cb: HalGpsCb, user_data: *mut c_void) -> i32 {
    let gps = &mut *(&raw mut S_GPS);
    gps.cb      = cb;
    gps.cb_data = user_data;
    ESP_OK
}

/// Enter or leave the MIA-M10Q hardware low-power backup mode.
///
/// - `enter = true`:  send UBX-RXM-PMREQ (indefinite backup mode).
/// - `enter = false`: send 0xFF wakeup pulse then delay 500 ms.
///
/// # Safety
/// Driver must be initialised.
unsafe extern "C" fn mia_m10q_sleep(enter: bool) -> i32 {
    let gps = &*(&raw const S_GPS);

    if !gps.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if enter {
        let written = uart_write_bytes(
            gps.cfg.uart_num,
            UBX_RXM_PMREQ_BACKUP.as_ptr(),
            UBX_RXM_PMREQ_BACKUP.len(),
        );
        if written < 0 {
            return ESP_FAIL;
        }
    } else {
        let wake: u8 = 0xFF;
        uart_write_bytes(gps.cfg.uart_num, &wake as *const u8, 1);
        // pdMS_TO_TICKS(500) with configTICK_RATE_HZ=1000 → 500 ticks
        vTaskDelay(500);
    }

    ESP_OK
}

// ── HAL vtable ────────────────────────────────────────────────────────────────

/// Static HAL GPS driver vtable for the MIA-M10Q.
///
/// Pass to `hal_gps_register()`. Returned by `drv_gps_mia_m10q_get()`.
static GPS_DRIVER: HalGpsDriver = HalGpsDriver {
    init:              Some(mia_m10q_init),
    deinit:            Some(mia_m10q_deinit),
    enable:            Some(mia_m10q_enable),
    disable:           Some(mia_m10q_disable),
    get_position:      Some(mia_m10q_get_position),
    register_callback: Some(mia_m10q_register_callback),
    sleep:             Some(mia_m10q_sleep),
    name:              b"MIA-M10Q\0".as_ptr() as *const c_char,
};

/// Return the MIA-M10Q driver vtable.
///
/// Drop-in replacement for the C `drv_gps_mia_m10q_get()`.
///
/// # Safety
/// Returns a pointer to a `'static` — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_gps_mia_m10q_get() -> *const HalGpsDriver {
    &GPS_DRIVER
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_GPS) = GpsState::new();
    }

    // ── nmea_verify_checksum ──────────────────────────────────────────────────

    #[test]
    fn test_checksum_valid_gnrmc() {
        // XOR of bytes in "GNRMC,092751.000,A,5321.6802,N,00630.3372,W,0.06,31.66,280511,,,A" = 0x58
        let s = b"$GNRMC,092751.000,A,5321.6802,N,00630.3372,W,0.06,31.66,280511,,,A*58";
        assert!(nmea_verify_checksum(s));
    }

    #[test]
    fn test_checksum_valid_gngga() {
        // XOR of bytes in "GNGGA,092751.000,5321.6802,N,00630.3372,W,1,08,1.03,61.7,M,55.3,M,," = 0x58
        let s = b"$GNGGA,092751.000,5321.6802,N,00630.3372,W,1,08,1.03,61.7,M,55.3,M,,*58";
        assert!(nmea_verify_checksum(s));
    }

    #[test]
    fn test_checksum_wrong_value() {
        let s = b"$GNRMC,092751.000,A,5321.6802,N,00630.3372,W,0.06,31.66,280511,,,A*FF";
        assert!(!nmea_verify_checksum(s));
    }

    #[test]
    fn test_checksum_missing_star() {
        let s = b"$GNRMC,092751.000,A";
        assert!(!nmea_verify_checksum(s));
    }

    #[test]
    fn test_checksum_empty_input() {
        assert!(!nmea_verify_checksum(b""));
    }

    #[test]
    fn test_checksum_no_leading_dollar() {
        assert!(!nmea_verify_checksum(b"GNRMC,test*00"));
    }

    #[test]
    fn test_checksum_minimal_valid() {
        // '$' then 'A' then '*41' — XOR of 'A' (0x41) == 0x41.
        let s = b"$A*41";
        assert!(nmea_verify_checksum(s));
    }

    // ── nmea_parse_coord ──────────────────────────────────────────────────────

    #[test]
    fn test_coord_north() {
        // 5321.6802 N => 53 + 21.6802/60
        let dd = nmea_parse_coord("5321.6802", "N");
        let expected = 53.0 + 21.6802 / 60.0;
        assert!((dd - expected).abs() < 1e-6, "got {dd}, want {expected}");
    }

    #[test]
    fn test_coord_south() {
        let dd = nmea_parse_coord("3400.0000", "S");
        let expected = -34.0;
        assert!((dd - expected).abs() < 1e-6);
    }

    #[test]
    fn test_coord_west() {
        // 00630.3372 W => -(6 + 30.3372/60)
        let dd = nmea_parse_coord("00630.3372", "W");
        let expected = -(6.0 + 30.3372 / 60.0);
        assert!((dd - expected).abs() < 1e-6);
    }

    #[test]
    fn test_coord_east() {
        let dd = nmea_parse_coord("01200.0000", "E");
        assert!((dd - 12.0).abs() < 1e-6);
    }

    #[test]
    fn test_coord_empty_field() {
        assert_eq!(nmea_parse_coord("", "N"), 0.0);
    }

    #[test]
    fn test_coord_zero() {
        assert_eq!(nmea_parse_coord("0000.0000", "N"), 0.0);
    }

    // ── nmea_to_timestamp ─────────────────────────────────────────────────────

    #[test]
    fn test_timestamp_unix_epoch() {
        // 1970-01-01 00:00:00 UTC
        // yy=70 → year=1970 (yy >= 70 maps to 1900+yy, matching C mktime convention)
        let ts = nmea_to_timestamp("000000", "010170");
        assert_eq!(ts, 0);
    }

    #[test]
    fn test_timestamp_known_value() {
        // 2011-05-28 09:27:51 UTC
        // Hand-computed: 14975 days (1970→2011) + 147 days (Jan–May28) = 15122 days
        // 15122 × 86400 + 9×3600 + 27×60 + 51 = 1306574871
        let ts = nmea_to_timestamp("092751", "280511");
        assert_eq!(ts, 1306574871);
    }

    #[test]
    fn test_timestamp_empty_time() {
        assert_eq!(nmea_to_timestamp("", "280511"), 0);
    }

    #[test]
    fn test_timestamp_empty_date() {
        assert_eq!(nmea_to_timestamp("092751", ""), 0);
    }

    #[test]
    fn test_timestamp_sub_seconds_ignored() {
        // hhmmss.ss — only the first 6 bytes matter
        let ts1 = nmea_to_timestamp("092751.00", "280511");
        let ts2 = nmea_to_timestamp("092751",    "280511");
        assert_eq!(ts1, ts2);
    }

    #[test]
    fn test_timestamp_leap_year_2000() {
        // 2000-02-29 00:00:00 UTC (2000 is a leap year)
        // python3: calendar.timegm((2000,2,29,0,0,0,0,0,0)) = 951782400
        let ts = nmea_to_timestamp("000000", "290200");
        assert_eq!(ts, 951782400);
    }

    #[test]
    fn test_timestamp_day_boundary_2020() {
        // 2020-01-01 00:00:00 UTC
        // python3: calendar.timegm((2020,1,1,0,0,0,0,0,0)) = 1577836800
        let ts = nmea_to_timestamp("000000", "010120");
        assert_eq!(ts, 1577836800);
    }

    // ── nmea_split ────────────────────────────────────────────────────────────

    #[test]
    fn test_split_basic() {
        let fields = nmea_split("$GNRMC,time,A,lat,N,lon,W");
        assert_eq!(fields[0], "$GNRMC");
        assert_eq!(fields[2], "A");
    }

    #[test]
    fn test_split_strips_checksum_suffix() {
        let fields = nmea_split("$GNRMC,a,b*2B");
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2], "b");
    }

    // ── process_gnrmc ─────────────────────────────────────────────────────────

    #[test]
    fn test_process_gnrmc_updates_position() {
        unsafe {
            reset_state();
            // Sentence with valid checksum (0x58 — same body as test_checksum_valid_gnrmc).
            let s = b"$GNRMC,092751.000,A,5321.6802,N,00630.3372,W,0.06,31.66,280511,,,A*58\n";
            nmea_process_sentence(s);

            let pos = (*(&raw const S_GPS)).last_position;
            assert!(pos.fix_valid, "expected fix_valid = true");

            let expected_lat = 53.0 + 21.6802 / 60.0;
            let expected_lon = -(6.0 + 30.3372 / 60.0);
            assert!((pos.latitude  - expected_lat).abs() < 1e-5);
            assert!((pos.longitude - expected_lon).abs() < 1e-5);

            let expected_speed = 0.06_f32 * KNOTS_TO_KMH;
            assert!((pos.speed_kmh   - expected_speed).abs() < 1e-4);
            assert!((pos.heading_deg - 31.66).abs() < 1e-3);
        }
    }

    #[test]
    fn test_process_gnrmc_void_status_clears_fix() {
        unsafe {
            reset_state();
            (*(&raw mut S_GPS)).last_position.fix_valid = true;

            let fields: Vec<&str> = vec![
                "$GNRMC", "000000.000", "V", "0000.0000", "N",
                "00000.0000", "E", "0.00", "0.00", "010170", "", "", "N",
            ];
            process_gnrmc(&fields);

            assert!(!(*(&raw const S_GPS)).last_position.fix_valid);
        }
    }

    // ── process_gngga ─────────────────────────────────────────────────────────

    #[test]
    fn test_process_gngga_updates_altitude_and_sats() {
        unsafe {
            reset_state();

            let fields: Vec<&str> = vec![
                "$GNGGA", "092751.000", "5321.6802", "N", "00630.3372", "W",
                "1", "8", "1.03", "61.7", "M", "55.3", "M", "", "",
            ];
            process_gngga(&fields);

            let pos = (*(&raw const S_GPS)).last_position;
            assert_eq!(pos.satellites, 8);
            assert!((pos.altitude_m - 61.7).abs() < 0.01);
        }
    }

    #[test]
    fn test_process_gngga_quality_zero_clears_fix() {
        unsafe {
            reset_state();
            (*(&raw mut S_GPS)).last_position.fix_valid = true;

            let fields: Vec<&str> = vec![
                "$GNGGA", "000000.000", "0000.0000", "N", "00000.0000", "W",
                "0", "0", "99.0", "0.0", "M", "0.0", "M", "", "",
            ];
            process_gngga(&fields);

            assert!(!(*(&raw const S_GPS)).last_position.fix_valid);
        }
    }

    // ── vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_non_null() {
        assert!(!drv_gps_mia_m10q_get().is_null());
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_gps_mia_m10q_get() };
        assert!(drv.init.is_some());
        assert!(drv.deinit.is_some());
        assert!(drv.enable.is_some());
        assert!(drv.disable.is_some());
        assert!(drv.get_position.is_some());
        assert!(drv.register_callback.is_some());
        assert!(drv.sleep.is_some());
        assert!(!drv.name.is_null());
    }

    // ── init / deinit ─────────────────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(mia_m10q_init(std::ptr::null()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            assert_eq!(mia_m10q_init(&cfg as *const GpsMiaM10qConfig as *const c_void), ESP_OK);
            assert!((*(&raw const S_GPS)).initialized);
            mia_m10q_deinit();
            assert!(!(*(&raw const S_GPS)).initialized);
        }
    }

    #[test]
    fn test_double_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            let p = &cfg as *const GpsMiaM10qConfig as *const c_void;
            assert_eq!(mia_m10q_init(p), ESP_OK);
            assert_eq!(mia_m10q_init(p), ESP_ERR_INVALID_STATE);
            mia_m10q_deinit();
        }
    }

    #[test]
    fn test_enable_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(mia_m10q_enable(), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_enable_disable_cycle() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            mia_m10q_init(&cfg as *const GpsMiaM10qConfig as *const c_void);
            assert_eq!(mia_m10q_enable(), ESP_OK);
            assert!((*(&raw const S_GPS)).enabled);
            assert_eq!(mia_m10q_disable(), ESP_OK);
            assert!(!(*(&raw const S_GPS)).enabled);
            mia_m10q_deinit();
        }
    }

    #[test]
    fn test_double_enable_is_idempotent() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            mia_m10q_init(&cfg as *const GpsMiaM10qConfig as *const c_void);
            assert_eq!(mia_m10q_enable(), ESP_OK);
            assert_eq!(mia_m10q_enable(), ESP_OK);
            mia_m10q_deinit();
        }
    }

    #[test]
    fn test_disable_before_enable_is_ok() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            mia_m10q_init(&cfg as *const GpsMiaM10qConfig as *const c_void);
            assert_eq!(mia_m10q_disable(), ESP_OK);
            mia_m10q_deinit();
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            mia_m10q_deinit(); // must not panic
            assert!(!(*(&raw const S_GPS)).initialized);
        }
    }

    // ── get_position ──────────────────────────────────────────────────────────

    #[test]
    fn test_get_position_null_returns_invalid_arg() {
        unsafe {
            assert_eq!(mia_m10q_get_position(std::ptr::null_mut()), ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_get_position_no_fix_returns_invalid_state() {
        unsafe {
            reset_state();
            let mut pos = HalGpsPosition {
                latitude: 0.0, longitude: 0.0, altitude_m: 0.0,
                speed_kmh: 0.0, heading_deg: 0.0, satellites: 0,
                fix_valid: false, timestamp: 0,
            };
            assert_eq!(
                mia_m10q_get_position(&mut pos as *mut HalGpsPosition),
                ESP_ERR_INVALID_STATE,
            );
        }
    }

    #[test]
    fn test_get_position_valid_fix_returns_ok() {
        unsafe {
            reset_state();
            (*(&raw mut S_GPS)).last_position.fix_valid = true;
            (*(&raw mut S_GPS)).last_position.latitude  = 53.36;

            let mut pos = HalGpsPosition {
                latitude: 0.0, longitude: 0.0, altitude_m: 0.0,
                speed_kmh: 0.0, heading_deg: 0.0, satellites: 0,
                fix_valid: false, timestamp: 0,
            };
            assert_eq!(mia_m10q_get_position(&mut pos as *mut HalGpsPosition), ESP_OK);
            assert!(pos.fix_valid);
            assert!((pos.latitude - 53.36).abs() < 1e-9);
        }
    }

    // ── register_callback ─────────────────────────────────────────────────────

    #[test]
    fn test_register_callback_stores_values() {
        unsafe {
            reset_state();

            unsafe extern "C" fn dummy_cb(_pos: *const HalGpsPosition, _ud: *mut c_void) {}

            let sentinel = 0xDEAD_BEEFusize as *mut c_void;
            assert_eq!(mia_m10q_register_callback(Some(dummy_cb), sentinel), ESP_OK);
            assert!((*(&raw const S_GPS)).cb.is_some());
            assert_eq!((*(&raw const S_GPS)).cb_data, sentinel);
        }
    }

    // ── sleep ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sleep_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(mia_m10q_sleep(true),  ESP_ERR_INVALID_STATE);
            assert_eq!(mia_m10q_sleep(false), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_sleep_enter_and_wake_after_init() {
        unsafe {
            reset_state();
            let cfg = GpsMiaM10qConfig { uart_num: 1, pin_tx: 10, pin_rx: 9, baud_rate: 9600 };
            mia_m10q_init(&cfg as *const GpsMiaM10qConfig as *const c_void);
            // Stub uart_write_bytes returns 0 (>= 0), so both succeed.
            assert_eq!(mia_m10q_sleep(true),  ESP_OK);
            assert_eq!(mia_m10q_sleep(false), ESP_OK);
            mia_m10q_deinit();
        }
    }
}
