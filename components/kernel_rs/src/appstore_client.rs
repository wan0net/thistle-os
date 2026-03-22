// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — appstore_client module
//
// Port of components/kernel/src/appstore_client.c
// Fetches a JSON catalog, downloads app/firmware/driver ELF files,
// verifies SHA-256 hashes and Ed25519 signatures, installs to SD card.
//
// HTTP is provided by esp_http_client (ESP-IDF) or the sim_http.c shim
// (libcurl-backed) in simulator builds. SHA-256 uses the sha2 crate.

use std::ffi::CStr;
use std::io::Write;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_INVALID_CRC: i32 = 0x109;
const ESP_FAIL: i32 = -1;

const DEFAULT_CATALOG_URL: &str = "https://wan0net.github.io/thistle-apps/catalog.json\0";
const MAX_CATALOG_JSON: usize = 32 * 1024; // 32 KB
const DOWNLOAD_BUF_SIZE: usize = 4096;
const APPSTORE_URL_MAX: usize = 256;

static TAG: &[u8] = b"appstore_client\0";

// ---------------------------------------------------------------------------
// Logging FFI
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;
const ESP_LOG_ERROR: i32 = 1;

// ---------------------------------------------------------------------------
// esp_http_client FFI — same API on device (esp_http_client.h) and
// simulator (sim_http.c shim).
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_http_client_init(config: *const EspHttpClientConfig) -> *mut c_void;
    fn esp_http_client_perform(client: *mut c_void) -> i32;
    fn esp_http_client_get_status_code(client: *mut c_void) -> c_int;
    fn esp_http_client_cleanup(client: *mut c_void) -> i32;
    fn esp_http_client_open(client: *mut c_void, write_len: i32) -> i32;
    fn esp_http_client_fetch_headers(client: *mut c_void) -> c_int;
    fn esp_http_client_read(client: *mut c_void, buf: *mut c_char, len: c_int) -> c_int;
    fn esp_http_client_close(client: *mut c_void) -> i32;
}

// esp_http_client_config_t — only the fields we use; must be repr(C) padded to match.
// We use a flexible approach: declare only the fields we set and pad to 128 bytes.
#[repr(C)]
struct EspHttpClientConfig {
    url: *const c_char,
    event_handler: Option<unsafe extern "C" fn(*mut EspHttpClientEvent) -> i32>,
    user_data: *mut c_void,
    timeout_ms: i32,
    _pad: [u8; 84], // Pad to match ESP-IDF struct size (~128 bytes)
}

impl EspHttpClientConfig {
    fn new(url: *const c_char, timeout_ms: i32) -> Self {
        EspHttpClientConfig {
            url,
            event_handler: None,
            user_data: std::ptr::null_mut(),
            timeout_ms,
            _pad: [0u8; 84],
        }
    }

    fn with_handler(
        url: *const c_char,
        timeout_ms: i32,
        handler: unsafe extern "C" fn(*mut EspHttpClientEvent) -> i32,
        user_data: *mut c_void,
    ) -> Self {
        EspHttpClientConfig {
            url,
            event_handler: Some(handler),
            user_data,
            timeout_ms,
            _pad: [0u8; 84],
        }
    }
}

// esp_http_client_event_t — minimal layout
#[repr(C)]
struct EspHttpClientEvent {
    event_id: i32,      // HTTP_EVENT_ON_DATA = 5
    data: *const u8,
    data_len: i32,
    user_data: *mut c_void,
    // … more fields follow in the C struct, but we only read the above
}

const HTTP_EVENT_ON_DATA: i32 = 5;

// ---------------------------------------------------------------------------
// Signing FFI
// ---------------------------------------------------------------------------

extern "C" {
    fn signing_verify_file(path: *const c_char) -> i32;
}

// ---------------------------------------------------------------------------
// SHA-256 — use the sha2 crate (pure Rust, no C dependency)
// ---------------------------------------------------------------------------

use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Catalog entry type — matches catalog_entry_t in appstore_client.h
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct CatalogEntry {
    pub id:               [u8; 64],
    pub name:             [u8; 64],
    pub version:          [u8; 16],
    pub author:           [u8; 32],
    pub description:      [u8; 256],
    pub url:              [u8; 256],
    pub sig_url:          [u8; 256],
    pub sha256_hex:       [u8; 65],
    pub permissions:      [u8; 64],
    pub min_os_version:   [u8; 16],
    pub size_bytes:       u32,
    pub entry_type:       u32, // CatalogType enum: 0=app, 1=firmware, 2=driver, 3=wm
    pub is_signed:        bool,
    /// Comma-separated board names this entry targets; empty = universal.
    pub compatible_boards: [u8; 128],

    /// Bus type for hardware detection: "i2c", "spi", "uart", or "" (none).
    pub detection_bus: [u8; 8],
    /// I2C address or SPI CS index for detection; 0 means not used.
    pub detection_address: u16,
    /// Register address to read for chip-ID verification; 0 means not used.
    pub detection_chip_id_reg: u16,
    /// Expected chip-ID register value; 0 means not used.
    pub detection_chip_id_value: u16,

    // Rich metadata — new fields (default to 0/empty for backward compat)
    /// Category string: "tools", "communication", "games", "drivers", "system"
    pub category:         [u8; 32],
    /// URL to 1-bit icon (32x32 or 48x48 PNG)
    pub icon_url:         [u8; 256],
    /// Up to 3 screenshot URLs
    pub screenshots:      [[u8; 256]; 3],
    /// Number of valid screenshot URLs (0–3)
    pub screenshot_count: u8,
    /// Average rating × 100 (e.g. 450 = 4.50 stars)
    pub rating_stars:     u16,
    /// Number of ratings
    pub rating_count:     u32,
    /// Total downloads
    pub download_count:   u32,
    /// ISO date string: "2026-03-22"
    pub updated_date:     [u8; 11],
    /// What's new in this version
    pub changelog:        [u8; 512],
}

impl Default for CatalogEntry {
    fn default() -> Self {
        CatalogEntry {
            id:               [0u8; 64],
            name:             [0u8; 64],
            version:          [0u8; 16],
            author:           [0u8; 32],
            description:      [0u8; 256],
            url:              [0u8; 256],
            sig_url:          [0u8; 256],
            sha256_hex:       [0u8; 65],
            permissions:      [0u8; 64],
            min_os_version:   [0u8; 16],
            size_bytes:       0,
            entry_type:       CATALOG_TYPE_APP,
            is_signed:        false,
            compatible_boards: [0u8; 128],
            detection_bus:    [0u8; 8],
            detection_address: 0,
            detection_chip_id_reg: 0,
            detection_chip_id_value: 0,
            category:         [0u8; 32],
            icon_url:         [0u8; 256],
            screenshots:      [[0u8; 256]; 3],
            screenshot_count: 0,
            rating_stars:     0,
            rating_count:     0,
            download_count:   0,
            updated_date:     [0u8; 11],
            changelog:        [0u8; 512],
        }
    }
}

const CATALOG_TYPE_APP:      u32 = 0;
const CATALOG_TYPE_FIRMWARE: u32 = 1;
const CATALOG_TYPE_DRIVER:   u32 = 2;
const CATALOG_TYPE_WM:       u32 = 3;

// Progress callback type
pub type DownloadProgressCb =
    unsafe extern "C" fn(downloaded: u32, total: u32, user_data: *mut c_void);

// ---------------------------------------------------------------------------
// Catalog URL config (loaded lazily from /sdcard/config/appstore.json)
// ---------------------------------------------------------------------------

static CATALOG_URL: Mutex<[u8; APPSTORE_URL_MAX]> = Mutex::new([0u8; APPSTORE_URL_MAX]);
static CATALOG_URL_LOADED: Mutex<bool> = Mutex::new(false);

fn load_catalog_url() {
    let already = CATALOG_URL_LOADED.lock().map(|v| *v).unwrap_or(false);
    if already { return; }

    let config_path = "/sdcard/config/appstore.json";
    if let Ok(content) = std::fs::read_to_string(config_path) {
        if let Some(url) = json_str_extract(&content, "catalog_url") {
            if let Ok(mut buf) = CATALOG_URL.lock() {
                let bytes = url.as_bytes();
                let len = bytes.len().min(APPSTORE_URL_MAX - 1);
                buf[..len].copy_from_slice(&bytes[..len]);
                buf[len] = 0;
            }
        }
    }

    if let Ok(mut loaded) = CATALOG_URL_LOADED.lock() {
        *loaded = true;
    }
}

