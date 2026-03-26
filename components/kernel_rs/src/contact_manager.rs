// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Contact Manager
//
// Address book for messenger integration. Stores contacts with name, callsign,
// device ID, phone, BLE address, and Ed25519 public key. Supports vCard 3.0
// import/export. Persists to JSON on SD card. Integrates with messenger
// (device_id / phone lookups) and SOS beacon (emergency contacts).

use std::ffi::{c_char, CStr};
use std::fs;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes (matching esp_err.h)
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of contacts in the address book.
pub const MAX_CONTACTS: usize = 128;

/// Path to the contacts JSON file on the SD card.
pub const STORAGE_PATH: &str = "/sdcard/data/contacts.json";

// ---------------------------------------------------------------------------
// Base64 encode/decode (minimal, standard alphabet with padding)
// ---------------------------------------------------------------------------

const B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);

    let mut i = 0;
    while i < bytes.len() {
        let a = base64_decode_char(bytes[i])? as u32;
        let b = if i + 1 < bytes.len() {
            base64_decode_char(bytes[i + 1])? as u32
        } else {
            0
        };
        let c = if i + 2 < bytes.len() {
            base64_decode_char(bytes[i + 2])? as u32
        } else {
            0
        };
        let d = if i + 3 < bytes.len() {
            base64_decode_char(bytes[i + 3])? as u32
        } else {
            0
        };

        let triple = (a << 18) | (b << 12) | (c << 6) | d;

        out.push((triple >> 16) as u8);
        if i + 2 < bytes.len() {
            out.push((triple >> 8) as u8);
        }
        if i + 3 < bytes.len() {
            out.push(triple as u8);
        }

        i += 4;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Contact — internal Rust representation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Contact {
    id: u32,
    name: String,
    callsign: String,
    device_id: u32,
    phone: String,
    ble_addr: String,
    public_key: [u8; 32],
    notes: String,
    is_emergency: bool,
    created_at: u32,
    updated_at: u32,
}

impl Contact {
    fn new(id: u32, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            callsign: String::new(),
            device_id: 0,
            phone: String::new(),
            ble_addr: String::new(),
            public_key: [0u8; 32],
            notes: String::new(),
            is_emergency: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Convert to the C-compatible info struct.
    fn to_c_info(&self) -> CContactInfo {
        let mut info = CContactInfo {
            id: self.id,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: self.device_id,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: self.public_key,
            is_emergency: self.is_emergency,
        };
        copy_str_to_buf(&self.name, &mut info.name);
        copy_str_to_buf(&self.callsign, &mut info.callsign);
        copy_str_to_buf(&self.phone, &mut info.phone);
        copy_str_to_buf(&self.ble_addr, &mut info.ble_addr);
        info
    }

    /// Create from C-compatible info struct.
    fn from_c_info(info: &CContactInfo) -> Self {
        Self {
            id: info.id,
            name: buf_to_string(&info.name),
            callsign: buf_to_string(&info.callsign),
            device_id: info.device_id,
            phone: buf_to_string(&info.phone),
            ble_addr: buf_to_string(&info.ble_addr),
            public_key: info.public_key,
            notes: String::new(),
            is_emergency: info.is_emergency,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Serialize a single contact to JSON object string.
    fn to_json(&self) -> String {
        let key_b64 = base64_encode(&self.public_key);
        format!(
            "{{\"id\":{},\"name\":\"{}\",\"callsign\":\"{}\",\"device_id\":{},\
             \"phone\":\"{}\",\"ble_addr\":\"{}\",\"public_key\":\"{}\",\
             \"notes\":\"{}\",\"is_emergency\":{},\"created_at\":{},\"updated_at\":{}}}",
            self.id,
            json_escape(&self.name),
            json_escape(&self.callsign),
            self.device_id,
            json_escape(&self.phone),
            json_escape(&self.ble_addr),
            key_b64,
            json_escape(&self.notes),
            self.is_emergency,
            self.created_at,
            self.updated_at,
        )
    }

    /// Deserialize a single contact from a JSON object string.
    fn from_json(json: &str) -> Option<Self> {
        let id = json_get_int(json, "id")? as u32;
        let name = json_get_string(json, "name")?;
        if name.is_empty() {
            return None;
        }

        let callsign = json_get_string(json, "callsign").unwrap_or_default();
        let device_id = json_get_int(json, "device_id").unwrap_or(0) as u32;
        let phone = json_get_string(json, "phone").unwrap_or_default();
        let ble_addr = json_get_string(json, "ble_addr").unwrap_or_default();
        let notes = json_get_string(json, "notes").unwrap_or_default();
        let is_emergency = json_get_bool(json, "is_emergency").unwrap_or(false);
        let created_at = json_get_int(json, "created_at").unwrap_or(0) as u32;
        let updated_at = json_get_int(json, "updated_at").unwrap_or(0) as u32;

        let mut public_key = [0u8; 32];
        if let Some(key_str) = json_get_string(json, "public_key") {
            if !key_str.is_empty() {
                if let Some(decoded) = base64_decode(&key_str) {
                    if decoded.len() >= 32 {
                        public_key.copy_from_slice(&decoded[..32]);
                    }
                }
            }
        }

        Some(Self {
            id,
            name,
            callsign,
            device_id,
            phone,
            ble_addr,
            public_key,
            notes,
            is_emergency,
            created_at,
            updated_at,
        })
    }

    /// Export contact as vCard 3.0 string.
    fn to_vcard(&self) -> String {
        let mut vc = String::with_capacity(256);
        vc.push_str("BEGIN:VCARD\r\n");
        vc.push_str("VERSION:3.0\r\n");
        vc.push_str(&format!("FN:{}\r\n", self.name));
        if !self.callsign.is_empty() {
            vc.push_str(&format!("NICKNAME:{}\r\n", self.callsign));
        }
        if !self.phone.is_empty() {
            vc.push_str(&format!("TEL:{}\r\n", self.phone));
        }
        // Encode device_id, ble_addr, emergency in NOTE field
        vc.push_str(&format!(
            "NOTE:device_id={};ble_addr={};emergency={}\r\n",
            self.device_id,
            self.ble_addr,
            self.is_emergency,
        ));
        if self.public_key != [0u8; 32] {
            vc.push_str(&format!(
                "KEY;ENCODING=BASE64:{}\r\n",
                base64_encode(&self.public_key)
            ));
        }
        vc.push_str("END:VCARD\r\n");
        vc
    }

    /// Import contact from vCard 3.0 string. Returns None if FN is missing.
    fn from_vcard(data: &str) -> Option<Self> {
        let mut name = String::new();
        let mut callsign = String::new();
        let mut phone = String::new();
        let mut device_id: u32 = 0;
        let mut ble_addr = String::new();
        let mut is_emergency = false;
        let mut public_key = [0u8; 32];

        for line in data.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(val) = line.strip_prefix("FN:") {
                name = val.to_string();
            } else if let Some(val) = line.strip_prefix("NICKNAME:") {
                callsign = val.to_string();
            } else if let Some(val) = line.strip_prefix("TEL:") {
                phone = val.to_string();
            } else if let Some(val) = line.strip_prefix("NOTE:") {
                // Parse key=value pairs separated by semicolons
                for part in val.split(';') {
                    if let Some(v) = part.strip_prefix("device_id=") {
                        device_id = v.parse().unwrap_or(0);
                    } else if let Some(v) = part.strip_prefix("ble_addr=") {
                        ble_addr = v.to_string();
                    } else if let Some(v) = part.strip_prefix("emergency=") {
                        is_emergency = v == "true";
                    }
                }
            } else if let Some(val) = line.strip_prefix("KEY;ENCODING=BASE64:") {
                if let Some(decoded) = base64_decode(val.trim()) {
                    if decoded.len() >= 32 {
                        public_key.copy_from_slice(&decoded[..32]);
                    }
                }
            }
        }

        if name.is_empty() {
            return None;
        }

        Some(Self {
            id: 0, // Will be assigned by the manager
            name,
            callsign,
            device_id,
            phone,
            ble_addr,
            public_key,
            notes: String::new(),
            is_emergency,
            created_at: 0,
            updated_at: 0,
        })
    }
}

// ---------------------------------------------------------------------------
// CContactInfo — repr(C) struct for FFI
// ---------------------------------------------------------------------------

/// Fixed-size contact info for C interop. String fields are null-terminated
/// byte arrays; unused bytes are zero-filled.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CContactInfo {
    pub id: u32,
    pub name: [u8; 64],
    pub callsign: [u8; 16],
    pub device_id: u32,
    pub phone: [u8; 24],
    pub ble_addr: [u8; 24],
    pub public_key: [u8; 32],
    pub is_emergency: bool,
}

// ---------------------------------------------------------------------------
// String ↔ fixed-size buffer helpers
// ---------------------------------------------------------------------------

/// Copy a Rust string into a fixed-size byte buffer, null-terminated.
fn copy_str_to_buf(s: &str, buf: &mut [u8]) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&bytes[..len]);
    // Zero the remainder
    for b in &mut buf[len..] {
        *b = 0;
    }
}

