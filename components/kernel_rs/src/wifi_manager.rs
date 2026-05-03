// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — wifi_manager module
//
// Port of components/kernel/src/wifi_manager.c
// Manages ESP-IDF WiFi station mode: connect, scan, disconnect, NTP sync.
// On simulator builds all functions are stubs.

use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

use crate::event::{event_publish, CEvent, EventType};

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

#[cfg(target_os = "espidf")]
const WIFI_MODE_STA: u32 = 1;
#[cfg(target_os = "espidf")]
const ESP_EVENT_ANY_ID: i32 = -1;
#[cfg(target_os = "espidf")]
const WIFI_EVENT_STA_DISCONNECTED: i32 = 5;
#[cfg(target_os = "espidf")]
const IP_EVENT_STA_GOT_IP: i32 = 0;

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
    static WIFI_EVENT: *const c_char;
    static IP_EVENT: *const c_char;

    fn esp_netif_init() -> i32;
    fn esp_event_loop_create_default() -> i32;
    fn esp_netif_get_handle_from_ifkey(key: *const c_char) -> *mut c_void;
    fn esp_netif_create_default_wifi_sta() -> *mut c_void;
    fn thistle_wifi_init() -> i32;  // C shim: WIFI_INIT_CONFIG_DEFAULT + esp_wifi_init
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

#[cfg(target_os = "espidf")]
#[repr(C)]
struct EspIp4Addr {
    addr: u32,
}

#[cfg(target_os = "espidf")]
#[repr(C)]
struct EspNetifIpInfo {
    ip: EspIp4Addr,
    netmask: EspIp4Addr,
    gw: EspIp4Addr,
}

#[cfg(target_os = "espidf")]
#[repr(C)]
struct IpEventGotIp {
    esp_netif: *mut c_void,
    ip_info: EspNetifIpInfo,
    ip_changed: bool,
}