/// Return the catalog URL as a null-terminated C string pointer.
///
/// # Safety
/// Returns a pointer to static storage. Do not free.
#[no_mangle]
pub extern "C" fn appstore_get_catalog_url() -> *const c_char {
    load_catalog_url();

    if let Ok(buf) = CATALOG_URL.lock() {
        if buf[0] != 0 {
            return buf.as_ptr() as *const c_char;
        }
    }

    DEFAULT_CATALOG_URL.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Minimal JSON helpers
// ---------------------------------------------------------------------------

/// Extract the string value of a JSON key from a flat JSON fragment.
fn json_str_extract(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let after_key = &json[pos + search.len()..];
    let quote_start = after_key.find('"')? + 1;
    let value_str = &after_key[quote_start..];
    let quote_end = value_str.find('"')?;
    Some(value_str[..quote_end].to_string())
}

/// Extract the integer value of a JSON key.
fn json_int_extract(json: &str, key: &str) -> Option<i64> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let after_key = &json[pos + search.len()..];
    let colon = after_key.find(':')? + 1;
    let num_str = after_key[colon..].trim_start();
    let end = num_str
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(num_str.len());
    num_str[..end].parse().ok()
}

/// Extract a JSON string array and return its values as a comma-separated string.
/// e.g., `["tdeck-pro", "tdeck"]` → `"tdeck-pro,tdeck"`.
/// Returns None if the key is absent or the value is not an array.
fn json_array_extract(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let after_key = &json[pos + search.len()..];
    let bracket = after_key.find('[')? ;
    let content_start = bracket + 1;
    let content = &after_key[content_start..];
    let bracket_end = content.find(']')?;
    let inner = &content[..bracket_end];

    let mut values = Vec::new();
    let mut rem = inner;
    loop {
        let q1 = match rem.find('"') {
            Some(i) => i,
            None => break,
        };
        let val_start = &rem[q1 + 1..];
        let q2 = match val_start.find('"') {
            Some(i) => i,
            None => break,
        };
        values.push(&val_start[..q2]);
        rem = &val_start[q2 + 1..];
    }

    if values.is_empty() {
        None
    } else {
        Some(values.join(","))
    }
}

/// Extract a string value from inside the `detection` sub-object of a catalog entry JSON fragment.
///
/// Given `{"id":"x","detection":{"bus":"i2c","address":"0x34"}}` and key `"bus"`,
/// returns `Some("i2c")`.
fn extract_detection_field(obj_json: &str, key: &str) -> Option<String> {
    // Find the detection object
    let det_key = "\"detection\"";
    let pos = obj_json.find(det_key)?;
    let after = &obj_json[pos + det_key.len()..];
    let brace = after.find('{')? + 1;
    let inner_start = &after[brace..];
    let brace_end = inner_start.find('}')?;
    let inner = &inner_start[..brace_end];
    json_str_extract(inner, key)
}

/// Extract a numeric value (hex string or decimal) from inside the `detection` sub-object.
/// Returns 0 when absent or unparseable.
fn extract_detection_u16(obj_json: &str, key: &str) -> u16 {
    let det_key = "\"detection\"";
    let pos = match obj_json.find(det_key) {
        Some(p) => p,
        None => return 0,
    };
    let after = &obj_json[pos + det_key.len()..];
    let brace = match after.find('{') { Some(i) => i + 1, None => return 0 };
    let inner_start = &after[brace..];
    let brace_end = match inner_start.find('}') { Some(i) => i, None => return 0 };
    let inner = &inner_start[..brace_end];
    json_hex_or_int_extract(inner, key)
}

/// Parse a JSON value that may be a decimal integer or a `"0x…"` hex string
/// into a `u16`.  Returns 0 when the key is absent or unparseable.
fn json_hex_or_int_extract(json: &str, key: &str) -> u16 {
    let search = format!("\"{}\"", key);
    let pos = match json.find(&search) {
        Some(p) => p,
        None => return 0,
    };
    let after_key = &json[pos + search.len()..];
    let after_colon = match after_key.trim_start().strip_prefix(':') {
        Some(s) => s.trim_start(),
        None => return 0,
    };

    if after_colon.starts_with('"') {
        // Quoted string: "0x34" or "52"
        let inner = &after_colon[1..];
        let end = match inner.find('"') { Some(i) => i, None => return 0 };
        let s = inner[..end].trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u16::from_str_radix(hex, 16).unwrap_or(0)
        } else {
            s.parse::<u16>().unwrap_or(0)
        }
    } else {
        // Bare decimal
        let num_end = after_colon
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_colon.len());
        after_colon[..num_end].parse::<u16>().unwrap_or(0)
    }
}

/// Extract a floating-point value from a JSON key. Returns None when absent.
/// Handles both integer `4` and decimal `4.5` forms.
fn json_float_extract(json: &str, key: &str) -> Option<f32> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let after_key = &json[pos + search.len()..];
    let colon = after_key.find(':')? + 1;
    let num_str = after_key[colon..].trim_start();
    let end = num_str
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(num_str.len());
    num_str[..end].parse::<f32>().ok()
}

/// Format a download count for display: exact below 1000, "1.5K" ≥1000, "1.2M" ≥1000000.
pub fn format_download_count(count: u32) -> String {
    if count >= 1_000_000 {
        let m = count as f32 / 1_000_000.0;
        // one decimal place, strip trailing ".0"
        let s = format!("{:.1}M", m);
        s
    } else if count >= 1_000 {
        let k = count as f32 / 1_000.0;
        let s = format!("{:.1}K", k);
        s
    } else {
        format!("{}", count)
    }
}

/// Render a star rating as a fixed-width ASCII string.
/// `rating_stars` is rating × 100 (e.g. 450 = 4.50).
/// Returns a 5-character string using ★ and ☆.
pub fn format_star_rating(rating_stars: u16) -> String {
    // Round to nearest half star
    let tenths = (rating_stars + 5) / 10; // rating × 10, rounded
    let full = (tenths / 10) as usize;
    let remainder = tenths % 10;
    let half = remainder >= 5;

    let mut s = String::with_capacity(15); // UTF-8: each char is 3 bytes
    for i in 0..5usize {
        if i < full {
            s.push('★');
        } else if i == full && half {
            // Use ★ for half too — e-paper can't display half-star glyphs
            s.push('★');
        } else {
            s.push('☆');
        }
    }
    s
}

/// Check whether a CatalogEntry is compatible with a given board name.
/// The `compatible_boards` field is stored as a comma-separated string.
/// An empty field means the entry is universal (compatible with all boards).
pub fn catalog_entry_is_board_compatible(entry: &CatalogEntry, board_name: &str) -> bool {
    if entry.compatible_boards[0] == 0 {
        return true; // universal
    }
    let boards_str = match std::str::from_utf8(&entry.compatible_boards) {
        Ok(s) => s.trim_end_matches('\0'),
        Err(_) => return false,
    };
    boards_str.split(',').any(|b| b == board_name)
}

/// Check whether a CatalogEntry matches a detected I2C/SPI device.
///
/// Returns true when:
/// - `detection_bus` is empty (no detection info — entry is not bus-filtered), OR
/// - `detection_bus` matches `bus` AND `detection_address` matches `address`
///   (0 in the entry means "match any address on this bus").
pub fn catalog_entry_detection_matches(entry: &CatalogEntry, bus: &str, address: u16) -> bool {
    if entry.detection_bus[0] == 0 {
        return false; // no detection info — not matched via bus probing
    }
    let entry_bus = match std::str::from_utf8(&entry.detection_bus) {
        Ok(s) => s.trim_end_matches('\0'),
        Err(_) => return false,
    };
    if entry_bus != bus {
        return false;
    }
    // address == 0 in entry means "any address" (e.g. SPI chip-ID only)
    entry.detection_address == 0 || entry.detection_address == address
}

/// Copy a Rust &str into a fixed C buffer (null-terminated, truncated).
pub fn copy_str_to_buf(src: &str, dst: &mut [u8]) {
    let bytes = src.as_bytes();
    let len = bytes.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&bytes[..len]);
    dst[len] = 0;
}

// ---------------------------------------------------------------------------
// HTTP response accumulator — for catalog fetches
// ---------------------------------------------------------------------------

struct HttpBuf {
    data: Vec<u8>,
    capacity: usize,
    overflow: bool,
}

impl HttpBuf {
    fn new(capacity: usize) -> Self {
        HttpBuf {
            data: Vec::with_capacity(capacity),
            capacity,
            overflow: false,
        }
    }
}

// http_buf_event_handler calls esp_log_write — excluded from test builds.
#[cfg(not(test))]
unsafe extern "C" fn http_buf_event_handler(evt: *mut EspHttpClientEvent) -> i32 {
    let resp = (*evt).user_data as *mut HttpBuf;
    if resp.is_null() { return ESP_OK; }
    let buf = &mut *resp;

    if (*evt).event_id == HTTP_EVENT_ON_DATA {
        let data = std::slice::from_raw_parts((*evt).data, (*evt).data_len as usize);
        if buf.data.len() + data.len() < buf.capacity {
            buf.data.extend_from_slice(data);
        } else {
            buf.overflow = true;
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"HTTP buffer overflow - truncated\0".as_ptr());
        }
    }

    ESP_OK
}

