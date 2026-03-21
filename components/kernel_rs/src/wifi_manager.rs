// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — wifi_manager module
//
// Port of components/kernel/src/wifi_manager.c
// Manages ESP-IDF WiFi station mode: connect, scan, disconnect, NTP sync.
// On simulator builds all functions are stubs.

use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_TIMEOUT: i32 = 0x107;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

// WiFi state constants — must match wifi_manager.h
const WIFI_STATE_DISCONNECTED: u32 = 0;
const WIFI_STATE_CONNECTING: u32 = 1;
const WIFI_STATE_CONNECTED: u32 = 2;
const WIFI_STATE_FAILED: u32 = 3;

// SSID max length (matches wifi_manager.h)
const WIFI_SSID_MAX_LEN: usize = 32;

static TAG: &[u8] = b"wifi_mgr\0";

// ---------------------------------------------------------------------------
// Logging FFI
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;
const ESP_LOG_ERROR: i32 = 1;
const ESP_LOG_DEBUG: i32 = 4;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct WifiState {
    state: u32,
    ip: [u8; 16],
    initialized: bool,
}

impl WifiState {
    const fn new() -> Self {
        WifiState {
            state: WIFI_STATE_DISCONNECTED,
            ip: [0u8; 16],
            initialized: false,
        }
    }
}

static WIFI_STATE: Mutex<WifiState> = Mutex::new(WifiState::new());

// ---------------------------------------------------------------------------
// ESP-IDF WiFi FFI (hardware only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_netif_init() -> i32;
    fn esp_event_loop_create_default() -> i32;
    fn esp_netif_create_default_wifi_sta() -> *mut c_void;
    fn esp_wifi_init(cfg: *const c_void) -> i32;
    fn esp_event_handler_register(
        base: *const c_char,
        id: i32,
        handler: *const c_void,
        arg: *mut c_void,
    ) -> i32;
    fn esp_wifi_set_mode(mode: u32) -> i32;
    fn esp_wifi_start() -> i32;
    fn esp_wifi_connect() -> i32;
    fn esp_wifi_disconnect() -> i32;
    fn esp_wifi_set_config(iface: u32, cfg: *const c_void) -> i32;
    fn esp_wifi_scan_start(cfg: *const c_void, block: bool) -> i32;
    fn esp_wifi_scan_get_ap_num(count: *mut u16) -> i32;
    fn esp_wifi_scan_get_ap_records(count: *mut u16, records: *mut c_void) -> i32;
    fn esp_wifi_sta_get_ap_info(info: *mut c_void) -> i32;
    fn esp_netif_sntp_init(cfg: *const c_void) -> i32;
    fn esp_netif_sntp_sync_wait(ticks: u32) -> i32;
    fn esp_netif_sntp_deinit();
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

// ---------------------------------------------------------------------------
// WiFi scan result struct — matches wifi_manager.h
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct WifiScanResult {
    pub ssid: [u8; WIFI_SSID_MAX_LEN + 1],
    pub rssi: i8,
    pub channel: u8,
    pub is_open: bool,
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the WiFi manager and start the WiFi stack.
///
/// On simulator builds, this is a no-op that returns ESP_OK.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn wifi_manager_init() -> i32 {
    let already_init = WIFI_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if already_init {
        return ESP_OK;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        let ret = esp_netif_init();
        if ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"netif init failed: %d\0".as_ptr(), ret);
            return ret;
        }

        let ret = esp_event_loop_create_default();
        if ret != ESP_OK && ret != 0x103 /* ESP_ERR_INVALID_STATE = already created */ {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"event loop failed: %d\0".as_ptr(), ret);
            return ret;
        }

        esp_netif_create_default_wifi_sta();

        // WIFI_INIT_CONFIG_DEFAULT is a macro — use a zeroed 512-byte buffer
        // which is larger than wifi_init_config_t; the magic value at offset 0
        // is set by the macro. We call esp_wifi_init with a default config
        // obtained from the C side instead.
        // In practice this module is always compiled alongside C code that
        // provides wifi_manager_init_config_default() or we link wifi_manager.c
        // directly. For the Rust-only path, call the C helper.
        extern "C" {
            fn wifi_manager_init_hardware() -> i32;
        }
        let ret = wifi_manager_init_hardware();
        if ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"wifi hw init failed: %d\0".as_ptr(), ret);
            return ret;
        }
    }

    if let Ok(mut state) = WIFI_STATE.lock() {
        state.initialized = true;
    }

    unsafe {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"WiFi manager initialized\0".as_ptr());
    }

    ESP_OK
}