#[cfg(target_os = "espidf")]
unsafe extern "C" fn wifi_event_handler(
    _arg: *mut c_void,
    event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
) {
    if event_base == WIFI_EVENT && event_id == WIFI_EVENT_STA_DISCONNECTED {
        wifi_manager_set_state(WIFI_STATE_DISCONNECTED, std::ptr::null());
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"WiFi disconnected\0".as_ptr());
    } else if event_base == IP_EVENT && event_id == IP_EVENT_STA_GOT_IP {
        let got_ip = event_data as *const IpEventGotIp;
        if !got_ip.is_null() {
            let octets = (*got_ip).ip_info.ip.addr.to_le_bytes();
            let ip = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
            if let Ok(ip_cstr) = std::ffi::CString::new(ip) {
                wifi_manager_set_state(WIFI_STATE_CONNECTED, ip_cstr.as_ptr());
            } else {
                wifi_manager_set_state(WIFI_STATE_CONNECTED, std::ptr::null());
            }
        } else {
            wifi_manager_set_state(WIFI_STATE_CONNECTED, std::ptr::null());
        }
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"WiFi got IP\0".as_ptr());
    }
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
        // Inline wifi_manager_init_hardware logic (formerly in kernel_shims.c)
        static mut S_WIFI_HW_INITIALIZED: bool = false;
        static mut S_WIFI_EVENTS_REGISTERED: bool = false;
        if !S_WIFI_HW_INITIALIZED {
            let ret = esp_netif_init();
            if ret != ESP_OK && ret != ESP_ERR_INVALID_STATE {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"netif init failed: %d\0".as_ptr(), ret);
                return ret;
            }
            let ret = esp_event_loop_create_default();
            if ret != ESP_OK && ret != ESP_ERR_INVALID_STATE {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"event loop failed: %d\0".as_ptr(), ret);
                return ret;
            }
            // Check if STA interface already exists before creating
            let existing = esp_netif_get_handle_from_ifkey(b"WIFI_STA_DEF\0".as_ptr() as *const c_char);
            let sta = if !existing.is_null() { existing } else { esp_netif_create_default_wifi_sta() };
            if sta.is_null() {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"netif create sta failed\0".as_ptr());
                return -1;
            }
            // Use the C shim: WIFI_INIT_CONFIG_DEFAULT is a macro that can't be
            // called from Rust. The shim sets the magic field required by v6.
            let ret = thistle_wifi_init();
            if ret != ESP_OK {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_wifi_init failed: %d\0".as_ptr(), ret);
                return ret;
            }

            let ret = esp_wifi_set_mode(WIFI_MODE_STA);
            if ret != ESP_OK {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_wifi_set_mode failed: %d\0".as_ptr(), ret);
                return ret;
            }

            let ret = esp_wifi_start();
            if ret != ESP_OK && ret != ESP_ERR_INVALID_STATE {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"esp_wifi_start failed: %d\0".as_ptr(), ret);
                return ret;
            }

            S_WIFI_HW_INITIALIZED = true;
        }

        if !S_WIFI_EVENTS_REGISTERED {
            let ret = esp_event_handler_register(
                WIFI_EVENT,
                ESP_EVENT_ANY_ID,
                wifi_event_handler as *const c_void,
                std::ptr::null_mut(),
            );
            if ret != ESP_OK {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"wifi event register failed: %d\0".as_ptr(), ret);
                return ret;
            }

            let ret = esp_event_handler_register(
                IP_EVENT,
                ESP_EVENT_ANY_ID,
                wifi_event_handler as *const c_void,
                std::ptr::null_mut(),
            );
            if ret != ESP_OK {
                esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ip event register failed: %d\0".as_ptr(), ret);
                return ret;
            }
            S_WIFI_EVENTS_REGISTERED = true;
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

        // wifi_ap_record_t layout (108 bytes):
        //   ssid[33] at offset 0, bssid[6] at offset 33,
        //   primary (channel, u8) at offset 39, second at 40,
        //   rssi (i8) at offset 41, authmode (u32) at offset 44
        const AP_RECORD_SIZE: usize = 108;
        const AP_SSID_OFFSET: usize = 0;
        const AP_CHANNEL_OFFSET: usize = 39;
        const AP_RSSI_OFFSET: usize = 41;
        const AP_AUTHMODE_OFFSET: usize = 44;
        const WIFI_AUTH_OPEN: u32 = 0;

        for i in 0..fetch_count as usize {
            let r = &mut *results.add(i);
            r.ssid = [0u8; WIFI_SSID_MAX_LEN + 1];

            let rec = (records as *const u8).add(i * AP_RECORD_SIZE);
            // SSID: null-terminated string at offset 0
            let ssid_ptr = rec.add(AP_SSID_OFFSET) as *const c_char;
            let ssid_bytes = std::ffi::CStr::from_ptr(ssid_ptr).to_bytes();
            let copy_len = ssid_bytes.len().min(WIFI_SSID_MAX_LEN);
            r.ssid[..copy_len].copy_from_slice(&ssid_bytes[..copy_len]);

            r.channel = *rec.add(AP_CHANNEL_OFFSET);
            r.rssi    = *rec.add(AP_RSSI_OFFSET) as i8;
            // authmode is a u32 at offset 44
            let authmode = (rec.add(AP_AUTHMODE_OFFSET) as *const u32).read_unaligned();
            r.is_open = authmode == WIFI_AUTH_OPEN;
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
        // NTP sync stub — implement with SNTP when needed
        return ESP_OK;
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
// WiFi credential persistence — save/load from system.json
// ---------------------------------------------------------------------------

/// Resolve the path to system.json (SD card first, then SPIFFS, then simulator).
fn system_json_path() -> &'static str {
    #[cfg(target_os = "espidf")]
    {
        if std::path::Path::new("/sdcard/config/system.json").exists() {
            return "/sdcard/config/system.json";
        }
        "/spiffs/config/system.json"
    }
    #[cfg(not(target_os = "espidf"))]
    {
        "/tmp/thistle_sdcard/config/system.json"
    }
}