// ---------------------------------------------------------------------------
// Catalog fetch
// ---------------------------------------------------------------------------

/// Fetch the app catalog JSON and parse entries into `entries`.
///
/// # Safety
/// `entries` must point to an array of at least `max_entries` CatalogEntry.
/// `out_count` must be a valid pointer.
///
/// The full HTTP implementation is excluded from test builds (it calls
/// esp_http_client_* which are not available on aarch64-apple-darwin).
/// The test-mode body only implements the guard clauses so the NULL-pointer
/// tests can still run.
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_fetch_catalog(
    catalog_url: *const c_char,
    entries: *mut CatalogEntry,
    max_entries: c_int,
    out_count: *mut c_int,
) -> i32 {
    if entries.is_null() || out_count.is_null() || max_entries <= 0 {
        return ESP_ERR_INVALID_ARG;
    }
    *out_count = 0;

    let url_ptr = if catalog_url.is_null() || *catalog_url == 0 {
        appstore_get_catalog_url()
    } else {
        catalog_url
    };

    let url_str = match CStr::from_ptr(url_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Fetching catalog: %s\0".as_ptr(), url_ptr);

    let mut resp_buf = Box::new(HttpBuf::new(MAX_CATALOG_JSON + 1));

    let url_cstr = std::ffi::CString::new(url_str).unwrap_or_default();
    let config = EspHttpClientConfig::with_handler(
        url_cstr.as_ptr(),
        15000,
        http_buf_event_handler,
        &mut *resp_buf as *mut HttpBuf as *mut c_void,
    );

    let client = esp_http_client_init(&config);
    if client.is_null() {
        return ESP_FAIL;
    }

    let err    = esp_http_client_perform(client);
    let status = esp_http_client_get_status_code(client);
    esp_http_client_cleanup(client);

    if err != ESP_OK || status != 200 {
        esp_log_write(
            ESP_LOG_ERROR,
            TAG.as_ptr(),
            b"Catalog fetch failed: HTTP %d\0".as_ptr(),
            status,
        );
        return ESP_FAIL;
    }

    let json = match std::str::from_utf8(&resp_buf.data) {
        Ok(s) => s.to_string(),
        Err(_) => return ESP_FAIL,
    };

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Catalog fetched: %d bytes\0".as_ptr(),
        json.len() as i32,
    );

    // Parse individual { ... } objects from the JSON array
    let mut count = 0i32;
    let mut cursor = json.as_str();

    while count < max_entries {
        let obj_start = match cursor.find('{') {
            Some(i) => i,
            None => break,
        };
        cursor = &cursor[obj_start..];

        let obj_end = match cursor.find('}') {
            Some(i) => i + 1,
            None => break,
        };

        let obj = &cursor[..obj_end];

        let entry = &mut *entries.add(count as usize);
        *entry = CatalogEntry::default();

        if let Some(v) = json_str_extract(obj, "id")          { copy_str_to_buf(&v, &mut entry.id); }
        if let Some(v) = json_str_extract(obj, "name")        { copy_str_to_buf(&v, &mut entry.name); }
        if let Some(v) = json_str_extract(obj, "version")     { copy_str_to_buf(&v, &mut entry.version); }
        if let Some(v) = json_str_extract(obj, "author")      { copy_str_to_buf(&v, &mut entry.author); }
        if let Some(v) = json_str_extract(obj, "description") { copy_str_to_buf(&v, &mut entry.description); }
        if let Some(v) = json_str_extract(obj, "url")         { copy_str_to_buf(&v, &mut entry.url); }
        if let Some(v) = json_str_extract(obj, "sig_url")     { copy_str_to_buf(&v, &mut entry.sig_url); }
        if let Some(v) = json_str_extract(obj, "sha256")      { copy_str_to_buf(&v, &mut entry.sha256_hex); }
        if let Some(v) = json_str_extract(obj, "permissions") { copy_str_to_buf(&v, &mut entry.permissions); }
        if let Some(v) = json_str_extract(obj, "min_os_version") { copy_str_to_buf(&v, &mut entry.min_os_version); }

        if let Some(sz) = json_int_extract(obj, "size_bytes") {
            if sz > 0 { entry.size_bytes = sz as u32; }
        }

        if let Some(t) = json_str_extract(obj, "type") {
            entry.entry_type = match t.as_str() {
                "firmware" => CATALOG_TYPE_FIRMWARE,
                "driver"   => CATALOG_TYPE_DRIVER,
                "wm"       => CATALOG_TYPE_WM,
                _          => CATALOG_TYPE_APP,
            };
        }

        // Parse compatible_boards JSON array into comma-separated bytes
        if let Some(boards) = json_array_extract(obj, "compatible_boards") {
            copy_str_to_buf(&boards, &mut entry.compatible_boards);
        }

        // Parse detection object: {"bus":"i2c","address":"0x34",...}
        if let Some(det_bus) = extract_detection_field(obj, "bus") {
            copy_str_to_buf(&det_bus, &mut entry.detection_bus);
            entry.detection_address       = extract_detection_u16(obj, "address");
            entry.detection_chip_id_reg   = extract_detection_u16(obj, "chip_id_reg");
            entry.detection_chip_id_value = extract_detection_u16(obj, "chip_id_value");
        }

        // Rich metadata fields
        if let Some(v) = json_str_extract(obj, "category")  { copy_str_to_buf(&v, &mut entry.category); }
        if let Some(v) = json_str_extract(obj, "icon_url")  { copy_str_to_buf(&v, &mut entry.icon_url); }
        if let Some(v) = json_str_extract(obj, "changelog") { copy_str_to_buf(&v, &mut entry.changelog); }
        if let Some(v) = json_str_extract(obj, "updated")   { copy_str_to_buf(&v, &mut entry.updated_date); }

        if let Some(f) = json_float_extract(obj, "rating") {
            // Store as rating × 100, rounded
            entry.rating_stars = (f * 100.0 + 0.5) as u16;
        }
        if let Some(n) = json_int_extract(obj, "rating_count") {
            if n > 0 { entry.rating_count = n as u32; }
        }
        if let Some(n) = json_int_extract(obj, "downloads") {
            if n > 0 { entry.download_count = n as u32; }
        }

        // Screenshots array
        if let Some(screenshots_raw) = json_array_extract(obj, "screenshots") {
            let mut sc_count = 0u8;
            for (i, url) in screenshots_raw.split(',').enumerate() {
                if i >= 3 { break; }
                copy_str_to_buf(url.trim(), &mut entry.screenshots[i]);
                sc_count += 1;
            }
            entry.screenshot_count = sc_count;
        }

        entry.is_signed = entry.sig_url[0] != 0;

        // Only count entries that have at minimum an id
        if entry.id[0] != 0 {
            count += 1;
        }

        cursor = &cursor[obj_end..];
    }

    *out_count = count;

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Parsed %d catalog entries\0".as_ptr(),
        count,
    );

    ESP_OK
}

// Test-mode stub: guard clauses only; no HTTP calls.
#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_fetch_catalog(
    _catalog_url: *const c_char,
    entries: *mut CatalogEntry,
    max_entries: c_int,
    out_count: *mut c_int,
) -> i32 {
    if entries.is_null() || out_count.is_null() || max_entries <= 0 {
        return ESP_ERR_INVALID_ARG;
    }
    *out_count = 0;
    // HTTP not available on host — return NOT_SUPPORTED for non-null inputs.
    ESP_ERR_NOT_SUPPORTED
}

// ---------------------------------------------------------------------------
// File download with SHA-256 verification
// ---------------------------------------------------------------------------