/// Scan for available WiFi networks.
///
/// On simulator, returns 0 results.
///
/// # Safety
/// `results` must point to an array of at least `max_results` WifiScanResult.
/// `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_scan(
    results: *mut WifiScanResult,
    max_results: u8,
    out_count: *mut u8,
) -> i32 {
    if results.is_null() || out_count.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    *out_count = 0;

    let initialized = WIFI_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    {
        // scan_config: show_hidden=false, scan_type=ACTIVE(0), min=100ms, max=300ms
        let scan_cfg = [0u8; 40]; // wifi_scan_config_t — let C defaults fill it
        let ret = esp_wifi_scan_start(scan_cfg.as_ptr() as *const c_void, true);
        if ret != ESP_OK {
            return ret;
        }

        let mut ap_count: u16 = 0;
        esp_wifi_scan_get_ap_num(&mut ap_count);

        let fetch = ap_count.min(max_results as u16);
        // wifi_ap_record_t is ~108 bytes; allocate via calloc
        let records = calloc(fetch as usize, 108);
        if records.is_null() {
            return ESP_ERR_NO_MEM;
        }

        let mut fetch_count = fetch;
        esp_wifi_scan_get_ap_records(&mut fetch_count, records);

        // wifi_ap_record_t layout: ssid[33], bssid[6], primary(u8), ...  rssi(i8 at offset ~40)
        // We use C helper to extract rather than hardcoding offsets
        extern "C" {
            fn wifi_ap_record_get_ssid(record: *const c_void, idx: usize) -> *const c_char;
            fn wifi_ap_record_get_rssi(record: *const c_void, idx: usize) -> i8;
            fn wifi_ap_record_get_channel(record: *const c_void, idx: usize) -> u8;
            fn wifi_ap_record_is_open(record: *const c_void, idx: usize) -> bool;
        }

        for i in 0..fetch_count as usize {
            let r = &mut *results.add(i);
            r.ssid = [0u8; WIFI_SSID_MAX_LEN + 1];

            let ssid_ptr = wifi_ap_record_get_ssid(records, i);
            if !ssid_ptr.is_null() {
                let ssid_bytes = std::ffi::CStr::from_ptr(ssid_ptr).to_bytes();
                let copy_len = ssid_bytes.len().min(WIFI_SSID_MAX_LEN);
                r.ssid[..copy_len].copy_from_slice(&ssid_bytes[..copy_len]);
            }

            r.rssi    = wifi_ap_record_get_rssi(records, i);
            r.channel = wifi_ap_record_get_channel(records, i);
            r.is_open = wifi_ap_record_is_open(records, i);
        }

        free(records);
        *out_count = fetch_count as u8;

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Scan complete: %d networks\0".as_ptr(),
            fetch_count as i32,
        );
    }

    #[cfg(not(target_os = "espidf"))]
    {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"WiFi scan: simulator stub\0".as_ptr());
    }

    ESP_OK
}