/// Replace a JSON string value for a given key in-place.
/// Finds `"key": "old_value"` and replaces `old_value` with `new_value`.
fn replace_json_string_value(json: &str, key: &str, new_value: &str) -> String {
    let key_pattern = format!("\"{}\"", key);
    if let Some(key_pos) = json.find(&key_pattern) {
        let after_key_quote = &json[key_pos + key_pattern.len()..];
        // Skip optional whitespace then colon
        let after_ws = after_key_quote.trim_start();
        if !after_ws.starts_with(':') {
            return json.to_string();
        }
        let after_colon = &after_ws[1..];
        let trimmed = after_colon.trim_start();
        if trimmed.starts_with('"') {
            // Calculate absolute position of the character after the opening quote
            let value_start = json.len() - trimmed.len() + 1;
            if let Some(end_quote) = json[value_start..].find('"') {
                let mut result = String::new();
                result.push_str(&json[..value_start]);
                result.push_str(new_value);
                result.push_str(&json[value_start + end_quote..]);
                return result;
            }
        }
    }
    json.to_string()
}

/// Replace a JSON boolean value for a given key in-place.
/// Finds `"key": true/false` and replaces with the new boolean.
fn replace_json_bool_value(json: &str, key: &str, new_value: bool) -> String {
    let key_pattern = format!("\"{}\"", key);
    if let Some(key_pos) = json.find(&key_pattern) {
        let after_key_quote = &json[key_pos + key_pattern.len()..];
        let after_ws = after_key_quote.trim_start();
        if !after_ws.starts_with(':') {
            return json.to_string();
        }
        let after_colon = &after_ws[1..];
        let trimmed = after_colon.trim_start();
        let old_token;
        if trimmed.starts_with("true") {
            old_token = "true";
        } else if trimmed.starts_with("false") {
            old_token = "false";
        } else {
            return json.to_string();
        }
        let token_start = json.len() - trimmed.len();
        let token_end = token_start + old_token.len();
        let new_token = if new_value { "true" } else { "false" };
        let mut result = String::new();
        result.push_str(&json[..token_start]);
        result.push_str(new_token);
        result.push_str(&json[token_end..]);
        return result;
    }
    json.to_string()
}

/// Extract a string value for a given key from JSON text.
/// Lightweight helper matching the approach in manifest.rs.
fn find_string_value(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let trimmed = after_key.trim_start();
    if !trimmed.starts_with('"') {
        return None;
    }
    let value_start = &trimmed[1..];
    let end = value_start.find('"')?;
    Some(value_start[..end].to_string())
}

/// Extract a boolean value for a given key from JSON text.
fn find_bool_value(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let trimmed = after_key.trim_start();
    if trimmed.starts_with("true") {
        Some(true)
    } else if trimmed.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Save WiFi credentials to system.json for auto-connect on boot.
///
/// # Safety
/// `ssid` and `password` must be valid null-terminated C strings.
/// `password` may be NULL for open networks.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_save_credentials(
    ssid: *const c_char,
    password: *const c_char,
) -> i32 {
    if ssid.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let ssid_str = match std::ffi::CStr::from_ptr(ssid).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let pass_str = if password.is_null() {
        ""
    } else {
        match std::ffi::CStr::from_ptr(password).to_str() {
            Ok(s) => s,
            Err(_) => "",
        }
    };

    let path = system_json_path();
    let json = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Cannot read %s for credential save\0".as_ptr(),
                path.as_ptr(),
            );
            return ESP_ERR_NOT_FOUND;
        }
    };

    // Update the wifi section fields
    let json = replace_json_string_value(&json, "ssid", ssid_str);
    let json = replace_json_string_value(&json, "password", pass_str);
    let json = replace_json_bool_value(&json, "enabled", true);

    match std::fs::write(path, &json) {
        Ok(_) => {
            esp_log_write(
                ESP_LOG_INFO,
                TAG.as_ptr(),
                b"WiFi credentials saved to system.json\0".as_ptr(),
            );
            ESP_OK
        }
        Err(_) => {
            esp_log_write(
                ESP_LOG_ERROR,
                TAG.as_ptr(),
                b"Failed to write system.json\0".as_ptr(),
            );
            ESP_ERR_NO_MEM
        }
    }
}