/// Download a file from `url` to `dest_path`, optionally verifying SHA-256.
///
/// # Safety
/// `url` and `dest_path` must be valid null-terminated C strings.
/// `expected_sha256_hex` may be NULL (skips hash check).
/// `progress_cb` may be NULL.
///
/// Excluded from test builds (calls esp_http_client_* not available on host).
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_download_file(
    url: *const c_char,
    dest_path: *const c_char,
    expected_sha256_hex: *const c_char,
    progress_cb: Option<DownloadProgressCb>,
    user_data: *mut c_void,
) -> i32 {
    if url.is_null() || dest_path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let url_str = match CStr::from_ptr(url).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    let dest_str = match CStr::from_ptr(dest_path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Downloading %s -> %s\0".as_ptr(),
        url,
        dest_path,
    );

    let mut file = match std::fs::File::create(dest_str) {
        Ok(f) => f,
        Err(_) => {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Cannot create dest file\0".as_ptr());
            return ESP_ERR_NOT_FOUND;
        }
    };

    let mut hasher = Sha256::new();

    let url_cstr = match std::ffi::CString::new(url_str) {
        Ok(c) => c,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    let config = EspHttpClientConfig::new(url_cstr.as_ptr(), 30000);
    let client = esp_http_client_init(&config);
    if client.is_null() {
        return ESP_FAIL;
    }

    let err = esp_http_client_open(client, 0);
    if err != ESP_OK {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"HTTP open failed: %d\0".as_ptr(), err);
        esp_http_client_cleanup(client);
        return err;
    }

    let content_length = esp_http_client_fetch_headers(client);
    let total: u32 = if content_length > 0 { content_length as u32 } else { 0 };
    let mut downloaded: u32 = 0;

    let mut buf = vec![0u8; DOWNLOAD_BUF_SIZE];

    loop {
        let read_len = esp_http_client_read(client, buf.as_mut_ptr() as *mut c_char, DOWNLOAD_BUF_SIZE as c_int);
        if read_len <= 0 { break; }

        let chunk = &buf[..read_len as usize];
        hasher.update(chunk);

        if file.write_all(chunk).is_err() {
            esp_http_client_close(client);
            esp_http_client_cleanup(client);
            return ESP_FAIL;
        }

        downloaded += read_len as u32;

        if let Some(cb) = progress_cb {
            cb(downloaded, total, user_data);
        }
    }

    esp_http_client_close(client);
    esp_http_client_cleanup(client);
    drop(file);

    // Verify SHA-256 if expected hash was provided
    if !expected_sha256_hex.is_null() {
        let expected_str = match CStr::from_ptr(expected_sha256_hex).to_str() {
            Ok(s) => s,
            Err(_) => return ESP_ERR_INVALID_ARG,
        };

        if !expected_str.is_empty() {
            let hash = hasher.finalize();
            let computed: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

            if computed != expected_str {
                esp_log_write(
                    ESP_LOG_ERROR,
                    TAG.as_ptr(),
                    b"SHA-256 mismatch!\0".as_ptr(),
                );
                let _ = std::fs::remove_file(dest_str);
                return ESP_ERR_INVALID_CRC;
            }

            esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"SHA-256 verified OK\0".as_ptr());
        }
    }

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Downloaded %d bytes to %s\0".as_ptr(),
        downloaded as i32,
        dest_path,
    );

    ESP_OK
}

// Test-mode stub for appstore_download_file.
#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_download_file(
    url: *const c_char,
    dest_path: *const c_char,
    _expected_sha256_hex: *const c_char,
    _progress_cb: Option<DownloadProgressCb>,
    _user_data: *mut c_void,
) -> i32 {
    if url.is_null() || dest_path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    ESP_ERR_NOT_SUPPORTED
}

// ---------------------------------------------------------------------------
// Install a catalog entry
// ---------------------------------------------------------------------------

/// Download and install a catalog entry to the correct SD card directory.
///
/// # Safety
/// `entry` must point to a valid CatalogEntry. `progress_cb` may be NULL.
///
/// Excluded from test builds (calls appstore_download_file / esp_log_write).
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_install_entry(
    entry: *const CatalogEntry,
    progress_cb: Option<DownloadProgressCb>,
    user_data: *mut c_void,
) -> i32 {
    if entry.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let e = &*entry;

    if e.url[0] == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    // Determine destination directory and extension
    let (dir, ext) = match e.entry_type {
        CATALOG_TYPE_FIRMWARE => ("/sdcard/update",   ".bin"),
        CATALOG_TYPE_DRIVER   => ("/sdcard/drivers",  ".drv.elf"),
        CATALOG_TYPE_WM       => ("/sdcard/wm",       ".wm.elf"),
        _                     => ("/sdcard/apps",     ".app.elf"),
    };

    // Ensure destination directory exists
    let _ = std::fs::create_dir_all(dir);

    // Build destination path
    let id_str = CStr::from_ptr(e.id.as_ptr() as *const c_char)
        .to_str()
        .unwrap_or("unknown");

    let dest_path = if e.entry_type == CATALOG_TYPE_FIRMWARE {
        format!("{}/thistle_os.bin", dir)
    } else {
        format!("{}/{}{}", dir, id_str, ext)
    };

    let dest_cstr = match std::ffi::CString::new(dest_path.as_str()) {
        Ok(c) => c,
        Err(_) => return ESP_FAIL,
    };

    let url_ptr  = e.url.as_ptr() as *const c_char;
    let sha_ptr  = if e.sha256_hex[0] != 0 { e.sha256_hex.as_ptr() as *const c_char } else { std::ptr::null() };

    // Download the payload
    let ret = appstore_download_file(url_ptr, dest_cstr.as_ptr(), sha_ptr, progress_cb, user_data);
    if ret != ESP_OK {
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Payload download failed: %d\0".as_ptr(), ret);
        return ret;
    }

    // Download and verify signature if sig_url is present
    if e.sig_url[0] != 0 {
        let sig_path = format!("{}.sig", dest_path);
        let sig_cstr = match std::ffi::CString::new(sig_path.as_str()) {
            Ok(c) => c,
            Err(_) => return ESP_FAIL,
        };

        let sig_url_ptr = e.sig_url.as_ptr() as *const c_char;
        let sig_dl = appstore_download_file(sig_url_ptr, sig_cstr.as_ptr(), std::ptr::null(), None, std::ptr::null_mut());

        if sig_dl != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Signature download failed - aborting install\0".as_ptr());
            let _ = std::fs::remove_file(&dest_path);
            return ESP_FAIL;
        }

        let sig_ret = signing_verify_file(dest_cstr.as_ptr());
        if sig_ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Signature verification failed - deleting\0".as_ptr());
            let _ = std::fs::remove_file(&dest_path);
            let _ = std::fs::remove_file(format!("{}.sig", dest_path));
            return ESP_ERR_INVALID_CRC;
        }
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Signature verified OK\0".as_ptr());
    }

    let name_str = CStr::from_ptr(e.name.as_ptr() as *const c_char)
        .to_str()
        .unwrap_or("?");

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Installed '%s' -> %s\0".as_ptr(),
        name_str.as_ptr(),
        dest_cstr.as_ptr(),
    );

    ESP_OK
}

// Test-mode stub for appstore_install_entry.
#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_install_entry(
    entry: *const CatalogEntry,
    _progress_cb: Option<DownloadProgressCb>,
    _user_data: *mut c_void,
) -> i32 {
    if entry.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    ESP_ERR_NOT_SUPPORTED
}

// ---------------------------------------------------------------------------
// Rich metadata C FFI accessors
// ---------------------------------------------------------------------------

/// Return a pointer to the null-terminated category string in a CatalogEntry.
///
/// # Safety
/// `entry` must be a valid non-null pointer to a CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_entry_get_category(entry: *const CatalogEntry) -> *const u8 {
    if entry.is_null() {
        return std::ptr::null();
    }
    (*entry).category.as_ptr()
}

/// Return the average rating × 100 (e.g. 450 = 4.50 stars).
///
/// # Safety
/// `entry` must be a valid non-null pointer to a CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_entry_get_rating(entry: *const CatalogEntry) -> u16 {
    if entry.is_null() {
        return 0;
    }
    (*entry).rating_stars
}

/// Return total download count.
///
/// # Safety
/// `entry` must be a valid non-null pointer to a CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_entry_get_downloads(entry: *const CatalogEntry) -> u32 {
    if entry.is_null() {
        return 0;
    }
    (*entry).download_count
}

/// Return a pointer to the null-terminated icon URL in a CatalogEntry.
///
/// # Safety
/// `entry` must be a valid non-null pointer to a CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_entry_get_icon_url(entry: *const CatalogEntry) -> *const u8 {
    if entry.is_null() {
        return std::ptr::null();
    }
    (*entry).icon_url.as_ptr()
}

/// Return a pointer to the null-terminated changelog in a CatalogEntry.
///
/// # Safety
/// `entry` must be a valid non-null pointer to a CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_entry_get_changelog(entry: *const CatalogEntry) -> *const u8 {
    if entry.is_null() {
        return std::ptr::null();
    }
    (*entry).changelog.as_ptr()
}

// ---------------------------------------------------------------------------
// Browsing API — category filter, sort, ratings, download reporting
// ---------------------------------------------------------------------------

