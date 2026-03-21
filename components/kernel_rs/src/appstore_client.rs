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
    pub id:              [u8; 64],
    pub name:            [u8; 64],
    pub version:         [u8; 16],
    pub author:          [u8; 32],
    pub description:     [u8; 256],
    pub url:             [u8; 256],
    pub sig_url:         [u8; 256],
    pub sha256_hex:      [u8; 65],
    pub permissions:     [u8; 64],
    pub min_os_version:  [u8; 16],
    pub size_bytes:      u32,
    pub entry_type:      u32, // CatalogType enum: 0=app, 1=firmware, 2=driver
    pub is_signed:       bool,
}

const CATALOG_TYPE_APP:      u32 = 0;
const CATALOG_TYPE_FIRMWARE: u32 = 1;
const CATALOG_TYPE_DRIVER:   u32 = 2;

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

/// Copy a Rust &str into a fixed C buffer (null-terminated, truncated).
fn copy_str_to_buf(src: &str, dst: &mut [u8]) {
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
#[no_mangle]
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
        entry.id            = [0u8; 64];
        entry.name          = [0u8; 64];
        entry.version       = [0u8; 16];
        entry.author        = [0u8; 32];
        entry.description   = [0u8; 256];
        entry.url           = [0u8; 256];
        entry.sig_url       = [0u8; 256];
        entry.sha256_hex    = [0u8; 65];
        entry.permissions   = [0u8; 64];
        entry.min_os_version = [0u8; 16];
        entry.size_bytes    = 0;
        entry.entry_type    = CATALOG_TYPE_APP;
        entry.is_signed     = false;

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
                _          => CATALOG_TYPE_APP,
            };
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

// ---------------------------------------------------------------------------
// File download with SHA-256 verification
// ---------------------------------------------------------------------------

/// Download a file from `url` to `dest_path`, optionally verifying SHA-256.
///
/// # Safety
/// `url` and `dest_path` must be valid null-terminated C strings.
/// `expected_sha256_hex` may be NULL (skips hash check).
/// `progress_cb` may be NULL.
#[no_mangle]
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

// ---------------------------------------------------------------------------
// Install a catalog entry
// ---------------------------------------------------------------------------

/// Download and install a catalog entry to the correct SD card directory.
///
/// # Safety
/// `entry` must point to a valid CatalogEntry. `progress_cb` may be NULL.
#[no_mangle]
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
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"Signature download failed: %d\0".as_ptr(), sig_dl);
        }

        let sig_ret = signing_verify_file(dest_cstr.as_ptr());
        if sig_ret == ESP_ERR_INVALID_CRC {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"Signature INVALID - deleting\0".as_ptr());
            let _ = std::fs::remove_file(&dest_path);
            let _ = std::fs::remove_file(format!("{}.sig", dest_path));
            return ESP_ERR_INVALID_CRC;
        } else if sig_ret == ESP_ERR_NOT_FOUND {
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"No signature file found\0".as_ptr());
        } else if sig_ret == ESP_OK {
            esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Signature verified OK\0".as_ptr());
        }
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