/// Convert a null-terminated (or full-length) byte buffer to a String.
fn buf_to_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).to_string()
}

// ---------------------------------------------------------------------------
// Simple JSON helpers (no serde)
// ---------------------------------------------------------------------------

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn json_get_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    if !trimmed.starts_with('"') {
        return None;
    }

    let value_start = &trimmed[1..];
    // Handle escaped quotes
    let mut end = 0;
    let bytes = value_start.as_bytes();
    while end < bytes.len() {
        if bytes[end] == b'"' && (end == 0 || bytes[end - 1] != b'\\') {
            break;
        }
        end += 1;
    }
    if end >= bytes.len() {
        return None;
    }
    Some(value_start[..end].to_string())
}

fn json_get_int(json: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    let num_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(trimmed.len());
    trimmed[..num_end].parse().ok()
}

fn json_get_bool(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    if trimmed.starts_with("true") {
        Some(true)
    } else if trimmed.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Split a JSON array string into individual object strings.
/// Expects input like `[{...},{...}]`. Uses brace-depth counting to find
/// top-level object boundaries, which handles whitespace and newlines.
fn json_split_array(json: &str) -> Vec<String> {
    let trimmed = json.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Vec::new();
    }
    let inner = &trimmed[1..trimmed.len() - 1];

    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut obj_start: Option<usize> = None;

    for (i, ch) in inner.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '{' {
            if depth == 0 {
                obj_start = Some(i);
            }
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = obj_start {
                    result.push(inner[start..=i].to_string());
                    obj_start = None;
                }
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// ContactManager — internal state
// ---------------------------------------------------------------------------

struct ContactManager {
    contacts: Vec<Contact>,
    next_id: u32,
    initialized: bool,
    dirty: bool,
}

impl ContactManager {
    const fn new() -> Self {
        Self {
            contacts: Vec::new(),
            next_id: 1,
            initialized: false,
            dirty: false,
        }
    }

    fn init(&mut self) -> i32 {
        if self.initialized {
            return ESP_OK;
        }
        // Try to load from disk
        self.load_from_disk();
        self.initialized = true;
        ESP_OK
    }

    fn load_from_disk(&mut self) {
        let json = match fs::read_to_string(STORAGE_PATH) {
            Ok(s) => s,
            Err(_) => return,
        };
        let entries = json_split_array(&json);
        for entry in &entries {
            if let Some(contact) = Contact::from_json(entry) {
                if contact.id >= self.next_id {
                    self.next_id = contact.id + 1;
                }
                self.contacts.push(contact);
            }
        }
        self.dirty = false;
    }

    fn save_to_disk(&mut self) -> i32 {
        if !self.initialized {
            return ESP_ERR_INVALID_STATE;
        }
        let mut json = String::from("[\n");
        for (i, contact) in self.contacts.iter().enumerate() {
            if i > 0 {
                json.push_str(",\n");
            }
            json.push_str("  ");
            json.push_str(&contact.to_json());
        }
        json.push_str("\n]");

        // Ensure parent directory exists
        let _ = fs::create_dir_all("/sdcard/data");

        match fs::write(STORAGE_PATH, &json) {
            Ok(()) => {
                self.dirty = false;
                ESP_OK
            }
            Err(_) => ESP_ERR_INVALID_STATE,
        }
    }

    fn add(&mut self, name: &str, callsign: &str, device_id: u32, phone: &str) -> i32 {
        if name.is_empty() {
            return ESP_ERR_INVALID_ARG;
        }
        if self.contacts.len() >= MAX_CONTACTS {
            return ESP_ERR_NO_MEM;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut contact = Contact::new(id, name);
        contact.callsign = callsign.to_string();
        contact.device_id = device_id;
        contact.phone = phone.to_string();
        self.contacts.push(contact);
        self.dirty = true;
        id as i32
    }

    fn remove(&mut self, id: u32) -> i32 {
        let before = self.contacts.len();
        self.contacts.retain(|c| c.id != id);
        if self.contacts.len() == before {
            return ESP_ERR_NOT_FOUND;
        }
        self.dirty = true;
        ESP_OK
    }

    fn update(&mut self, info: &CContactInfo) -> i32 {
        for contact in &mut self.contacts {
            if contact.id == info.id {
                contact.name = buf_to_string(&info.name);
                contact.callsign = buf_to_string(&info.callsign);
                contact.device_id = info.device_id;
                contact.phone = buf_to_string(&info.phone);
                contact.ble_addr = buf_to_string(&info.ble_addr);
                contact.public_key = info.public_key;
                contact.is_emergency = info.is_emergency;
                self.dirty = true;
                return ESP_OK;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn get(&self, id: u32) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    fn get_at(&self, index: usize) -> Option<&Contact> {
        self.contacts.get(index)
    }

    fn count(&self) -> usize {
        self.contacts.len()
    }

    fn find_by_device_id(&self, device_id: u32) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.device_id == device_id && c.device_id != 0)
    }

    fn find_by_phone(&self, phone: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.phone == phone && !c.phone.is_empty())
    }

    fn search(&self, query: &str) -> Vec<&Contact> {
        let q = query.to_lowercase();
        self.contacts
            .iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&q)
                    || c.callsign.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn get_emergency(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| c.is_emergency).collect()
    }

    fn set_pubkey(&mut self, id: u32, key: &[u8; 32]) -> i32 {
        for contact in &mut self.contacts {
            if contact.id == id {
                contact.public_key = *key;
                self.dirty = true;
                return ESP_OK;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn export_vcard(&self, id: u32) -> Option<String> {
        self.get(id).map(|c| c.to_vcard())
    }

    fn import_vcard(&mut self, data: &str) -> i32 {
        if self.contacts.len() >= MAX_CONTACTS {
            return ESP_ERR_NO_MEM;
        }
        match Contact::from_vcard(data) {
            Some(mut contact) => {
                let id = self.next_id;
                self.next_id += 1;
                contact.id = id;
                self.contacts.push(contact);
                self.dirty = true;
                id as i32
            }
            None => ESP_ERR_INVALID_ARG,
        }
    }

    #[cfg(test)]
    fn reset(&mut self) {
        self.contacts.clear();
        self.next_id = 1;
        self.initialized = false;
        self.dirty = false;
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static CONTACTS: Mutex<ContactManager> = Mutex::new(ContactManager::new());

// ---------------------------------------------------------------------------
// Public Rust API
// ---------------------------------------------------------------------------

pub fn contact_manager_init() -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.init(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

pub fn contact_add(name: &str, callsign: &str, device_id: u32, phone: &str) -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.add(name, callsign, device_id, phone),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

pub fn contact_remove(id: u32) -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.remove(id),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

pub fn contact_count() -> i32 {
    match CONTACTS.lock() {
        Ok(mgr) => mgr.count() as i32,
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

pub fn contact_save() -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.save_to_disk(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the contact manager. Loads contacts from SD card if available.
/// Idempotent; safe to call multiple times.
#[no_mangle]
pub extern "C" fn rs_contact_manager_init() -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.init(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Add a new contact. Returns the new contact's ID (>0) on success, or a
/// negative ESP error code on failure.
///
/// # Safety
///
/// `name` and `phone` must be valid null-terminated C strings or null.
/// `callsign` may be null (treated as empty).
#[no_mangle]
pub unsafe extern "C" fn rs_contact_add(
    name: *const c_char,
    callsign: *const c_char,
    device_id: u32,
    phone: *const c_char,
) -> i32 {
    if name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    // SAFETY: caller guarantees `name` is a valid null-terminated string.
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let callsign_str = if callsign.is_null() {
        ""
    } else {
        // SAFETY: caller guarantees `callsign` is valid if non-null.
        match CStr::from_ptr(callsign).to_str() {
            Ok(s) => s,
            Err(_) => "",
        }
    };
    let phone_str = if phone.is_null() {
        ""
    } else {
        // SAFETY: caller guarantees `phone` is valid if non-null.
        match CStr::from_ptr(phone).to_str() {
            Ok(s) => s,
            Err(_) => "",
        }
    };
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.add(name_str, callsign_str, device_id, phone_str),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Remove a contact by ID.
#[no_mangle]
pub extern "C" fn rs_contact_remove(id: u32) -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.remove(id),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Update a contact from a CContactInfo struct.
///
/// # Safety
///
/// `info` must point to a valid, fully initialised CContactInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_update(info: *const CContactInfo) -> i32 {
    if info.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    // SAFETY: caller guarantees the pointer is valid and aligned.
    let info_ref = &*info;
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.update(info_ref),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get a contact by ID, writing it to the provided output struct.
///
/// # Safety
///
/// `out` must point to a valid, writable CContactInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_get(id: u32, out: *mut CContactInfo) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match CONTACTS.lock() {
        Ok(mgr) => match mgr.get(id) {
            Some(contact) => {
                // SAFETY: caller guarantees `out` is valid and writable.
                *out = contact.to_c_info();
                ESP_OK
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Return the number of contacts, or a negative error code.
#[no_mangle]
pub extern "C" fn rs_contact_count() -> i32 {
    match CONTACTS.lock() {
        Ok(mgr) => mgr.count() as i32,
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get a contact by zero-based index (for iteration).
///
/// # Safety
///
/// `out` must point to a valid, writable CContactInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_get_at(index: u32, out: *mut CContactInfo) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match CONTACTS.lock() {
        Ok(mgr) => match mgr.get_at(index as usize) {
            Some(contact) => {
                // SAFETY: caller guarantees `out` is valid and writable.
                *out = contact.to_c_info();
                ESP_OK
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Find a contact by LoRa device ID.
///
/// # Safety
///
/// `out` must point to a valid, writable CContactInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_find_by_device_id(
    device_id: u32,
    out: *mut CContactInfo,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match CONTACTS.lock() {
        Ok(mgr) => match mgr.find_by_device_id(device_id) {
            Some(contact) => {
                // SAFETY: caller guarantees `out` is valid and writable.
                *out = contact.to_c_info();
                ESP_OK
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Find a contact by phone number.
///
/// # Safety
///
/// `phone` must be a valid null-terminated C string.
/// `out` must point to a valid, writable CContactInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_find_by_phone(
    phone: *const c_char,
    out: *mut CContactInfo,
) -> i32 {
    if phone.is_null() || out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    // SAFETY: caller guarantees `phone` is a valid null-terminated string.
    let phone_str = match CStr::from_ptr(phone).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    match CONTACTS.lock() {
        Ok(mgr) => match mgr.find_by_phone(phone_str) {
            Some(contact) => {
                // SAFETY: caller guarantees `out` is valid and writable.
                *out = contact.to_c_info();
                ESP_OK
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Search contacts by name/callsign substring. Writes up to `max` results
/// into the `results` array. Returns the number of matches written, or a
/// negative error code.
///
/// # Safety
///
/// `query` must be a valid null-terminated C string.
/// `results` must point to an array of at least `max` CContactInfo structs.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_search(
    query: *const c_char,
    results: *mut CContactInfo,
    max: u32,
) -> i32 {
    if query.is_null() || results.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if max == 0 {
        return 0;
    }
    // SAFETY: caller guarantees `query` is a valid null-terminated string.
    let query_str = match CStr::from_ptr(query).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    match CONTACTS.lock() {
        Ok(mgr) => {
            let matches = mgr.search(query_str);
            let count = matches.len().min(max as usize);
            for i in 0..count {
                // SAFETY: caller guarantees `results` has at least `max` elements.
                *results.add(i) = matches[i].to_c_info();
            }
            count as i32
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get emergency contacts. Writes up to `max` results into the `results`
/// array. Returns the number of emergency contacts written, or a negative
/// error code.
///
/// # Safety
///
/// `results` must point to an array of at least `max` CContactInfo structs.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_get_emergency(
    results: *mut CContactInfo,
    max: u32,
) -> i32 {
    if results.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if max == 0 {
        return 0;
    }
    match CONTACTS.lock() {
        Ok(mgr) => {
            let emerg = mgr.get_emergency();
            let count = emerg.len().min(max as usize);
            for i in 0..count {
                // SAFETY: caller guarantees `results` has at least `max` elements.
                *results.add(i) = emerg[i].to_c_info();
            }
            count as i32
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Set the Ed25519 public key for a contact.
///
/// # Safety
///
/// `key` must point to exactly 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_set_pubkey(id: u32, key: *const u8) -> i32 {
    if key.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    // SAFETY: caller guarantees `key` points to at least 32 bytes.
    let mut key_buf = [0u8; 32];
    std::ptr::copy_nonoverlapping(key, key_buf.as_mut_ptr(), 32);
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.set_pubkey(id, &key_buf),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Persist contacts to SD card JSON.
#[no_mangle]
pub extern "C" fn rs_contact_save() -> i32 {
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.save_to_disk(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Export a contact as vCard 3.0 into the provided buffer.
/// Returns the number of bytes written on success, or a negative error code.
///
/// # Safety
///
/// `buf` must point to a writable buffer of at least `buf_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_export_vcard(
    id: u32,
    buf: *mut u8,
    buf_len: usize,
) -> i32 {
    if buf.is_null() || buf_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    match CONTACTS.lock() {
        Ok(mgr) => match mgr.export_vcard(id) {
            Some(vcard) => {
                let bytes = vcard.as_bytes();
                if bytes.len() > buf_len {
                    return ESP_ERR_NO_MEM;
                }
                // SAFETY: caller guarantees `buf` is writable for at least `buf_len`.
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                bytes.len() as i32
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Import a vCard 3.0 string and create a new contact. Returns the new
/// contact ID on success, or a negative error code.
///
/// # Safety
///
/// `data` must point to `data_len` bytes of valid UTF-8 vCard text.
#[no_mangle]
pub unsafe extern "C" fn rs_contact_import_vcard(
    data: *const u8,
    data_len: usize,
) -> i32 {
    if data.is_null() || data_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    // SAFETY: caller guarantees `data` points to `data_len` bytes.
    let slice = std::slice::from_raw_parts(data, data_len);
    let text = match std::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    match CONTACTS.lock() {
        Ok(mut mgr) => mgr.import_vcard(text),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Reset global state before each test.
    fn reset() {
        let mut mgr = CONTACTS.lock().unwrap();
        mgr.reset();
    }

    // -----------------------------------------------------------------------
    // Init tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_returns_ok() {
        reset();
        let rc = rs_contact_manager_init();
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_init_idempotent() {
        reset();
        let rc1 = rs_contact_manager_init();
        let rc2 = rs_contact_manager_init();
        assert_eq!(rc1, ESP_OK);
        assert_eq!(rc2, ESP_OK);
    }

    // -----------------------------------------------------------------------
    // Add tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_single_contact() {
        reset();
        let name = CString::new("Ewan").unwrap();
        let callsign = CString::new("CAIRN-1").unwrap();
        let phone = CString::new("+447700900123").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), callsign.as_ptr(), 42567, phone.as_ptr())
        };
        assert!(id > 0, "add must return positive ID, got {}", id);
    }

    #[test]
    fn test_add_returns_incrementing_ids() {
        reset();
        let n1 = CString::new("Alice").unwrap();
        let n2 = CString::new("Bob").unwrap();
        let id1 = unsafe {
            rs_contact_add(n1.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        let id2 = unsafe {
            rs_contact_add(n2.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert!(id1 > 0);
        assert!(id2 > 0);
        assert!(id2 > id1, "IDs must increment");
    }

    #[test]
    fn test_add_multiple_contacts() {
        reset();
        for i in 0..10 {
            let name = CString::new(format!("Contact{}", i)).unwrap();
            let id = unsafe {
                rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
            };
            assert!(id > 0, "add #{} failed", i);
        }
        assert_eq!(rs_contact_count(), 10);
    }

    #[test]
    fn test_add_at_capacity_fails() {
        reset();
        for i in 0..MAX_CONTACTS {
            let name = CString::new(format!("C{}", i)).unwrap();
            let id = unsafe {
                rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
            };
            assert!(id > 0, "add #{} failed unexpectedly", i);
        }
        // 129th contact must fail
        let name = CString::new("Overflow").unwrap();
        let rc = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_NO_MEM, "add beyond capacity must return NO_MEM");
    }

    #[test]
    fn test_add_empty_name_rejected() {
        reset();
        let name = CString::new("").unwrap();
        let rc = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "empty name must be rejected");
    }

    #[test]
    fn test_add_null_name_rejected() {
        reset();
        let rc = unsafe {
            rs_contact_add(std::ptr::null(), std::ptr::null(), 0, std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null name must be rejected");
    }

    // -----------------------------------------------------------------------
    // Remove tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_remove_existing() {
        reset();
        let name = CString::new("Remove Me").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert!(id > 0);
        let rc = rs_contact_remove(id as u32);
        assert_eq!(rc, ESP_OK);
        assert_eq!(rs_contact_count(), 0);
    }

    #[test]
    fn test_remove_nonexistent() {
        reset();
        let rc = rs_contact_remove(999);
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // Get tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_by_id() {
        reset();
        let name = CString::new("Thorn").unwrap();
        let callsign = CString::new("THN").unwrap();
        let phone = CString::new("+1555").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), callsign.as_ptr(), 100, phone.as_ptr())
        };
        assert!(id > 0);

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(id as u32, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.id, id as u32);
        assert_eq!(buf_to_string(&out.name), "Thorn");
        assert_eq!(buf_to_string(&out.callsign), "THN");
        assert_eq!(out.device_id, 100);
        assert_eq!(buf_to_string(&out.phone), "+1555");
    }

    #[test]
    fn test_get_nonexistent() {
        reset();
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(999, &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_get_null_out() {
        reset();
        let rc = unsafe { rs_contact_get(1, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_get_at_index() {
        reset();
        let n1 = CString::new("Alpha").unwrap();
        let n2 = CString::new("Bravo").unwrap();
        unsafe {
            rs_contact_add(n1.as_ptr(), std::ptr::null(), 0, std::ptr::null());
            rs_contact_add(n2.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get_at(1, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "Bravo");
    }

    #[test]
    fn test_get_at_out_of_range() {
        reset();
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get_at(0, &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND, "get_at on empty list must return NOT_FOUND");
    }

    // -----------------------------------------------------------------------
    // Update tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_update_existing() {
        reset();
        let name = CString::new("Old Name").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert!(id > 0);

        let mut info = CContactInfo {
            id: id as u32,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 777,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: true,
        };
        copy_str_to_buf("New Name", &mut info.name);
        copy_str_to_buf("NN-1", &mut info.callsign);

        let rc = unsafe { rs_contact_update(&info) };
        assert_eq!(rc, ESP_OK);

        // Verify updated values
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(id as u32, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "New Name");
        assert_eq!(buf_to_string(&out.callsign), "NN-1");
        assert_eq!(out.device_id, 777);
        assert!(out.is_emergency);
    }

    #[test]
    fn test_update_nonexistent() {
        reset();
        let mut info = CContactInfo {
            id: 999,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        copy_str_to_buf("Ghost", &mut info.name);
        let rc = unsafe { rs_contact_update(&info) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_update_null_ptr() {
        reset();
        let rc = unsafe { rs_contact_update(std::ptr::null()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Search tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_by_device_id() {
        reset();
        let name = CString::new("DeviceUser").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 42567, std::ptr::null())
        };
        assert!(id > 0);

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_find_by_device_id(42567, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "DeviceUser");
    }

    #[test]
    fn test_find_by_device_id_zero_not_matched() {
        reset();
        let name = CString::new("NoDevice").unwrap();
        unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_find_by_device_id(0, &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND, "device_id 0 must not match");
    }

    #[test]
    fn test_find_by_phone() {
        reset();
        let name = CString::new("PhoneUser").unwrap();
        let phone = CString::new("+447700900123").unwrap();
        unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, phone.as_ptr());
        }

        let query = CString::new("+447700900123").unwrap();
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_find_by_phone(query.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "PhoneUser");
    }

    #[test]
    fn test_find_by_phone_not_found() {
        reset();
        let query = CString::new("+0000000").unwrap();
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_find_by_phone(query.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_search_by_name_substring() {
        reset();
        let n1 = CString::new("Ewan MacLeod").unwrap();
        let n2 = CString::new("Fiona MacLeod").unwrap();
        let n3 = CString::new("James Smith").unwrap();
        unsafe {
            rs_contact_add(n1.as_ptr(), std::ptr::null(), 0, std::ptr::null());
            rs_contact_add(n2.as_ptr(), std::ptr::null(), 0, std::ptr::null());
            rs_contact_add(n3.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let query = CString::new("MacLeod").unwrap();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe {
            rs_contact_search(query.as_ptr(), results.as_mut_ptr(), 10)
        };
        assert_eq!(count, 2, "should match two MacLeods");
    }

    #[test]
    fn test_search_case_insensitive() {
        reset();
        let n1 = CString::new("Ewan MacLeod").unwrap();
        unsafe {
            rs_contact_add(n1.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let query = CString::new("ewan").unwrap();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe {
            rs_contact_search(query.as_ptr(), results.as_mut_ptr(), 10)
        };
        assert_eq!(count, 1, "search should be case-insensitive");
    }

    #[test]
    fn test_search_by_callsign() {
        reset();
        let name = CString::new("Radio Op").unwrap();
        let callsign = CString::new("CAIRN-1").unwrap();
        unsafe {
            rs_contact_add(name.as_ptr(), callsign.as_ptr(), 0, std::ptr::null());
        }

        let query = CString::new("cairn").unwrap();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe {
            rs_contact_search(query.as_ptr(), results.as_mut_ptr(), 10)
        };
        assert_eq!(count, 1, "should match by callsign");
    }

    #[test]
    fn test_search_no_results() {
        reset();
        let name = CString::new("Alice").unwrap();
        unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let query = CString::new("ZZZZZ").unwrap();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe {
            rs_contact_search(query.as_ptr(), results.as_mut_ptr(), 10)
        };
        assert_eq!(count, 0);
    }

    #[test]
    fn test_search_null_query() {
        reset();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 1];
        let rc = unsafe {
            rs_contact_search(std::ptr::null(), results.as_mut_ptr(), 1)
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Emergency contact tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_emergency_contacts() {
        reset();
        // Add two normal + one emergency
        let n1 = CString::new("Normal1").unwrap();
        let n2 = CString::new("Normal2").unwrap();
        let n3 = CString::new("Emergency1").unwrap();
        let id1 = unsafe {
            rs_contact_add(n1.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        let _id2 = unsafe {
            rs_contact_add(n2.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        let id3 = unsafe {
            rs_contact_add(n3.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };

        // Mark id1 and id3 as emergency via update
        let mut info = CContactInfo {
            id: id3 as u32,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: true,
        };
        copy_str_to_buf("Emergency1", &mut info.name);
        unsafe { rs_contact_update(&info); }

        info.id = id1 as u32;
        copy_str_to_buf("Normal1", &mut info.name);
        info.is_emergency = true;
        unsafe { rs_contact_update(&info); }

        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe { rs_contact_get_emergency(results.as_mut_ptr(), 10) };
        assert_eq!(count, 2, "should have 2 emergency contacts");
    }

    #[test]
    fn test_no_emergency_contacts() {
        reset();
        let name = CString::new("Normal").unwrap();
        unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null());
        }

        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 10];
        let count = unsafe { rs_contact_get_emergency(results.as_mut_ptr(), 10) };
        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // Public key tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_and_get_pubkey() {
        reset();
        let name = CString::new("KeyUser").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };
        assert!(id > 0);

        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = (i + 1) as u8;
        }
        let rc = unsafe { rs_contact_set_pubkey(id as u32, key.as_ptr()) };
        assert_eq!(rc, ESP_OK);

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(id as u32, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.public_key, key, "public key must round-trip");
    }

    #[test]
    fn test_set_pubkey_nonexistent() {
        reset();
        let key = [0xABu8; 32];
        let rc = unsafe { rs_contact_set_pubkey(999, key.as_ptr()) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_set_pubkey_null_key() {
        reset();
        let rc = unsafe { rs_contact_set_pubkey(1, std::ptr::null()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // JSON serialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_contact_to_json() {
        let c = Contact {
            id: 1,
            name: "Ewan".to_string(),
            callsign: "CAIRN-1".to_string(),
            device_id: 42567,
            phone: "+447700900123".to_string(),
            ble_addr: String::new(),
            public_key: [0u8; 32],
            notes: "Team Lead".to_string(),
            is_emergency: true,
            created_at: 1711497600,
            updated_at: 1711497600,
        };
        let json = c.to_json();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"name\":\"Ewan\""));
        assert!(json.contains("\"callsign\":\"CAIRN-1\""));
        assert!(json.contains("\"device_id\":42567"));
        assert!(json.contains("\"phone\":\"+447700900123\""));
        assert!(json.contains("\"is_emergency\":true"));
        assert!(json.contains("\"notes\":\"Team Lead\""));
    }

    #[test]
    fn test_contact_from_json() {
        let json = r#"{"id":5,"name":"Fiona","callsign":"FI-2","device_id":100,"phone":"+1234","ble_addr":"AA:BB","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","notes":"Test","is_emergency":false,"created_at":100,"updated_at":200}"#;
        let c = Contact::from_json(json).expect("should parse");
        assert_eq!(c.id, 5);
        assert_eq!(c.name, "Fiona");
        assert_eq!(c.callsign, "FI-2");
        assert_eq!(c.device_id, 100);
        assert_eq!(c.phone, "+1234");
        assert_eq!(c.ble_addr, "AA:BB");
        assert!(!c.is_emergency);
        assert_eq!(c.created_at, 100);
        assert_eq!(c.updated_at, 200);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = (i * 7 + 3) as u8;
        }
        let original = Contact {
            id: 42,
            name: "Roundtrip".to_string(),
            callsign: "RT-1".to_string(),
            device_id: 9999,
            phone: "+44123".to_string(),
            ble_addr: "DE:AD".to_string(),
            public_key: key,
            notes: "Some notes".to_string(),
            is_emergency: true,
            created_at: 500,
            updated_at: 600,
        };
        let json = original.to_json();
        let parsed = Contact::from_json(&json).expect("round-trip parse must succeed");
        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.callsign, original.callsign);
        assert_eq!(parsed.device_id, original.device_id);
        assert_eq!(parsed.phone, original.phone);
        assert_eq!(parsed.ble_addr, original.ble_addr);
        assert_eq!(parsed.public_key, original.public_key);
        assert_eq!(parsed.notes, original.notes);
        assert_eq!(parsed.is_emergency, original.is_emergency);
        assert_eq!(parsed.created_at, original.created_at);
        assert_eq!(parsed.updated_at, original.updated_at);
    }

    #[test]
    fn test_json_malformed_handled() {
        assert!(Contact::from_json("not json at all").is_none());
        assert!(Contact::from_json("{}").is_none());
        assert!(Contact::from_json("{\"id\":1}").is_none()); // no name
        assert!(Contact::from_json("{\"id\":1,\"name\":\"\"}").is_none()); // empty name
    }

    #[test]
    fn test_json_split_array() {
        let arr = r#"[{"id":1,"name":"A"},{"id":2,"name":"B"}]"#;
        let parts = json_split_array(arr);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("\"id\":1"));
        assert!(parts[1].contains("\"id\":2"));
    }

    #[test]
    fn test_json_split_empty_array() {
        let parts = json_split_array("[]");
        assert_eq!(parts.len(), 0);
    }

    #[test]
    fn test_json_escape_special_chars() {
        let escaped = json_escape("He said \"hello\"\nNew line\\slash");
        assert_eq!(escaped, "He said \\\"hello\\\"\\nNew line\\\\slash");
    }

    // -----------------------------------------------------------------------
    // vCard tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_export_vcard() {
        reset();
        let name = CString::new("Ewan MacLeod").unwrap();
        let callsign = CString::new("CAIRN-1").unwrap();
        let phone = CString::new("+447700900123").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), callsign.as_ptr(), 42567, phone.as_ptr())
        };
        assert!(id > 0);

        let mut buf = [0u8; 512];
        let len = unsafe {
            rs_contact_export_vcard(id as u32, buf.as_mut_ptr(), buf.len())
        };
        assert!(len > 0, "export must return positive length");
        let vcard = std::str::from_utf8(&buf[..len as usize]).unwrap();
        assert!(vcard.contains("BEGIN:VCARD"));
        assert!(vcard.contains("VERSION:3.0"));
        assert!(vcard.contains("FN:Ewan MacLeod"));
        assert!(vcard.contains("NICKNAME:CAIRN-1"));
        assert!(vcard.contains("TEL:+447700900123"));
        assert!(vcard.contains("device_id=42567"));
        assert!(vcard.contains("END:VCARD"));
    }

    #[test]
    fn test_export_vcard_nonexistent() {
        reset();
        let mut buf = [0u8; 512];
        let rc = unsafe {
            rs_contact_export_vcard(999, buf.as_mut_ptr(), buf.len())
        };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_export_vcard_buffer_too_small() {
        reset();
        let name = CString::new("Ewan MacLeod").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), std::ptr::null(), 0, std::ptr::null())
        };

        let mut buf = [0u8; 2]; // too small
        let rc = unsafe {
            rs_contact_export_vcard(id as u32, buf.as_mut_ptr(), buf.len())
        };
        assert_eq!(rc, ESP_ERR_NO_MEM, "too-small buffer must return NO_MEM");
    }

    #[test]
    fn test_export_vcard_null_buf() {
        reset();
        let rc = unsafe {
            rs_contact_export_vcard(1, std::ptr::null_mut(), 100)
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_export_vcard_zero_len() {
        reset();
        let mut buf = [0u8; 1];
        let rc = unsafe {
            rs_contact_export_vcard(1, buf.as_mut_ptr(), 0)
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_import_vcard() {
        reset();
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Fiona Ross\r\nNICKNAME:FR-1\r\nTEL:+441234\r\nNOTE:device_id=555;ble_addr=AA:BB;emergency=true\r\nEND:VCARD\r\n";
        let data = vcard.as_bytes();
        let id = unsafe {
            rs_contact_import_vcard(data.as_ptr(), data.len())
        };
        assert!(id > 0, "import must return positive ID");

        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(id as u32, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "Fiona Ross");
        assert_eq!(buf_to_string(&out.callsign), "FR-1");
        assert_eq!(out.device_id, 555);
        assert_eq!(buf_to_string(&out.ble_addr), "AA:BB");
        assert!(out.is_emergency);
        assert_eq!(buf_to_string(&out.phone), "+441234");
    }

    #[test]
    fn test_import_vcard_no_fn() {
        reset();
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nTEL:+1234\r\nEND:VCARD\r\n";
        let data = vcard.as_bytes();
        let rc = unsafe {
            rs_contact_import_vcard(data.as_ptr(), data.len())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "vCard without FN must be rejected");
    }

    #[test]
    fn test_import_vcard_null_data() {
        reset();
        let rc = unsafe { rs_contact_import_vcard(std::ptr::null(), 10) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_import_vcard_zero_len() {
        reset();
        let data = b"x";
        let rc = unsafe { rs_contact_import_vcard(data.as_ptr(), 0) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_vcard_roundtrip() {
        reset();
        // Create a contact with a public key
        let name = CString::new("Roundtrip User").unwrap();
        let callsign = CString::new("RTU").unwrap();
        let phone = CString::new("+44999").unwrap();
        let id = unsafe {
            rs_contact_add(name.as_ptr(), callsign.as_ptr(), 12345, phone.as_ptr())
        };
        assert!(id > 0);

        // Set a public key
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = (i * 3 + 5) as u8;
        }
        unsafe { rs_contact_set_pubkey(id as u32, key.as_ptr()); }

        // Export to vCard
        let mut buf = [0u8; 1024];
        let len = unsafe {
            rs_contact_export_vcard(id as u32, buf.as_mut_ptr(), buf.len())
        };
        assert!(len > 0);

        // Import the vCard back
        let id2 = unsafe {
            rs_contact_import_vcard(buf.as_ptr(), len as usize)
        };
        assert!(id2 > 0);

        // Compare
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_get(id2 as u32, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(buf_to_string(&out.name), "Roundtrip User");
        assert_eq!(buf_to_string(&out.callsign), "RTU");
        assert_eq!(out.device_id, 12345);
        assert_eq!(buf_to_string(&out.phone), "+44999");
        assert_eq!(out.public_key, key, "public key must survive vCard round-trip");
    }

    // -----------------------------------------------------------------------
    // Base64 tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_base64_roundtrip() {
        let data: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
        ];
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).expect("decode must succeed");
        assert_eq!(decoded.len(), 32);
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn test_base64_encode_known() {
        // "Hello" -> "SGVsbG8="
        let encoded = base64_encode(b"Hello");
        assert_eq!(encoded, "SGVsbG8=");
    }

    #[test]
    fn test_base64_decode_known() {
        let decoded = base64_decode("SGVsbG8=").expect("decode must succeed");
        assert_eq!(&decoded[..], b"Hello");
    }

    #[test]
    fn test_base64_all_zeros() {
        let zeros = [0u8; 32];
        let encoded = base64_encode(&zeros);
        let decoded = base64_decode(&encoded).expect("decode must succeed");
        assert_eq!(decoded.len(), 32);
        assert_eq!(&decoded[..], &zeros[..]);
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_count_empty() {
        reset();
        assert_eq!(rs_contact_count(), 0);
    }

    #[test]
    fn test_find_by_phone_null_args() {
        reset();
        let mut out = CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        };
        let rc = unsafe { rs_contact_find_by_phone(std::ptr::null(), &mut out) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);

        let phone = CString::new("+1").unwrap();
        let rc = unsafe { rs_contact_find_by_phone(phone.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_find_by_device_id_null_out() {
        reset();
        let rc = unsafe { rs_contact_find_by_device_id(1, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_get_at_null_out() {
        reset();
        let rc = unsafe { rs_contact_get_at(0, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_emergency_null_results() {
        reset();
        let rc = unsafe { rs_contact_get_emergency(std::ptr::null_mut(), 10) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_emergency_zero_max() {
        reset();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 1];
        let count = unsafe { rs_contact_get_emergency(results.as_mut_ptr(), 0) };
        assert_eq!(count, 0);
    }

    #[test]
    fn test_search_zero_max() {
        reset();
        let query = CString::new("x").unwrap();
        let mut results = [CContactInfo {
            id: 0,
            name: [0u8; 64],
            callsign: [0u8; 16],
            device_id: 0,
            phone: [0u8; 24],
            ble_addr: [0u8; 24],
            public_key: [0u8; 32],
            is_emergency: false,
        }; 1];
        let count = unsafe {
            rs_contact_search(query.as_ptr(), results.as_mut_ptr(), 0)
        };
        assert_eq!(count, 0);
    }

    #[test]
    fn test_copy_str_to_buf_truncation() {
        let mut buf = [0u8; 4];
        copy_str_to_buf("Hello World", &mut buf);
        // Should copy "Hel" + null
        assert_eq!(&buf, &[b'H', b'e', b'l', 0]);
    }

    #[test]
    fn test_buf_to_string_full_buffer() {
        let buf = [b'A', b'B', b'C', b'D']; // no null terminator
        assert_eq!(buf_to_string(&buf), "ABCD");
    }

    #[test]
    fn test_buf_to_string_with_null() {
        let buf = [b'H', b'i', 0, b'X'];
        assert_eq!(buf_to_string(&buf), "Hi");
    }

    #[test]
    fn test_c_contact_info_roundtrip() {
        let mut key = [0u8; 32];
        key[0] = 0xDE;
        key[31] = 0xAD;
        let contact = Contact {
            id: 7,
            name: "Test".to_string(),
            callsign: "TST".to_string(),
            device_id: 42,
            phone: "+1".to_string(),
            ble_addr: "AB:CD".to_string(),
            public_key: key,
            notes: "Ignored in CContactInfo".to_string(),
            is_emergency: true,
            created_at: 100,
            updated_at: 200,
        };

        let c_info = contact.to_c_info();
        assert_eq!(c_info.id, 7);
        assert_eq!(buf_to_string(&c_info.name), "Test");
        assert_eq!(c_info.public_key[0], 0xDE);
        assert_eq!(c_info.public_key[31], 0xAD);
        assert!(c_info.is_emergency);

        let back = Contact::from_c_info(&c_info);
        assert_eq!(back.id, 7);
        assert_eq!(back.name, "Test");
        assert_eq!(back.callsign, "TST");
        assert_eq!(back.device_id, 42);
        assert_eq!(back.public_key, key);
    }

    // -----------------------------------------------------------------------
    // Persistence test (uses temp dir)
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_and_load_roundtrip() {
        reset();
        // Use a temp directory for this test
        let tmp_dir = std::env::temp_dir().join("thistle_contacts_test");
        let _ = fs::create_dir_all(&tmp_dir);
        let tmp_path = tmp_dir.join("contacts.json");
        let tmp_path_str = tmp_path.to_str().unwrap().to_string();

        // Manually add contacts and serialise
        {
            let mut mgr = CONTACTS.lock().unwrap();
            mgr.initialized = true;
            mgr.add("SaveTest1", "ST-1", 111, "+111");
            mgr.add("SaveTest2", "ST-2", 222, "+222");

            // Serialise manually to our temp path
            let mut json = String::from("[\n");
            for (i, c) in mgr.contacts.iter().enumerate() {
                if i > 0 {
                    json.push_str(",\n");
                }
                json.push_str("  ");
                json.push_str(&c.to_json());
            }
            json.push_str("\n]");
            fs::write(&tmp_path_str, &json).unwrap();
        }

        // Read back and parse
        let json = fs::read_to_string(&tmp_path_str).unwrap();
        let entries = json_split_array(&json);
        assert_eq!(entries.len(), 2);

        let c1 = Contact::from_json(&entries[0]).unwrap();
        assert_eq!(c1.name, "SaveTest1");
        assert_eq!(c1.callsign, "ST-1");
        assert_eq!(c1.device_id, 111);

        let c2 = Contact::from_json(&entries[1]).unwrap();
        assert_eq!(c2.name, "SaveTest2");

        // Cleanup
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_dir(&tmp_dir);
    }
}