/// Parse a catalog JSON string and fill `entries` with only those entries
/// matching `category`. Pass "all" or an empty string to return everything.
///
/// This is the pure-Rust parser used by tests and the app store UI.
pub fn parse_catalog_entries(json: &str, category_filter: &str, entries: &mut Vec<CatalogEntry>) {
    entries.clear();

    let mut cursor = json;
    loop {
        let obj_start = match cursor.find('{') {
            Some(i) => i,
            None => break,
        };
        cursor = &cursor[obj_start..];

        let obj_end = match cursor.find('}') {
            Some(i) => i + 1,
            None => break,
        };

        let obj = &cursor[..obj_end];
        cursor = &cursor[obj_end..];

        let mut entry = CatalogEntry::default();

        if let Some(v) = json_str_extract(obj, "id")          { copy_str_to_buf(&v, &mut entry.id); }
        if entry.id[0] == 0 { continue; }

        if let Some(v) = json_str_extract(obj, "name")        { copy_str_to_buf(&v, &mut entry.name); }
        if let Some(v) = json_str_extract(obj, "version")     { copy_str_to_buf(&v, &mut entry.version); }
        if let Some(v) = json_str_extract(obj, "author")      { copy_str_to_buf(&v, &mut entry.author); }
        if let Some(v) = json_str_extract(obj, "description") { copy_str_to_buf(&v, &mut entry.description); }
        if let Some(v) = json_str_extract(obj, "url")         { copy_str_to_buf(&v, &mut entry.url); }
        if let Some(v) = json_str_extract(obj, "sig_url")     { copy_str_to_buf(&v, &mut entry.sig_url); }
        if let Some(v) = json_str_extract(obj, "sha256")      { copy_str_to_buf(&v, &mut entry.sha256_hex); }
        if let Some(v) = json_str_extract(obj, "permissions") { copy_str_to_buf(&v, &mut entry.permissions); }
        if let Some(v) = json_str_extract(obj, "min_os_version") { copy_str_to_buf(&v, &mut entry.min_os_version); }
        if let Some(v) = json_str_extract(obj, "category")    { copy_str_to_buf(&v, &mut entry.category); }
        if let Some(v) = json_str_extract(obj, "icon_url")    { copy_str_to_buf(&v, &mut entry.icon_url); }
        if let Some(v) = json_str_extract(obj, "changelog")   { copy_str_to_buf(&v, &mut entry.changelog); }
        if let Some(v) = json_str_extract(obj, "updated")     { copy_str_to_buf(&v, &mut entry.updated_date); }

        if let Some(sz) = json_int_extract(obj, "size_bytes") {
            if sz > 0 { entry.size_bytes = sz as u32; }
        }
        if let Some(f) = json_float_extract(obj, "rating") {
            entry.rating_stars = (f * 100.0 + 0.5) as u16;
        }
        if let Some(n) = json_int_extract(obj, "rating_count") {
            if n > 0 { entry.rating_count = n as u32; }
        }
        if let Some(n) = json_int_extract(obj, "downloads") {
            if n > 0 { entry.download_count = n as u32; }
        }
        if let Some(t) = json_str_extract(obj, "type") {
            entry.entry_type = match t.as_str() {
                "firmware" => CATALOG_TYPE_FIRMWARE,
                "driver"   => CATALOG_TYPE_DRIVER,
                "wm"       => CATALOG_TYPE_WM,
                _          => CATALOG_TYPE_APP,
            };
        }
        if let Some(boards) = json_array_extract(obj, "compatible_boards") {
            copy_str_to_buf(&boards, &mut entry.compatible_boards);
        }
        if let Some(screenshots_raw) = json_array_extract(obj, "screenshots") {
            let mut sc_count = 0u8;
            for (i, url) in screenshots_raw.split(',').enumerate() {
                if i >= 3 { break; }
                copy_str_to_buf(url.trim(), &mut entry.screenshots[i]);
                sc_count += 1;
            }
            entry.screenshot_count = sc_count;
        }
        if let Some(det_bus) = extract_detection_field(obj, "bus") {
            copy_str_to_buf(&det_bus, &mut entry.detection_bus);
            entry.detection_address       = extract_detection_u16(obj, "address");
            entry.detection_chip_id_reg   = extract_detection_u16(obj, "chip_id_reg");
            entry.detection_chip_id_value = extract_detection_u16(obj, "chip_id_value");
        }
        entry.is_signed = entry.sig_url[0] != 0;

        // Category filter
        if !category_filter.is_empty() && category_filter != "all" {
            let cat = std::str::from_utf8(&entry.category)
                .unwrap_or("")
                .trim_end_matches('\0');
            if cat != category_filter {
                continue;
            }
        }

        entries.push(entry);
    }
}

/// Fetch catalog and fill only entries matching `category`.
/// Pass "all" or NULL for no filter. Pure guard/stub in test builds.
///
/// # Safety
/// `entries` must point to an array of at least `max_entries` CatalogEntry.
/// `out_count` must be a valid pointer. `category` may be NULL (= all).
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_fetch_by_category(
    catalog_url: *const c_char,
    category: *const c_char,
    entries: *mut CatalogEntry,
    max_entries: u32,
    out_count: *mut u32,
) -> i32 {
    if entries.is_null() || out_count.is_null() || max_entries == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    *out_count = 0;

    let cat_str = if category.is_null() || *category == 0 {
        "all"
    } else {
        match CStr::from_ptr(category).to_str() {
            Ok(s) => s,
            Err(_) => return ESP_ERR_INVALID_ARG,
        }
    };

    // Reuse appstore_fetch_catalog to get the full list
    let mut tmp_entries: Vec<CatalogEntry> = (0..max_entries as usize)
        .map(|_| CatalogEntry::default())
        .collect();
    let mut total: c_int = 0;
    let rc = appstore_fetch_catalog(
        catalog_url,
        tmp_entries.as_mut_ptr(),
        max_entries as c_int,
        &mut total,
    );
    if rc != ESP_OK {
        return rc;
    }

    let mut out_idx = 0u32;
    for i in 0..total as usize {
        if out_idx >= max_entries { break; }
        let e = &tmp_entries[i];
        if cat_str != "all" {
            let cat = std::str::from_utf8(&e.category)
                .unwrap_or("")
                .trim_end_matches('\0');
            if cat != cat_str {
                continue;
            }
        }
        *entries.add(out_idx as usize) = CatalogEntry {
            id: e.id, name: e.name, version: e.version, author: e.author,
            description: e.description, url: e.url, sig_url: e.sig_url,
            sha256_hex: e.sha256_hex, permissions: e.permissions,
            min_os_version: e.min_os_version, size_bytes: e.size_bytes,
            entry_type: e.entry_type, is_signed: e.is_signed,
            compatible_boards: e.compatible_boards,
            detection_bus: e.detection_bus,
            detection_address: e.detection_address,
            detection_chip_id_reg: e.detection_chip_id_reg,
            detection_chip_id_value: e.detection_chip_id_value,
            category: e.category, icon_url: e.icon_url,
            screenshots: e.screenshots, screenshot_count: e.screenshot_count,
            rating_stars: e.rating_stars, rating_count: e.rating_count,
            download_count: e.download_count, updated_date: e.updated_date,
            changelog: e.changelog,
        };
        out_idx += 1;
    }
    *out_count = out_idx;
    ESP_OK
}

#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_fetch_by_category(
    _catalog_url: *const c_char,
    _category: *const c_char,
    entries: *mut CatalogEntry,
    max_entries: u32,
    out_count: *mut u32,
) -> i32 {
    if entries.is_null() || out_count.is_null() || max_entries == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    *out_count = 0;
    ESP_ERR_NOT_SUPPORTED
}

// Sort field constants
pub const SORT_BY_NAME:      u32 = 0;
pub const SORT_BY_RATING:    u32 = 1;
pub const SORT_BY_DOWNLOADS: u32 = 2;
pub const SORT_BY_UPDATED:   u32 = 3;

/// Sort a slice of CatalogEntry in place.
///
/// `sort_by`: 0=name, 1=rating, 2=downloads, 3=updated
/// `ascending`: true = A→Z / low→high, false = Z→A / high→low
pub fn sort_entries_slice(entries: &mut [CatalogEntry], sort_by: u32, ascending: bool) {
    entries.sort_by(|a, b| {
        let ord = match sort_by {
            SORT_BY_RATING => a.rating_stars.cmp(&b.rating_stars),
            SORT_BY_DOWNLOADS => a.download_count.cmp(&b.download_count),
            SORT_BY_UPDATED => {
                let da = std::str::from_utf8(&a.updated_date).unwrap_or("").trim_end_matches('\0');
                let db = std::str::from_utf8(&b.updated_date).unwrap_or("").trim_end_matches('\0');
                da.cmp(db)
            }
            _ => {
                // name
                let na = std::str::from_utf8(&a.name).unwrap_or("").trim_end_matches('\0');
                let nb = std::str::from_utf8(&b.name).unwrap_or("").trim_end_matches('\0');
                na.cmp(nb)
            }
        };
        if ascending { ord } else { ord.reverse() }
    });
}