/// Connect to a WiFi network.
///
/// On simulator, always returns ESP_ERR_NOT_SUPPORTED.
///
/// # Safety
/// `ssid` must be a valid null-terminated C string.
/// `password` may be NULL for open networks.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_connect(
    ssid: *const c_char,
    password: *const c_char,
    timeout_ms: u32,
) -> i32 {
    if ssid.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let initialized = WIFI_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    {
        // Build wifi_config_t on the stack (256 bytes, zeroed)
        let mut wifi_cfg = [0u8; 256];

        // Copy SSID into station config (offset 0)
        let ssid_bytes = std::ffi::CStr::from_ptr(ssid).to_bytes();
        let ssid_len = ssid_bytes.len().min(32);
        wifi_cfg[..ssid_len].copy_from_slice(&ssid_bytes[..ssid_len]);

        // Copy password (offset 32)
        if !password.is_null() {
            let pass_bytes = std::ffi::CStr::from_ptr(password).to_bytes();
            let pass_len = pass_bytes.len().min(64);
            wifi_cfg[32..32 + pass_len].copy_from_slice(&pass_bytes[..pass_len]);
        }

        let _ = esp_wifi_set_config(0 /* WIFI_IF_STA */, wifi_cfg.as_ptr() as *const c_void);

        if let Ok(mut s) = WIFI_STATE.lock() {
            s.state = WIFI_STATE_CONNECTING;
        }

        let _ = esp_wifi_connect();

        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Connecting to WiFi...\0".as_ptr());

        // Poll for connection with timeout
        let timeout = if timeout_ms == 0 { 10000 } else { timeout_ms };
        let start = std::time::Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let state = WIFI_STATE.lock().map(|s| s.state).unwrap_or(WIFI_STATE_DISCONNECTED);
            if state == WIFI_STATE_CONNECTED {
                return ESP_OK;
            }
            if state == WIFI_STATE_FAILED || start.elapsed().as_millis() >= timeout as u128 {
                if let Ok(mut s) = WIFI_STATE.lock() {
                    s.state = WIFI_STATE_FAILED;
                }
                return ESP_ERR_TIMEOUT;
            }
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"WiFi connect: simulator stub\0".as_ptr());
        ESP_ERR_NOT_SUPPORTED
    }
}

/// Disconnect from the current WiFi network.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn wifi_manager_disconnect() -> i32 {
    let initialized = WIFI_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        esp_wifi_disconnect();
    }

    if let Ok(mut s) = WIFI_STATE.lock() {
        s.state = WIFI_STATE_DISCONNECTED;
    }

    ESP_OK
}

/// Return the current WiFi state (matches wifi_state_t enum).
#[no_mangle]
pub extern "C" fn wifi_manager_get_state() -> u32 {
    WIFI_STATE.lock().map(|s| s.state).unwrap_or(WIFI_STATE_DISCONNECTED)
}

/// Return the current IP address as a NUL-terminated C string, or NULL if not connected.
///
/// # Safety
/// Returns a pointer into stable static storage. Do not free.
#[no_mangle]
pub extern "C" fn wifi_manager_get_ip() -> *const c_char {
    // We return a pointer into the static buffer inside WIFI_STATE.
    // This is safe because the Mutex guard is released before we return,
    // but the data is stable (it's inside a static Mutex).
    // In practice C callers use this immediately and don't store it.
    static IP_BUF: Mutex<[u8; 16]> = Mutex::new([0u8; 16]);

    if let Ok(state) = WIFI_STATE.lock() {
        if state.state != WIFI_STATE_CONNECTED {
            return std::ptr::null();
        }
        if let Ok(mut buf) = IP_BUF.lock() {
            *buf = state.ip;
        }
    }

    if let Ok(buf) = IP_BUF.lock() {
        if buf[0] == 0 {
            return std::ptr::null();
        }
        return buf.as_ptr() as *const c_char;
    }

    std::ptr::null()
}