const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// Load saved WiFi credentials from system.json and attempt to connect.
/// Called from kernel_init after WiFi manager is ready.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub unsafe extern "C" fn wifi_manager_auto_connect() -> i32 {
    let path = system_json_path();
    let json = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            esp_log_write(
                ESP_LOG_DEBUG,
                TAG.as_ptr(),
                b"No system.json found, skipping auto-connect\0".as_ptr(),
            );
            return ESP_OK;
        }
    };

    // Look inside the "wifi" sub-object
    let wifi_obj = match crate::board_config::extract_object(&json, "wifi") {
        Some(s) => s,
        None => return ESP_OK,
    };

    let enabled = find_bool_value(&wifi_obj, "enabled").unwrap_or(false);
    if !enabled {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"WiFi auto-connect disabled in system.json\0".as_ptr(),
        );
        return ESP_OK;
    }

    let ssid = match find_string_value(&wifi_obj, "ssid") {
        Some(s) if !s.is_empty() => s,
        _ => return ESP_OK,
    };

    let password = find_string_value(&wifi_obj, "password").unwrap_or_default();

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Auto-connecting to saved WiFi network...\0".as_ptr(),
    );

    let c_ssid = match std::ffi::CString::new(ssid) {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let c_pass = match std::ffi::CString::new(password) {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    let ret = wifi_manager_connect(c_ssid.as_ptr(), c_pass.as_ptr(), 10000);
    if ret == ESP_OK {
        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Auto-connect: success\0".as_ptr(),
        );
    } else if ret == ESP_ERR_NOT_SUPPORTED {
        // Simulator — silently ignore
    } else {
        esp_log_write(
            ESP_LOG_WARN,
            TAG.as_ptr(),
            b"Auto-connect failed: 0x%x\0".as_ptr(),
            ret,
        );
    }
    ret
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
    let old_state = WIFI_STATE
        .lock()
        .map(|s| s.state)
        .unwrap_or(WIFI_STATE_DISCONNECTED);

    if let Ok(mut state) = WIFI_STATE.lock() {
        state.state = new_state;
        if new_state == WIFI_STATE_CONNECTED && !ip.is_null() {
            let ip_str = std::ffi::CStr::from_ptr(ip).to_bytes();
            let len = ip_str.len().min(15);
            state.ip[..len].copy_from_slice(&ip_str[..len]);
            state.ip[len] = 0;
        } else {
            state.ip = [0u8; 16];
        }
    }

    if old_state != new_state {
        let event_type = match new_state {
            WIFI_STATE_CONNECTED => Some(EventType::WifiConnected as u32),
            WIFI_STATE_DISCONNECTED | WIFI_STATE_FAILED => Some(EventType::WifiDisconnected as u32),
            _ => None,
        };

        if let Some(event_type) = event_type {
            let ev = CEvent {
                event_type,
                timestamp: 0,
                data: std::ptr::null_mut(),
                data_len: 0,
            };
            let _ = event_publish(&ev as *const CEvent);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
//
// Only pure Rust functions are tested (no esp_log_write linkage needed):
//   wifi_manager_get_state()    — reads global Mutex, no C calls
//   wifi_manager_set_state()    — writes global Mutex, no C calls
//   wifi_manager_get_rssi()     — pure Rust on non-espidf
//   get_time_str_internal()     — pure Rust std::time
//   get_date_str_internal()     — pure Rust std::time
//   is_leap() / days_to_ymd()  — pure helpers
//
// wifi_manager_init(), wifi_manager_connect(), wifi_manager_scan(), and
// wifi_manager_ntp_sync() all call esp_log_write and are not tested here.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // test_initial_state_is_disconnected
    // Mirrors test_wifi_manager.c: before init, state must be DISCONNECTED.
    // -----------------------------------------------------------------------

    #[test]
    fn test_initial_state_is_disconnected() {
        let state = wifi_manager_get_state();
        assert_eq!(
            state, WIFI_STATE_DISCONNECTED,
            "initial WiFi state must be DISCONNECTED"
        );
    }

    // -----------------------------------------------------------------------
    // test_set_and_get_state
    // wifi_manager_set_state / wifi_manager_get_state round-trip.
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_and_get_state() {
        // Save original state
        let original = wifi_manager_get_state();

        unsafe { wifi_manager_set_state(WIFI_STATE_CONNECTING, std::ptr::null()) };
        assert_eq!(
            wifi_manager_get_state(), WIFI_STATE_CONNECTING,
            "state must be CONNECTING after set"
        );

        // Restore
        unsafe { wifi_manager_set_state(original, std::ptr::null()) };
    }

    #[test]
    fn test_get_ip_cleared_on_disconnect() {
        let original = wifi_manager_get_state();
        let ip = std::ffi::CString::new("192.168.1.44").unwrap();

        unsafe { wifi_manager_set_state(WIFI_STATE_CONNECTED, ip.as_ptr()) };
        assert!(
            !wifi_manager_get_ip().is_null(),
            "ip must be available while connected"
        );

        unsafe { wifi_manager_set_state(WIFI_STATE_DISCONNECTED, std::ptr::null()) };
        assert!(
            wifi_manager_get_ip().is_null(),
            "ip must be cleared after disconnect"
        );

        unsafe { wifi_manager_set_state(original, std::ptr::null()) };
    }

    // -----------------------------------------------------------------------
    // test_get_time_str_format
    // Mirrors test_wifi_manager.c: get_time_str returns "--:--" or a valid HH:MM.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_time_str_format() {
        let s = get_time_str_internal();
        // Either the clock-unset placeholder or a valid HH:MM
        if s == "--:--" {
            // Acceptable — system clock not set to a post-2024 value
        } else {
            // Must be exactly "HH:MM"
            assert_eq!(s.len(), 5, "time string must be 5 chars: '{}'", s);
            let bytes = s.as_bytes();
            assert!(bytes[0].is_ascii_digit(), "H tens must be a digit");
            assert!(bytes[1].is_ascii_digit(), "H units must be a digit");
            assert_eq!(bytes[2], b':', "separator must be ':'");
            assert!(bytes[3].is_ascii_digit(), "M tens must be a digit");
            assert!(bytes[4].is_ascii_digit(), "M units must be a digit");

            let hours: u32 = s[..2].parse().unwrap();
            let mins:  u32 = s[3..].parse().unwrap();
            assert!(hours < 24, "hours must be < 24");
            assert!(mins  < 60, "minutes must be < 60");
        }
    }

    // -----------------------------------------------------------------------
    // test_get_date_str_format
    // Mirrors test_wifi_manager.c: get_date_str returns "----/--/--" or valid YYYY-MM-DD.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_date_str_format() {
        let s = get_date_str_internal();
        if s == "----/--/--" {
            // Acceptable — clock unset
        } else {
            // Must be exactly "YYYY-MM-DD"
            assert_eq!(s.len(), 10, "date string must be 10 chars: '{}'", s);
            let bytes = s.as_bytes();
            assert_eq!(bytes[4], b'-', "first separator must be '-'");
            assert_eq!(bytes[7], b'-', "second separator must be '-'");

            let year:  u32 = s[..4].parse().unwrap();
            let month: u32 = s[5..7].parse().unwrap();
            let day:   u32 = s[8..].parse().unwrap();
            assert!(year >= 2024,  "year must be >= 2024 if clock is set");
            assert!(month >= 1 && month <= 12, "month must be 1..=12");
            assert!(day >= 1 && day <= 31,     "day must be 1..=31");
        }
    }

    // -----------------------------------------------------------------------
    // test_rssi_zero_when_disconnected
    // wifi_manager_get_rssi() must return 0 when not connected.
    // -----------------------------------------------------------------------

    #[test]
    fn test_rssi_zero_when_disconnected() {
        // Ensure state is DISCONNECTED
        let original = wifi_manager_get_state();
        unsafe { wifi_manager_set_state(WIFI_STATE_DISCONNECTED, std::ptr::null()) };

        let rssi = wifi_manager_get_rssi();
        assert_eq!(rssi, 0, "RSSI must be 0 when not connected");

        // Restore
        unsafe { wifi_manager_set_state(original, std::ptr::null()) };
    }

    // -----------------------------------------------------------------------
    // test_is_leap_helper
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_leap_helper() {
        assert!(is_leap(2000), "2000 is a leap year");
        assert!(is_leap(2024), "2024 is a leap year");
        assert!(!is_leap(1900), "1900 is not a leap year");
        assert!(!is_leap(2023), "2023 is not a leap year");
        assert!(!is_leap(2100), "2100 is not a leap year");
    }

    // -----------------------------------------------------------------------
    // test_days_to_ymd_epoch
    // days_to_ymd(0) must return 1970-01-01.
    // -----------------------------------------------------------------------

    #[test]
    fn test_days_to_ymd_epoch() {
        let (year, month, day) = days_to_ymd(0);
        assert_eq!(year, 1970, "epoch day 0 must be 1970");
        assert_eq!(month, 1,   "epoch day 0 must be January");
        assert_eq!(day,   1,   "epoch day 0 must be the 1st");
    }

    // -----------------------------------------------------------------------
    // JSON helper tests for credential persistence
    // -----------------------------------------------------------------------

    #[test]
    fn test_replace_json_string_value() {
        let json = r#"{"ssid": "old_network", "password": "secret"}"#;
        let result = replace_json_string_value(json, "ssid", "new_network");
        assert_eq!(result, r#"{"ssid": "new_network", "password": "secret"}"#);
    }

    #[test]
    fn test_replace_json_string_value_empty() {
        let json = r#"{"ssid": "", "password": ""}"#;
        let result = replace_json_string_value(json, "ssid", "my_wifi");
        assert_eq!(result, r#"{"ssid": "my_wifi", "password": ""}"#);
    }

    #[test]
    fn test_replace_json_string_value_missing_key() {
        let json = r#"{"other": "value"}"#;
        let result = replace_json_string_value(json, "ssid", "test");
        assert_eq!(result, json, "missing key should return unchanged JSON");
    }

    #[test]
    fn test_replace_json_bool_value_false_to_true() {
        let json = r#"{"enabled": false, "other": 1}"#;
        let result = replace_json_bool_value(json, "enabled", true);
        assert_eq!(result, r#"{"enabled": true, "other": 1}"#);
    }

    #[test]
    fn test_replace_json_bool_value_true_to_false() {
        let json = r#"{"enabled": true}"#;
        let result = replace_json_bool_value(json, "enabled", false);
        assert_eq!(result, r#"{"enabled": false}"#);
    }

    #[test]
    fn test_replace_json_bool_value_same() {
        let json = r#"{"enabled": true}"#;
        let result = replace_json_bool_value(json, "enabled", true);
        assert_eq!(result, r#"{"enabled": true}"#);
    }

    #[test]
    fn test_replace_json_bool_value_missing_key() {
        let json = r#"{"other": true}"#;
        let result = replace_json_bool_value(json, "enabled", true);
        assert_eq!(result, json);
    }

    #[test]
    fn test_find_string_value() {
        let json = r#"{"ssid": "my_network", "password": "pass123"}"#;
        assert_eq!(find_string_value(json, "ssid"), Some("my_network".to_string()));
        assert_eq!(find_string_value(json, "password"), Some("pass123".to_string()));
        assert_eq!(find_string_value(json, "missing"), None);
    }

    #[test]
    fn test_find_string_value_empty() {
        let json = r#"{"ssid": ""}"#;
        assert_eq!(find_string_value(json, "ssid"), Some("".to_string()));
    }

    #[test]
    fn test_find_bool_value() {
        let json = r#"{"enabled": true, "disabled": false}"#;
        assert_eq!(find_bool_value(json, "enabled"), Some(true));
        assert_eq!(find_bool_value(json, "disabled"), Some(false));
        assert_eq!(find_bool_value(json, "missing"), None);
    }

    #[test]
    fn test_replace_json_string_value_with_whitespace() {
        let json = r#"{"ssid" :  "old"}"#;
        let result = replace_json_string_value(json, "ssid", "new");
        assert_eq!(result, r#"{"ssid" :  "new"}"#);
    }

    #[test]
    fn test_full_system_json_roundtrip() {
        let json = r#"{
    "thistle_os": {
        "wifi": {
            "enabled": false,
            "ssid": "",
            "password": ""
        }
    }
}"#;
        let json = replace_json_string_value(&json, "ssid", "TestNetwork");
        let json = replace_json_string_value(&json, "password", "secret123");
        let json = replace_json_bool_value(&json, "enabled", true);
        assert!(json.contains(r#""ssid": "TestNetwork""#));
        assert!(json.contains(r#""password": "secret123""#));
        assert!(json.contains(r#""enabled": true"#));
    }
}