/// Sort a C array of CatalogEntry in place.
///
/// # Safety
/// `entries` must point to an array of at least `count` CatalogEntry.
#[no_mangle]
pub unsafe extern "C" fn appstore_sort_entries(
    entries: *mut CatalogEntry,
    count: u32,
    sort_by: u32,
    ascending: bool,
) -> i32 {
    if entries.is_null() || count == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let slice = std::slice::from_raw_parts_mut(entries, count as usize);
    sort_entries_slice(slice, sort_by, ascending);
    ESP_OK
}

/// Submit a star rating for an entry (POST request to the API).
/// Stars must be 1–5. No-op stub in test builds (HTTP not available on host).
///
/// # Safety
/// `api_url` and `entry_id` must be valid null-terminated C strings.
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_submit_rating(
    api_url: *const c_char,
    entry_id: *const c_char,
    stars: u8,
) -> i32 {
    if api_url.is_null() || entry_id.is_null() || stars == 0 || stars > 5 {
        return ESP_ERR_INVALID_ARG;
    }

    let url_str = match CStr::from_ptr(api_url).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let id_str = match CStr::from_ptr(entry_id).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    let body = format!("{{\"id\":\"{}\",\"stars\":{}}}", id_str, stars);
    let endpoint = format!("{}/rate", url_str.trim_end_matches('/'));

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Submitting rating %d for %s\0".as_ptr(),
        stars as i32,
        entry_id,
    );

    let endpoint_cstr = match std::ffi::CString::new(endpoint.as_str()) {
        Ok(c) => c,
        Err(_) => return ESP_FAIL,
    };

    let config = EspHttpClientConfig::new(endpoint_cstr.as_ptr(), 10000);
    let client = esp_http_client_init(&config);
    if client.is_null() {
        return ESP_FAIL;
    }

    let open_rc = esp_http_client_open(client, body.len() as i32);
    if open_rc != ESP_OK {
        esp_http_client_cleanup(client);
        return ESP_FAIL;
    }

    // Write the POST body
    let written = esp_http_client_read(client, body.as_ptr() as *mut c_char, body.len() as c_int);
    esp_http_client_close(client);
    esp_http_client_cleanup(client);

    if written < 0 {
        return ESP_FAIL;
    }
    ESP_OK
}

#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_submit_rating(
    api_url: *const c_char,
    entry_id: *const c_char,
    stars: u8,
) -> i32 {
    if api_url.is_null() || entry_id.is_null() || stars == 0 || stars > 5 {
        return ESP_ERR_INVALID_ARG;
    }
    ESP_ERR_NOT_SUPPORTED
}

/// Notify the server of a download (POST request; increments download counter).
///
/// # Safety
/// `api_url` and `entry_id` must be valid null-terminated C strings.
#[no_mangle]
#[cfg(not(test))]
pub unsafe extern "C" fn appstore_report_download(
    api_url: *const c_char,
    entry_id: *const c_char,
) -> i32 {
    if api_url.is_null() || entry_id.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let url_str = match CStr::from_ptr(api_url).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let id_str = match CStr::from_ptr(entry_id).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    let body = format!("{{\"id\":\"{}\"}}", id_str);
    let endpoint = format!("{}/download", url_str.trim_end_matches('/'));

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"Reporting download for %s\0".as_ptr(),
        entry_id,
    );

    let endpoint_cstr = match std::ffi::CString::new(endpoint.as_str()) {
        Ok(c) => c,
        Err(_) => return ESP_FAIL,
    };

    let config = EspHttpClientConfig::new(endpoint_cstr.as_ptr(), 10000);
    let client = esp_http_client_init(&config);
    if client.is_null() {
        return ESP_FAIL;
    }

    let open_rc = esp_http_client_open(client, body.len() as i32);
    if open_rc != ESP_OK {
        esp_http_client_cleanup(client);
        return ESP_FAIL;
    }

    let written = esp_http_client_read(client, body.as_ptr() as *mut c_char, body.len() as c_int);
    esp_http_client_close(client);
    esp_http_client_cleanup(client);

    if written < 0 {
        return ESP_FAIL;
    }
    ESP_OK
}

#[no_mangle]
#[cfg(test)]
pub unsafe extern "C" fn appstore_report_download(
    api_url: *const c_char,
    entry_id: *const c_char,
) -> i32 {
    if api_url.is_null() || entry_id.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    ESP_ERR_NOT_SUPPORTED
}