/// Return the RSSI of the current access point, or 0 if not connected.
#[no_mangle]
pub extern "C" fn wifi_manager_get_rssi() -> i8 {
    let connected = WIFI_STATE
        .lock()
        .map(|s| s.state == WIFI_STATE_CONNECTED)
        .unwrap_or(false);

    if !connected {
        return 0;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        // wifi_ap_record_t is ~108 bytes; rssi is i8 after 33+6 bytes of MAC+SSID
        let mut ap_info = [0u8; 128];
        if esp_wifi_sta_get_ap_info(ap_info.as_mut_ptr() as *mut c_void) == ESP_OK {
            return ap_info[39] as i8; // rssi offset in wifi_ap_record_t
        }
    }

    0
}

/// Sync time via NTP (requires WiFi connection).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn wifi_manager_ntp_sync() -> i32 {
    let connected = WIFI_STATE
        .lock()
        .map(|s| s.state == WIFI_STATE_CONNECTED)
        .unwrap_or(false);

    if !connected {
        unsafe {
            esp_log_write(
                ESP_LOG_WARN,
                TAG.as_ptr(),
                b"Cannot sync NTP: not connected to WiFi\0".as_ptr(),
            );
        }
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Starting NTP sync...\0".as_ptr());

        // ESP_NETIF_SNTP_DEFAULT_CONFIG expands to a struct with "pool.ntp.org"
        // We call a C helper to avoid reproducing the macro
        extern "C" {
            fn wifi_manager_do_ntp_sync() -> i32;
        }
        return wifi_manager_do_ntp_sync();
    }

    #[cfg(not(target_os = "espidf"))]
    {
        unsafe {
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"NTP sync: simulator stub\0".as_ptr());
        }
        ESP_ERR_NOT_SUPPORTED
    }
}

/// Write the current time as "HH:MM" into buf.
///
/// Writes "--:--" if the clock has not been set.
///
/// # Safety
/// `buf` must point to at least `buf_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_get_time_str(buf: *mut c_char, buf_len: usize) {
    if buf.is_null() || buf_len == 0 {
        return;
    }

    let time_str = get_time_str_internal();
    let bytes = time_str.as_bytes();
    let len = bytes.len().min(buf_len - 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
    *buf.add(len) = 0;
}

/// Write the current date as "YYYY-MM-DD" into buf.
///
/// # Safety
/// `buf` must point to at least `buf_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_get_date_str(buf: *mut c_char, buf_len: usize) {
    if buf.is_null() || buf_len == 0 {
        return;
    }

    let date_str = get_date_str_internal();
    let bytes = date_str.as_bytes();
    let len = bytes.len().min(buf_len - 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
    *buf.add(len) = 0;
}

// ---------------------------------------------------------------------------
// Internal time helpers
// ---------------------------------------------------------------------------

fn get_time_str_internal() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Very rough: if Unix time is before 2024-01-01, clock is unset
    if secs < 1704067200 {
        return "--:--".to_string();
    }

    let secs_in_day = secs % 86400;
    let hh = secs_in_day / 3600;
    let mm = (secs_in_day % 3600) / 60;
    format!("{:02}:{:02}", hh, mm)
}

fn get_date_str_internal() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if secs < 1704067200 {
        return "----/--/--".to_string();
    }

    // Basic Gregorian calendar calculation
    let days = secs / 86400;
    let (year, month, day) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Days since 1970-01-01
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// Internal state update — called from the C wifi_event_handler shim
// ---------------------------------------------------------------------------

/// Update the WiFi state and IP from the C event handler.
///
/// # Safety
/// `ip` must point to a 16-byte null-terminated string, or may be NULL.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_set_state(new_state: u32, ip: *const c_char) {
    if let Ok(mut state) = WIFI_STATE.lock() {
        state.state = new_state;
        if new_state == WIFI_STATE_CONNECTED && !ip.is_null() {
            let ip_str = std::ffi::CStr::from_ptr(ip).to_bytes();
            let len = ip_str.len().min(15);
            state.ip[..len].copy_from_slice(&ip_str[..len]);
            state.ip[len] = 0;
        }
    }
}