// ---------------------------------------------------------------------------
// Tests
//
// appstore_fetch_catalog(), appstore_download_file(), and
// appstore_install_entry() all call esp_http_client_init (or esp_log_write)
// and are not safe on aarch64-apple-darwin. Test builds use guard-only stubs.
//
// The following are pure Rust and tested here:
//   appstore_get_catalog_url()  — reads a static buffer / default URL
//   json_str_extract()          — pure Rust string parser
//   json_int_extract()          — pure Rust string parser
//   copy_str_to_buf()           — pure Rust buffer helper
//   CatalogEntry field sizes    — compile-time layout checks
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // -----------------------------------------------------------------------
    // test_get_catalog_url_contains_thistle_apps
    // Mirrors test_appstore_client.c: URL must contain "thistle-apps".
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_catalog_url_contains_thistle_apps() {
        let ptr = appstore_get_catalog_url();
        assert!(!ptr.is_null(), "appstore_get_catalog_url() must not return NULL");
        let url = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert!(
            url.contains("thistle-apps"),
            "catalog URL must contain \"thistle-apps\", got: {}", url
        );
    }

    // -----------------------------------------------------------------------
    // test_get_catalog_url_non_empty
    // Mirrors test_appstore_client.c: URL must be a non-empty string.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_catalog_url_non_empty() {
        let ptr = appstore_get_catalog_url();
        let url = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert!(!url.is_empty(), "catalog URL must not be empty");
    }

    // -----------------------------------------------------------------------
    // test_json_str_extract_simple
    // Mirrors test_appstore_client.c: extract a string value by key.
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_str_extract_simple() {
        let json = r#"{"name":"my_app","version":"1.2.3"}"#;
        assert_eq!(json_str_extract(json, "name"),    Some("my_app".to_string()));
        assert_eq!(json_str_extract(json, "version"), Some("1.2.3".to_string()));
        assert_eq!(json_str_extract(json, "missing"), None);
    }

    // -----------------------------------------------------------------------
    // test_json_str_extract_url
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_str_extract_url() {
        let json = r#"{"catalog_url":"https://example.com/catalog.json"}"#;
        let result = json_str_extract(json, "catalog_url");
        assert_eq!(result, Some("https://example.com/catalog.json".to_string()));
    }

    // -----------------------------------------------------------------------
    // test_json_int_extract
    // Mirrors test_appstore_client.c: extract an integer value by key.
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_int_extract_simple() {
        let json = r#"{"size_bytes":102400,"entry_type":0}"#;
        assert_eq!(json_int_extract(json, "size_bytes"), Some(102400i64));
        assert_eq!(json_int_extract(json, "entry_type"), Some(0i64));
        assert_eq!(json_int_extract(json, "missing"),    None);
    }

    // -----------------------------------------------------------------------
    // test_json_int_extract_negative
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_int_extract_negative() {
        let json = r#"{"rssi":-72}"#;
        assert_eq!(json_int_extract(json, "rssi"), Some(-72i64));
    }

    // -----------------------------------------------------------------------
    // test_copy_str_to_buf_truncates
    // copy_str_to_buf must null-terminate and not overflow the buffer.
    // -----------------------------------------------------------------------

    #[test]
    fn test_copy_str_to_buf_truncates() {
        let mut buf = [0xFFu8; 8];
        copy_str_to_buf("hello world", &mut buf);
        // "hello w" fits (7 chars), then NUL at buf[7]
        assert_eq!(&buf[..7], b"hello w");
        assert_eq!(buf[7], 0, "buffer must be null-terminated");
    }

    // -----------------------------------------------------------------------
    // test_copy_str_to_buf_exact_fit
    // -----------------------------------------------------------------------

    #[test]
    fn test_copy_str_to_buf_exact_fit() {
        let mut buf = [0xFFu8; 6];
        copy_str_to_buf("hello", &mut buf);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    // -----------------------------------------------------------------------
    // test_catalog_entry_field_sizes
    // Mirrors test_appstore_client.c: field sizes must match header constants.
    // -----------------------------------------------------------------------

    #[test]
    fn test_catalog_entry_field_sizes() {
        let e = CatalogEntry::default();
        assert_eq!(e.url.len(),              APPSTORE_URL_MAX, "url field must be APPSTORE_URL_MAX bytes");
        assert_eq!(e.sig_url.len(),          APPSTORE_URL_MAX, "sig_url field must be APPSTORE_URL_MAX bytes");
        assert_eq!(e.sha256_hex.len(),       65,               "sha256_hex must be 65 bytes (64 hex + NUL)");
        assert_eq!(e.compatible_boards.len(), 128,             "compatible_boards field must be 128 bytes");
        assert_eq!(e.detection_bus.len(),    8,                "detection_bus field must be 8 bytes");
        assert_eq!(e.category.len(),         32,               "category field must be 32 bytes");
        assert_eq!(e.icon_url.len(),         256,              "icon_url field must be 256 bytes");
        assert_eq!(e.screenshots.len(),      3,                "screenshots must hold 3 entries");
        assert_eq!(e.screenshots[0].len(),   256,              "each screenshot URL must be 256 bytes");
        assert_eq!(e.updated_date.len(),     11,               "updated_date must be 11 bytes");
        assert_eq!(e.changelog.len(),        512,              "changelog must be 512 bytes");
    }

    // -----------------------------------------------------------------------
    // test_fetch_catalog_null_entries_returns_invalid_arg
    // Mirrors test_appstore_client.c: NULL entries pointer returns INVALID_ARG.
    // This only tests the guard clause (no HTTP call).
    // -----------------------------------------------------------------------

    #[test]
    fn test_fetch_catalog_null_entries_returns_invalid_arg() {
        let mut count: c_int = 0;
        let rc = unsafe {
            appstore_fetch_catalog(
                std::ptr::null(),
                std::ptr::null_mut(),
                10,
                &mut count as *mut c_int,
            )
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "NULL entries must return ESP_ERR_INVALID_ARG");
    }

    // -----------------------------------------------------------------------
    // test_fetch_catalog_null_out_count_returns_invalid_arg
    // -----------------------------------------------------------------------

    #[test]
    fn test_fetch_catalog_null_out_count_returns_invalid_arg() {
        let mut entries = [CatalogEntry::default()];
        let rc = unsafe {
            appstore_fetch_catalog(
                std::ptr::null(),
                entries.as_mut_ptr(),
                1,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "NULL out_count must return ESP_ERR_INVALID_ARG");
    }

    // -----------------------------------------------------------------------
    // test_json_array_extract
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_array_extract_two_boards() {
        let json = r#"{"compatible_boards": ["tdeck-pro", "tdeck"]}"#;
        let result = json_array_extract(json, "compatible_boards");
        assert_eq!(result, Some("tdeck-pro,tdeck".to_string()));
    }

    #[test]
    fn test_json_array_extract_empty_array() {
        let json = r#"{"compatible_boards": []}"#;
        let result = json_array_extract(json, "compatible_boards");
        assert_eq!(result, None, "empty array should return None");
    }

    #[test]
    fn test_json_array_extract_absent_key() {
        let json = r#"{"name": "foo"}"#;
        let result = json_array_extract(json, "compatible_boards");
        assert_eq!(result, None, "absent key should return None");
    }

    // -----------------------------------------------------------------------
    // test_catalog_entry_is_board_compatible
    // -----------------------------------------------------------------------

    fn make_empty_entry() -> CatalogEntry {
        let mut e = CatalogEntry::default();
        e.entry_type = CATALOG_TYPE_DRIVER;
        e
    }

    #[test]
    fn test_board_compatible_empty_is_universal() {
        let e = make_empty_entry();
        assert!(catalog_entry_is_board_compatible(&e, "tdeck-pro"), "empty = universal");
        assert!(catalog_entry_is_board_compatible(&e, "any-board"),  "empty = universal");
    }

    #[test]
    fn test_board_compatible_match() {
        let mut e = make_empty_entry();
        copy_str_to_buf("tdeck-pro,tdeck", &mut e.compatible_boards);
        assert!( catalog_entry_is_board_compatible(&e, "tdeck-pro"));
        assert!( catalog_entry_is_board_compatible(&e, "tdeck"));
        assert!(!catalog_entry_is_board_compatible(&e, "esp32-devkit"));
    }

    #[test]
    fn test_catalog_type_wm_constant() {
        assert_eq!(CATALOG_TYPE_WM, 3, "WM type must be 3");
    }

    // -----------------------------------------------------------------------
    // detection field parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_detection_field_i2c() {
        let obj = r#"{"id":"x","detection":{"bus":"i2c","address":"0x34"}}"#;
        assert_eq!(extract_detection_field(obj, "bus"), Some("i2c".to_string()));
        assert_eq!(extract_detection_u16(obj, "address"), 0x34);
    }

    #[test]
    fn test_extract_detection_field_spi() {
        let obj = r#"{"id":"x","detection":{"bus":"spi","chip_id_reg":"0x0320","chip_id_value":"0x0058"}}"#;
        assert_eq!(extract_detection_field(obj, "bus"), Some("spi".to_string()));
        assert_eq!(extract_detection_u16(obj, "chip_id_reg"),   0x0320);
        assert_eq!(extract_detection_u16(obj, "chip_id_value"), 0x0058);
    }

    #[test]
    fn test_extract_detection_absent_returns_none_zero() {
        let obj = r#"{"id":"x","compatible_boards":["tdeck-pro"]}"#;
        assert_eq!(extract_detection_field(obj, "bus"), None);
        assert_eq!(extract_detection_u16(obj, "address"), 0);
    }

    #[test]
    fn test_catalog_entry_detection_matches_bus_and_address() {
        let mut e = make_empty_entry();
        copy_str_to_buf("i2c", &mut e.detection_bus);
        e.detection_address = 0x34;
        assert!( catalog_entry_detection_matches(&e, "i2c", 0x34));
        assert!(!catalog_entry_detection_matches(&e, "i2c", 0x1A));
        assert!(!catalog_entry_detection_matches(&e, "spi", 0x34));
    }

    #[test]
    fn test_catalog_entry_detection_zero_address_matches_any() {
        let mut e = make_empty_entry();
        copy_str_to_buf("spi", &mut e.detection_bus);
        e.detection_address = 0; // SPI: no address, just chip-ID
        assert!(catalog_entry_detection_matches(&e, "spi", 0));
        assert!(catalog_entry_detection_matches(&e, "spi", 1));
    }

    #[test]
    fn test_catalog_entry_detection_no_bus_not_matched() {
        let e = make_empty_entry(); // detection_bus is all zeros
        assert!(!catalog_entry_detection_matches(&e, "i2c", 0x34));
    }

    // -----------------------------------------------------------------------
    // New rich metadata tests
    // -----------------------------------------------------------------------

    // test_catalog_entry_default_new_fields — new fields default to zero/empty
    #[test]
    fn test_catalog_entry_default_new_fields() {
        let e = CatalogEntry::default();
        assert_eq!(e.category[0], 0, "category must default to empty");
        assert_eq!(e.icon_url[0], 0, "icon_url must default to empty");
        assert_eq!(e.rating_stars, 0, "rating_stars must default to 0");
        assert_eq!(e.rating_count, 0, "rating_count must default to 0");
        assert_eq!(e.download_count, 0, "download_count must default to 0");
        assert_eq!(e.screenshot_count, 0, "screenshot_count must default to 0");
        assert_eq!(e.changelog[0], 0, "changelog must default to empty");
        assert_eq!(e.updated_date[0], 0, "updated_date must default to empty");
    }

    // test_parse_catalog_entries_rich_fields — full rich entry parsing
    #[test]
    fn test_parse_catalog_entries_rich_fields() {
        let json = r#"[
            {
                "id": "com.thistle.messenger",
                "type": "app",
                "name": "Messenger",
                "version": "2.1.0",
                "author": "ThistleOS",
                "description": "LoRa, SMS, and BLE messaging",
                "category": "communication",
                "rating": 4.5,
                "rating_count": 127,
                "downloads": 1523,
                "updated": "2026-03-22",
                "changelog": "Added LoRa mesh support",
                "permissions": "radio,ipc,storage",
                "url": "https://example.com/messenger.app.elf",
                "size_bytes": 65536
            }
        ]"#;

        let mut entries = Vec::new();
        parse_catalog_entries(json, "all", &mut entries);
        assert_eq!(entries.len(), 1, "should parse 1 entry");

        let e = &entries[0];

        let id = std::str::from_utf8(&e.id).unwrap().trim_end_matches('\0');
        assert_eq!(id, "com.thistle.messenger");

        let cat = std::str::from_utf8(&e.category).unwrap().trim_end_matches('\0');
        assert_eq!(cat, "communication");

        // 4.5 × 100 = 450
        assert_eq!(e.rating_stars, 450, "rating_stars should be 450 for 4.5 stars");
        assert_eq!(e.rating_count, 127);
        assert_eq!(e.download_count, 1523);

        let date = std::str::from_utf8(&e.updated_date).unwrap().trim_end_matches('\0');
        assert_eq!(date, "2026-03-22");

        let cl = std::str::from_utf8(&e.changelog).unwrap().trim_end_matches('\0');
        assert_eq!(cl, "Added LoRa mesh support");

        assert_eq!(e.size_bytes, 65536);
        assert_eq!(e.entry_type, CATALOG_TYPE_APP);
    }

    // test_parse_catalog_entries_category_filter
    #[test]
    fn test_parse_catalog_entries_category_filter() {
        let json = r#"[
            {"id": "app1", "name": "App1", "category": "tools", "url": "https://x.com/a.elf"},
            {"id": "app2", "name": "App2", "category": "communication", "url": "https://x.com/b.elf"},
            {"id": "app3", "name": "App3", "category": "tools", "url": "https://x.com/c.elf"}
        ]"#;

        let mut entries = Vec::new();
        parse_catalog_entries(json, "tools", &mut entries);
        assert_eq!(entries.len(), 2, "filter=tools should return 2 entries");

        let mut entries_all = Vec::new();
        parse_catalog_entries(json, "all", &mut entries_all);
        assert_eq!(entries_all.len(), 3, "filter=all should return all 3 entries");

        let mut entries_empty = Vec::new();
        parse_catalog_entries(json, "", &mut entries_empty);
        assert_eq!(entries_empty.len(), 3, "empty filter should return all 3 entries");
    }

    // test_parse_catalog_entries_old_format_compat — entries without new fields parse fine
    #[test]
    fn test_parse_catalog_entries_old_format_compat() {
        let json = r#"[
            {
                "id": "com.thistle.os",
                "type": "firmware",
                "name": "ThistleOS",
                "version": "0.2.0",
                "url": "https://example.com/thistle_os_s3.bin",
                "size_bytes": 4194304
            }
        ]"#;

        let mut entries = Vec::new();
        parse_catalog_entries(json, "all", &mut entries);
        assert_eq!(entries.len(), 1);

        let e = &entries[0];
        assert_eq!(e.rating_stars, 0, "old entries must default rating_stars to 0");
        assert_eq!(e.download_count, 0, "old entries must default download_count to 0");
        assert_eq!(e.category[0], 0, "old entries must default category to empty");
    }

    // test_sort_entries_by_rating
    #[test]
    fn test_sort_entries_by_rating() {
        let mut entries = vec![
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("app_a", &mut e.id);
                e.rating_stars = 300; // 3.0
                e
            },
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("app_b", &mut e.id);
                e.rating_stars = 480; // 4.8
                e
            },
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("app_c", &mut e.id);
                e.rating_stars = 410; // 4.1
                e
            },
        ];

        sort_entries_slice(&mut entries, SORT_BY_RATING, false); // high → low
        let ids: Vec<&str> = entries.iter().map(|e| {
            std::str::from_utf8(&e.id).unwrap().trim_end_matches('\0')
        }).collect();
        assert_eq!(ids, vec!["app_b", "app_c", "app_a"], "sort by rating desc");

        sort_entries_slice(&mut entries, SORT_BY_RATING, true); // low → high
        let ids: Vec<&str> = entries.iter().map(|e| {
            std::str::from_utf8(&e.id).unwrap().trim_end_matches('\0')
        }).collect();
        assert_eq!(ids, vec!["app_a", "app_c", "app_b"], "sort by rating asc");
    }

    // test_sort_entries_by_downloads
    #[test]
    fn test_sort_entries_by_downloads() {
        let mut entries = vec![
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("x", &mut e.id);
                e.download_count = 500;
                e
            },
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("y", &mut e.id);
                e.download_count = 10000;
                e
            },
            {
                let mut e = CatalogEntry::default();
                copy_str_to_buf("z", &mut e.id);
                e.download_count = 1500;
                e
            },
        ];

        sort_entries_slice(&mut entries, SORT_BY_DOWNLOADS, false);
        assert_eq!(entries[0].download_count, 10000);
        assert_eq!(entries[1].download_count, 1500);
        assert_eq!(entries[2].download_count, 500);
    }

    // test_sort_entries_by_name
    #[test]
    fn test_sort_entries_by_name() {
        let mut entries = vec![
            { let mut e = CatalogEntry::default(); copy_str_to_buf("Zebra", &mut e.name); e },
            { let mut e = CatalogEntry::default(); copy_str_to_buf("Apple", &mut e.name); e },
            { let mut e = CatalogEntry::default(); copy_str_to_buf("Mango", &mut e.name); e },
        ];

        sort_entries_slice(&mut entries, SORT_BY_NAME, true);
        let names: Vec<&str> = entries.iter().map(|e| {
            std::str::from_utf8(&e.name).unwrap().trim_end_matches('\0')
        }).collect();
        assert_eq!(names, vec!["Apple", "Mango", "Zebra"]);
    }

    // test_format_download_count — <1000, >=1000, >=1000000
    #[test]
    fn test_format_download_count() {
        assert_eq!(format_download_count(0),       "0");
        assert_eq!(format_download_count(999),     "999");
        assert_eq!(format_download_count(1000),    "1.0K");
        assert_eq!(format_download_count(1523),    "1.5K");
        assert_eq!(format_download_count(2341),    "2.3K");
        assert_eq!(format_download_count(1_000_000), "1.0M");
        assert_eq!(format_download_count(1_200_000), "1.2M");
    }

    // test_format_star_rating — verify star rendering
    #[test]
    fn test_format_star_rating() {
        // 5 stars
        assert_eq!(format_star_rating(500), "★★★★★");
        // 4 stars
        assert_eq!(format_star_rating(400), "★★★★☆");
        // 4.5 → rounds to 5 filled (half treated as full for e-paper)
        // 450 → tenths=45 → full=4 half=true → ★★★★★
        assert_eq!(format_star_rating(450), "★★★★★");
        // 3 stars
        assert_eq!(format_star_rating(300), "★★★☆☆");
        // 0 stars
        assert_eq!(format_star_rating(0),   "☆☆☆☆☆");
        // 1 star
        assert_eq!(format_star_rating(100), "★☆☆☆☆");
    }

    // test_json_float_extract
    #[test]
    fn test_json_float_extract() {
        let json = r#"{"rating": 4.5, "score": 3}"#;
        let r = json_float_extract(json, "rating");
        assert!(r.is_some());
        let v = r.unwrap();
        assert!((v - 4.5).abs() < 0.01, "expected 4.5, got {}", v);

        let r2 = json_float_extract(json, "score");
        assert!(r2.is_some());
        assert!((r2.unwrap() - 3.0).abs() < 0.01);

        assert!(json_float_extract(json, "missing").is_none());
    }

    // test_appstore_entry_accessors — FFI accessors
    #[test]
    fn test_appstore_entry_accessors() {
        let mut e = CatalogEntry::default();
        copy_str_to_buf("communication", &mut e.category);
        e.rating_stars = 420;
        e.download_count = 1234;

        unsafe {
            let cat_ptr = appstore_entry_get_category(&e as *const CatalogEntry);
            let cat = CStr::from_ptr(cat_ptr as *const c_char).to_str().unwrap();
            assert_eq!(cat, "communication");

            let rating = appstore_entry_get_rating(&e as *const CatalogEntry);
            assert_eq!(rating, 420);

            let dl = appstore_entry_get_downloads(&e as *const CatalogEntry);
            assert_eq!(dl, 1234);
        }
    }

    // test_appstore_sort_entries_c_api — test C-callable sort
    #[test]
    fn test_appstore_sort_entries_c_api() {
        let mut entries = vec![
            { let mut e = CatalogEntry::default(); e.download_count = 100; e },
            { let mut e = CatalogEntry::default(); e.download_count = 500; e },
            { let mut e = CatalogEntry::default(); e.download_count = 200; e },
        ];

        let rc = unsafe {
            appstore_sort_entries(entries.as_mut_ptr(), entries.len() as u32, SORT_BY_DOWNLOADS, false)
        };
        assert_eq!(rc, ESP_OK);
        assert_eq!(entries[0].download_count, 500);
        assert_eq!(entries[1].download_count, 200);
        assert_eq!(entries[2].download_count, 100);
    }

    // test_appstore_sort_entries_null_returns_invalid_arg
    #[test]
    fn test_appstore_sort_entries_null_returns_invalid_arg() {
        let rc = unsafe { appstore_sort_entries(std::ptr::null_mut(), 0, SORT_BY_NAME, true) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // test_appstore_submit_rating_invalid_stars
    #[test]
    fn test_appstore_submit_rating_invalid_stars() {
        let url = b"https://example.com/api\0";
        let id  = b"com.thistle.test\0";
        let rc = unsafe {
            appstore_submit_rating(
                url.as_ptr() as *const c_char,
                id.as_ptr() as *const c_char,
                6, // invalid: > 5
            )
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // test_appstore_report_download_null_args
    #[test]
    fn test_appstore_report_download_null_args() {
        let rc = unsafe {
            appstore_report_download(std::ptr::null(), std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }
}
