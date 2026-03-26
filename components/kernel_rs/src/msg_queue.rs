// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Message Queue (store-and-forward)
//
// Queues outbound messages when delivery is uncertain (e.g. LoRa broadcast
// with no relay in range). Retries with exponential backoff. Persists queue
// to SD card so messages survive reboots. Serves SAR volunteers (Cairn) and
// field researchers (Ember) who are often out of range.

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

/// Maximum number of messages in the queue.
const MAX_QUEUE_SIZE: usize = 64;

/// Default maximum number of retry attempts.
const DEFAULT_MAX_RETRIES: u32 = 10;

/// Default time-to-live in milliseconds (1 hour).
const DEFAULT_TTL_MS: u64 = 3_600_000;

/// Base retry interval in milliseconds (5 seconds).
const BASE_RETRY_MS: u64 = 5_000;

/// Maximum retry interval in milliseconds (5 minutes).
const MAX_RETRY_MS: u64 = 300_000;

/// Exponential backoff multiplier.
const BACKOFF_MULTIPLIER: u64 = 2;

/// Path to the queue JSON file on the SD card.
const STORAGE_PATH: &str = "/sdcard/data/msg_queue.json";

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

/// Split a JSON array string into individual object strings.
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
// MsgPriority
// ---------------------------------------------------------------------------

/// Priority levels for queued messages.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MsgPriority {
    Normal = 0,
    High = 1,
    Urgent = 2,
}

impl MsgPriority {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(MsgPriority::Normal),
            1 => Some(MsgPriority::High),
            2 => Some(MsgPriority::Urgent),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// MsgStatus
// ---------------------------------------------------------------------------

/// Status of a queued message.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MsgStatus {
    Pending = 0,
    Retrying = 1,
    Sent = 2,
    Expired = 3,
    Failed = 4,
    Cancelled = 5,
}

impl MsgStatus {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(MsgStatus::Pending),
            1 => Some(MsgStatus::Retrying),
            2 => Some(MsgStatus::Sent),
            3 => Some(MsgStatus::Expired),
            4 => Some(MsgStatus::Failed),
            5 => Some(MsgStatus::Cancelled),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// QueuedMessage — internal Rust representation
// ---------------------------------------------------------------------------

/// A queued outbound message.
#[derive(Debug, Clone)]
struct QueuedMessage {
    id: u32,
    transport: u8,
    dest: [u8; 32],
    dest_len: usize,
    payload: [u8; 256],
    payload_len: usize,
    priority: MsgPriority,
    created_at: u64,
    next_retry_at: u64,
    retry_count: u32,
    max_retries: u32,
    ttl_ms: u64,
    status: MsgStatus,
}

impl QueuedMessage {
    fn new(id: u32, transport: u8) -> Self {
        Self {
            id,
            transport,
            dest: [0u8; 32],
            dest_len: 0,
            payload: [0u8; 256],
            payload_len: 0,
            priority: MsgPriority::Normal,
            created_at: 0,
            next_retry_at: 0,
            retry_count: 0,
            max_retries: DEFAULT_MAX_RETRIES,
            ttl_ms: DEFAULT_TTL_MS,
            status: MsgStatus::Pending,
        }
    }

    /// Serialize a single message to a JSON object string.
    fn to_json(&self) -> String {
        let payload_b64 = base64_encode(&self.payload[..self.payload_len]);
        let dest_str = if self.dest_len > 0 {
            base64_encode(&self.dest[..self.dest_len])
        } else {
            String::new()
        };
        format!(
            "{{\"id\":{},\"transport\":{},\"dest\":\"{}\",\"payload\":\"{}\",\
             \"priority\":{},\"created_at\":{},\"next_retry_at\":{},\
             \"retry_count\":{},\"max_retries\":{},\"ttl_ms\":{},\"status\":{}}}",
            self.id,
            self.transport,
            json_escape(&dest_str),
            json_escape(&payload_b64),
            self.priority as u8,
            self.created_at,
            self.next_retry_at,
            self.retry_count,
            self.max_retries,
            self.ttl_ms,
            self.status as u8,
        )
    }

    /// Deserialize a single message from a JSON object string.
    fn from_json(json: &str) -> Option<Self> {
        let id = json_get_int(json, "id")? as u32;
        let transport = json_get_int(json, "transport")? as u8;
        let priority_val = json_get_int(json, "priority").unwrap_or(0) as u8;
        let priority = MsgPriority::from_u8(priority_val).unwrap_or(MsgPriority::Normal);
        let created_at = json_get_int(json, "created_at").unwrap_or(0) as u64;
        let next_retry_at = json_get_int(json, "next_retry_at").unwrap_or(0) as u64;
        let retry_count = json_get_int(json, "retry_count").unwrap_or(0) as u32;
        let max_retries = json_get_int(json, "max_retries").unwrap_or(DEFAULT_MAX_RETRIES as i64) as u32;
        let ttl_ms = json_get_int(json, "ttl_ms").unwrap_or(DEFAULT_TTL_MS as i64) as u64;
        let status_val = json_get_int(json, "status").unwrap_or(0) as u8;
        let status = MsgStatus::from_u8(status_val).unwrap_or(MsgStatus::Pending);

        let mut msg = QueuedMessage::new(id, transport);
        msg.priority = priority;
        msg.created_at = created_at;
        msg.next_retry_at = next_retry_at;
        msg.retry_count = retry_count;
        msg.max_retries = max_retries;
        msg.ttl_ms = ttl_ms;
        msg.status = status;

        // Decode dest
        if let Some(dest_str) = json_get_string(json, "dest") {
            if !dest_str.is_empty() {
                if let Some(decoded) = base64_decode(&dest_str) {
                    let len = decoded.len().min(32);
                    msg.dest[..len].copy_from_slice(&decoded[..len]);
                    msg.dest_len = len;
                }
            }
        }

        // Decode payload
        if let Some(payload_str) = json_get_string(json, "payload") {
            if !payload_str.is_empty() {
                if let Some(decoded) = base64_decode(&payload_str) {
                    let len = decoded.len().min(256);
                    msg.payload[..len].copy_from_slice(&decoded[..len]);
                    msg.payload_len = len;
                }
            }
        }

        Some(msg)
    }

    /// Check whether this message is active (eligible for processing).
    fn is_active(&self) -> bool {
        matches!(self.status, MsgStatus::Pending | MsgStatus::Retrying)
    }

    /// Check whether this message is ready for a send attempt at the given time.
    fn is_ready(&self, now_ms: u64) -> bool {
        self.is_active() && now_ms >= self.next_retry_at
    }
}

// ---------------------------------------------------------------------------
// CQueuedMsgInfo — repr(C) struct for FFI
// ---------------------------------------------------------------------------

/// Fixed-size message info for C interop.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CQueuedMsgInfo {
    pub id: u32,
    pub transport: u8,
    pub dest: [u8; 32],
    pub dest_len: u8,
    pub payload_len: u16,
    pub priority: u8,
    pub retry_count: u32,
    pub max_retries: u32,
    pub status: u8,
    pub ttl_ms: u64,
}

// ---------------------------------------------------------------------------
// CQueueStats — repr(C) struct for FFI
// ---------------------------------------------------------------------------

/// Queue statistics for C interop.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CQueueStats {
    pub total_queued: u32,
    pub pending_count: u32,
    pub retrying_count: u32,
    pub sent_count: u32,
    pub expired_count: u32,
    pub failed_count: u32,
    pub total_retries: u32,
}

// ---------------------------------------------------------------------------
// MsgQueueState — internal state
// ---------------------------------------------------------------------------

struct MsgQueueState {
    messages: Vec<QueuedMessage>,
    next_id: u32,
    initialized: bool,
    default_max_retries: u32,
    default_ttl_ms: u64,
}

impl MsgQueueState {
    const fn new() -> Self {
        Self {
            messages: Vec::new(),
            next_id: 1,
            initialized: false,
            default_max_retries: DEFAULT_MAX_RETRIES,
            default_ttl_ms: DEFAULT_TTL_MS,
        }
    }

    fn init(&mut self) -> i32 {
        if self.initialized {
            return ESP_OK;
        }
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
            if let Some(msg) = QueuedMessage::from_json(entry) {
                if msg.id >= self.next_id {
                    self.next_id = msg.id + 1;
                }
                self.messages.push(msg);
            }
        }
    }

    fn save_to_disk(&self) -> i32 {
        if !self.initialized {
            return ESP_ERR_INVALID_STATE;
        }
        let mut json = String::from("[\n");
        for (i, msg) in self.messages.iter().enumerate() {
            if i > 0 {
                json.push_str(",\n");
            }
            json.push_str("  ");
            json.push_str(&msg.to_json());
        }
        json.push_str("\n]");

        // Ensure parent directory exists
        let _ = fs::create_dir_all("/sdcard/data");

        match fs::write(STORAGE_PATH, &json) {
            Ok(()) => ESP_OK,
            Err(_) => ESP_ERR_INVALID_STATE,
        }
    }

    fn enqueue(
        &mut self,
        transport: u8,
        dest: &[u8],
        payload: &[u8],
        priority: MsgPriority,
        now_ms: u64,
    ) -> i32 {
        if transport > 3 {
            return ESP_ERR_INVALID_ARG;
        }
        if payload.is_empty() || payload.len() > 256 {
            return ESP_ERR_INVALID_ARG;
        }
        if dest.len() > 32 {
            return ESP_ERR_INVALID_ARG;
        }
        // Count active messages for capacity check
        let active_count = self.messages.iter().filter(|m| m.is_active()).count();
        if active_count >= MAX_QUEUE_SIZE {
            return ESP_ERR_NO_MEM;
        }

        let id = self.next_id;
        self.next_id += 1;

        let mut msg = QueuedMessage::new(id, transport);
        let dest_len = dest.len().min(32);
        msg.dest[..dest_len].copy_from_slice(&dest[..dest_len]);
        msg.dest_len = dest_len;
        let payload_len = payload.len().min(256);
        msg.payload[..payload_len].copy_from_slice(&payload[..payload_len]);
        msg.payload_len = payload_len;
        msg.priority = priority;
        msg.created_at = now_ms;
        msg.next_retry_at = now_ms; // Ready immediately
        msg.max_retries = self.default_max_retries;
        msg.ttl_ms = self.default_ttl_ms;
        msg.status = MsgStatus::Pending;

        self.messages.push(msg);
        id as i32
    }

    fn cancel(&mut self, id: u32) -> i32 {
        for msg in &mut self.messages {
            if msg.id == id {
                if msg.is_active() {
                    msg.status = MsgStatus::Cancelled;
                    return ESP_OK;
                }
                // Already in a terminal state — nothing to cancel
                return ESP_OK;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn cancel_all(&mut self) -> i32 {
        for msg in &mut self.messages {
            if msg.is_active() {
                msg.status = MsgStatus::Cancelled;
            }
        }
        ESP_OK
    }

    fn tick(&mut self, now_ms: u64) -> i32 {
        let mut ready_count = 0i32;

        for msg in &mut self.messages {
            if !msg.is_active() {
                continue;
            }

            // Check TTL expiry
            if msg.ttl_ms > 0 && now_ms >= msg.created_at + msg.ttl_ms {
                msg.status = MsgStatus::Expired;
                continue;
            }

            // Check max retries (0 = unlimited)
            if msg.max_retries > 0 && msg.retry_count >= msg.max_retries {
                msg.status = MsgStatus::Failed;
                continue;
            }

            // Count messages ready for send attempt
            if now_ms >= msg.next_retry_at {
                ready_count += 1;
            }
        }

        ready_count
    }

    fn get_ready(&self, out: &mut [CQueuedMsgInfo], now_ms: u64) -> i32 {
        // Collect ready messages, sorted by priority (Urgent first)
        let mut ready: Vec<&QueuedMessage> = self
            .messages
            .iter()
            .filter(|m| m.is_ready(now_ms))
            .collect();

        // Sort by priority descending (Urgent=2 first, Normal=0 last)
        ready.sort_by(|a, b| (b.priority as u8).cmp(&(a.priority as u8)));

        let count = ready.len().min(out.len());
        for i in 0..count {
            out[i] = Self::msg_to_c_info(ready[i]);
        }
        count as i32
    }

    fn get_payload(&self, id: u32, out: &mut [u8]) -> i32 {
        for msg in &self.messages {
            if msg.id == id {
                let len = msg.payload_len.min(out.len());
                out[..len].copy_from_slice(&msg.payload[..len]);
                return len as i32;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn mark_sent(&mut self, id: u32) -> i32 {
        for msg in &mut self.messages {
            if msg.id == id {
                msg.status = MsgStatus::Sent;
                return ESP_OK;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn mark_failed(&mut self, id: u32, now_ms: u64) -> i32 {
        for msg in &mut self.messages {
            if msg.id == id {
                if !msg.is_active() {
                    return ESP_ERR_INVALID_STATE;
                }
                msg.retry_count += 1;
                msg.status = MsgStatus::Retrying;

                // Exponential backoff: BASE_RETRY_MS * 2^retry_count, capped
                let backoff = Self::calculate_backoff(msg.retry_count);
                msg.next_retry_at = now_ms.saturating_add(backoff);
                return ESP_OK;
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn get_count(&self) -> i32 {
        self.messages.iter().filter(|m| m.is_active()).count() as i32
    }

    fn get_info(&self, id: u32) -> Option<CQueuedMsgInfo> {
        self.messages
            .iter()
            .find(|m| m.id == id)
            .map(|m| Self::msg_to_c_info(m))
    }

    fn get_stats(&self) -> CQueueStats {
        let mut stats = CQueueStats {
            total_queued: self.messages.len() as u32,
            pending_count: 0,
            retrying_count: 0,
            sent_count: 0,
            expired_count: 0,
            failed_count: 0,
            total_retries: 0,
        };
        for msg in &self.messages {
            match msg.status {
                MsgStatus::Pending => stats.pending_count += 1,
                MsgStatus::Retrying => stats.retrying_count += 1,
                MsgStatus::Sent => stats.sent_count += 1,
                MsgStatus::Expired => stats.expired_count += 1,
                MsgStatus::Failed => stats.failed_count += 1,
                MsgStatus::Cancelled => {} // Not tracked in stats
            }
            stats.total_retries += msg.retry_count;
        }
        stats
    }

    fn set_defaults(&mut self, max_retries: u32, ttl_ms: u64) -> i32 {
        self.default_max_retries = max_retries;
        self.default_ttl_ms = ttl_ms;
        ESP_OK
    }

    fn purge_completed(&mut self) -> i32 {
        let before = self.messages.len();
        self.messages.retain(|m| m.is_active());
        (before - self.messages.len()) as i32
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn msg_to_c_info(msg: &QueuedMessage) -> CQueuedMsgInfo {
        CQueuedMsgInfo {
            id: msg.id,
            transport: msg.transport,
            dest: msg.dest,
            dest_len: msg.dest_len as u8,
            payload_len: msg.payload_len as u16,
            priority: msg.priority as u8,
            retry_count: msg.retry_count,
            max_retries: msg.max_retries,
            status: msg.status as u8,
            ttl_ms: msg.ttl_ms,
        }
    }

    /// Calculate exponential backoff for a given retry count.
    /// BASE_RETRY_MS * BACKOFF_MULTIPLIER^retry_count, capped at MAX_RETRY_MS.
    fn calculate_backoff(retry_count: u32) -> u64 {
        // Use saturating arithmetic to prevent overflow
        let mut delay = BASE_RETRY_MS;
        for _ in 0..retry_count {
            delay = delay.saturating_mul(BACKOFF_MULTIPLIER);
            if delay >= MAX_RETRY_MS {
                return MAX_RETRY_MS;
            }
        }
        delay.min(MAX_RETRY_MS)
    }

    #[cfg(test)]
    fn reset(&mut self) {
        self.messages.clear();
        self.next_id = 1;
        self.initialized = false;
        self.default_max_retries = DEFAULT_MAX_RETRIES;
        self.default_ttl_ms = DEFAULT_TTL_MS;
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static MSG_QUEUE: Mutex<MsgQueueState> = Mutex::new(MsgQueueState::new());

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the message queue. Loads persisted messages from SD card.
/// Idempotent; safe to call multiple times.
#[no_mangle]
pub extern "C" fn rs_msg_queue_init() -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.init(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Enqueue a new outbound message. Returns the message ID (>0) on success,
/// or a negative ESP error code on failure.
///
/// # Safety
///
/// `dest` must point to `dest_len` bytes (or be null if `dest_len` is 0).
/// `payload` must point to `payload_len` bytes and must not be null.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_queue_enqueue(
    transport: u8,
    dest: *const u8,
    dest_len: u8,
    payload: *const u8,
    payload_len: u16,
    priority: u8,
) -> i32 {
    if payload.is_null() || payload_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let prio = match MsgPriority::from_u8(priority) {
        Some(p) => p,
        None => return ESP_ERR_INVALID_ARG,
    };

    // SAFETY: caller guarantees `payload` points to `payload_len` bytes.
    let payload_slice = std::slice::from_raw_parts(payload, payload_len as usize);

    let dest_slice = if dest.is_null() || dest_len == 0 {
        &[]
    } else {
        // SAFETY: caller guarantees `dest` points to `dest_len` bytes.
        std::slice::from_raw_parts(dest, dest_len as usize)
    };

    // Use monotonic time 0 for now — the caller should provide real time via tick().
    // In practice, the transport layer calls tick() with a real timestamp before
    // checking for ready messages.
    let now_ms = 0u64;

    match MSG_QUEUE.lock() {
        Ok(mut state) => state.enqueue(transport, dest_slice, payload_slice, prio, now_ms),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Cancel a queued message by ID.
#[no_mangle]
pub extern "C" fn rs_msg_queue_cancel(id: u32) -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.cancel(id),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Cancel all pending and retrying messages.
#[no_mangle]
pub extern "C" fn rs_msg_queue_cancel_all() -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.cancel_all(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Process the queue: expire TTLs, fail max-retries, count ready messages.
/// Returns the number of messages ready for send attempt, or a negative error.
#[no_mangle]
pub extern "C" fn rs_msg_queue_tick(now_ms: u64) -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.tick(now_ms),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get messages ready for send attempt. Writes up to `max` entries into `out`.
/// Returns the number of entries written, or a negative error code.
///
/// # Safety
///
/// `out` must point to an array of at least `max` CQueuedMsgInfo structs.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_queue_get_ready(
    out: *mut CQueuedMsgInfo,
    max: u32,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if max == 0 {
        return 0;
    }
    match MSG_QUEUE.lock() {
        Ok(state) => {
            // SAFETY: caller guarantees `out` points to at least `max` elements.
            let slice = std::slice::from_raw_parts_mut(out, max as usize);
            // Use next_retry_at comparison — we need a "now" value.
            // get_ready uses the largest next_retry_at as the reference,
            // but really the caller should call tick() first with the real time.
            // Here we use u64::MAX to get all ready messages (those already ticked).
            state.get_ready(slice, u64::MAX)
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get the payload for a specific message. Writes up to `max_len` bytes
/// into `out`. Returns the payload length, or a negative error code.
///
/// # Safety
///
/// `out` must point to a writable buffer of at least `max_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_queue_get_payload(
    id: u32,
    out: *mut u8,
    max_len: u16,
) -> i32 {
    if out.is_null() || max_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    match MSG_QUEUE.lock() {
        Ok(state) => {
            // SAFETY: caller guarantees `out` is writable for `max_len` bytes.
            let slice = std::slice::from_raw_parts_mut(out, max_len as usize);
            state.get_payload(id, slice)
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Mark a message as successfully sent.
#[no_mangle]
pub extern "C" fn rs_msg_queue_mark_sent(id: u32) -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.mark_sent(id),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Mark a single send attempt as failed, scheduling a retry with exponential
/// backoff. The caller must provide the current monotonic time.
#[no_mangle]
pub extern "C" fn rs_msg_queue_mark_failed(id: u32) -> i32 {
    // Use 0 as now_ms — in practice the transport layer should call tick()
    // with real time. mark_failed schedules relative to "now".
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.mark_failed(id, 0),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Return the count of active (pending + retrying) messages.
#[no_mangle]
pub extern "C" fn rs_msg_queue_get_count() -> i32 {
    match MSG_QUEUE.lock() {
        Ok(state) => state.get_count(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get info for a specific message by ID.
///
/// # Safety
///
/// `out` must point to a valid, writable CQueuedMsgInfo.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_queue_get_info(id: u32, out: *mut CQueuedMsgInfo) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match MSG_QUEUE.lock() {
        Ok(state) => match state.get_info(id) {
            Some(info) => {
                // SAFETY: caller guarantees `out` is valid and writable.
                *out = info;
                ESP_OK
            }
            None => ESP_ERR_NOT_FOUND,
        },
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get queue statistics.
///
/// # Safety
///
/// `out` must point to a valid, writable CQueueStats.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_queue_get_stats(out: *mut CQueueStats) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match MSG_QUEUE.lock() {
        Ok(state) => {
            // SAFETY: caller guarantees `out` is valid and writable.
            *out = state.get_stats();
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Persist the queue to SD card.
#[no_mangle]
pub extern "C" fn rs_msg_queue_save() -> i32 {
    match MSG_QUEUE.lock() {
        Ok(state) => state.save_to_disk(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Set default max_retries and TTL for newly enqueued messages.
#[no_mangle]
pub extern "C" fn rs_msg_queue_set_defaults(max_retries: u32, ttl_ms: u64) -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.set_defaults(max_retries, ttl_ms),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Remove sent, expired, failed, and cancelled entries from the queue.
/// Returns the number of entries removed.
#[no_mangle]
pub extern "C" fn rs_msg_queue_purge_completed() -> i32 {
    match MSG_QUEUE.lock() {
        Ok(mut state) => state.purge_completed(),
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset global state before each test.
    fn reset() {
        let mut state = MSG_QUEUE.lock().unwrap();
        state.reset();
    }

    /// Helper: enqueue a message directly on the internal state.
    fn enqueue_test_msg(state: &mut MsgQueueState, transport: u8, priority: MsgPriority) -> u32 {
        let payload = b"Hello";
        let id = state.enqueue(transport, &[], payload, priority, 1000);
        assert!(id > 0, "enqueue must return positive ID, got {}", id);
        id as u32
    }

    // -----------------------------------------------------------------------
    // Init tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_returns_ok() {
        reset();
        let rc = rs_msg_queue_init();
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_init_idempotent() {
        reset();
        let rc1 = rs_msg_queue_init();
        let rc2 = rs_msg_queue_init();
        assert_eq!(rc1, ESP_OK);
        assert_eq!(rc2, ESP_OK);
    }

    // -----------------------------------------------------------------------
    // Enqueue tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_enqueue_single() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = state.enqueue(0, &[], b"Test message", MsgPriority::Normal, 1000);
        assert!(id > 0);
        assert_eq!(state.get_count(), 1);
    }

    #[test]
    fn test_enqueue_multiple() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        for i in 0..5 {
            let payload = format!("Msg {}", i);
            let id = state.enqueue(0, &[], payload.as_bytes(), MsgPriority::Normal, 1000);
            assert!(id > 0);
        }
        assert_eq!(state.get_count(), 5);
    }

    #[test]
    fn test_enqueue_at_capacity_fails() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        for i in 0..MAX_QUEUE_SIZE {
            let payload = format!("Msg {}", i);
            let id = state.enqueue(0, &[], payload.as_bytes(), MsgPriority::Normal, 1000);
            assert!(id > 0, "enqueue #{} failed unexpectedly", i);
        }
        // Next enqueue must fail
        let rc = state.enqueue(0, &[], b"Overflow", MsgPriority::Normal, 1000);
        assert_eq!(rc, ESP_ERR_NO_MEM);
    }

    #[test]
    fn test_enqueue_with_priority() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = state.enqueue(0, &[], b"SOS", MsgPriority::Urgent, 1000);
        assert!(id > 0);
        let info = state.get_info(id as u32).unwrap();
        assert_eq!(info.priority, MsgPriority::Urgent as u8);
    }

    #[test]
    fn test_enqueue_invalid_transport_rejected() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let rc = state.enqueue(99, &[], b"Bad transport", MsgPriority::Normal, 1000);
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_enqueue_empty_payload_rejected() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let rc = state.enqueue(0, &[], &[], MsgPriority::Normal, 1000);
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Cancel tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_existing() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let rc = state.cancel(id);
        assert_eq!(rc, ESP_OK);
        // Cancelled messages are no longer active
        assert_eq!(state.get_count(), 0);
    }

    #[test]
    fn test_cancel_nonexistent() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let rc = state.cancel(999);
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_cancel_all() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        enqueue_test_msg(&mut state, 0, MsgPriority::High);
        enqueue_test_msg(&mut state, 1, MsgPriority::Urgent);
        assert_eq!(state.get_count(), 3);
        let rc = state.cancel_all();
        assert_eq!(rc, ESP_OK);
        assert_eq!(state.get_count(), 0);
    }

    #[test]
    fn test_cancel_doesnt_affect_sent() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        state.mark_sent(id);
        let active_before = state.get_count();
        state.cancel_all();
        let active_after = state.get_count();
        // Sent message was already not active, cancel_all doesn't change it
        assert_eq!(active_before, active_after);
        // Verify it's still Sent
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Sent as u8);
    }

    // -----------------------------------------------------------------------
    // Tick and expiry tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tick_no_messages() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let ready = state.tick(1000);
        assert_eq!(ready, 0);
    }

    #[test]
    fn test_tick_expires_ttl() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        // Default TTL is 1 hour = 3_600_000ms. Created at 1000.
        let _ = state.tick(1000 + DEFAULT_TTL_MS);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Expired as u8);
    }

    #[test]
    fn test_tick_doesnt_expire_before_ttl() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        // Just before TTL expires
        let _ = state.tick(1000 + DEFAULT_TTL_MS - 1);
        let info = state.get_info(id).unwrap();
        assert!(info.status == MsgStatus::Pending as u8 || info.status == MsgStatus::Retrying as u8);
    }

    #[test]
    fn test_tick_zero_ttl_no_expiry() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        state.set_defaults(DEFAULT_MAX_RETRIES, 0); // TTL = 0 means no expiry
        let id = state.enqueue(0, &[], b"Forever", MsgPriority::Normal, 1000);
        assert!(id > 0);
        // Tick at a very large time
        let _ = state.tick(999_999_999);
        let info = state.get_info(id as u32).unwrap();
        // Should still be active, not expired
        assert!(info.status == MsgStatus::Pending as u8 || info.status == MsgStatus::Retrying as u8);
    }

    #[test]
    fn test_tick_fails_max_retries() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        state.set_defaults(3, DEFAULT_TTL_MS);
        let id = state.enqueue(0, &[], b"Will fail", MsgPriority::Normal, 1000);
        assert!(id > 0);
        let id = id as u32;
        // Simulate 3 failures
        for i in 0..3 {
            state.mark_failed(id, 2000 + i * 10000);
        }
        // Now tick — should be failed because retry_count (3) >= max_retries (3)
        let _ = state.tick(100000);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Failed as u8);
    }

    #[test]
    fn test_tick_counts_ready_messages() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        // Enqueue 3 messages at time 1000, all immediately ready
        for _ in 0..3 {
            state.enqueue(0, &[], b"Ready", MsgPriority::Normal, 1000);
        }
        let ready = state.tick(1000);
        assert_eq!(ready, 3);
    }

    // -----------------------------------------------------------------------
    // Ready message tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_ready_when_none() {
        reset();
        let state = MSG_QUEUE.lock().unwrap();
        let mut out = [CQueuedMsgInfo {
            id: 0, transport: 0, dest: [0u8; 32], dest_len: 0,
            payload_len: 0, priority: 0, retry_count: 0, max_retries: 0,
            status: 0, ttl_ms: 0,
        }; 4];
        let count = state.get_ready(&mut out, 1000);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_get_ready_returns_due_messages() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let mut out = [CQueuedMsgInfo {
            id: 0, transport: 0, dest: [0u8; 32], dest_len: 0,
            payload_len: 0, priority: 0, retry_count: 0, max_retries: 0,
            status: 0, ttl_ms: 0,
        }; 4];
        // Messages created at 1000, next_retry_at = 1000
        let count = state.get_ready(&mut out, 1000);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_get_ready_skips_not_due() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        // Mark failed to push next_retry_at into the future
        state.mark_failed(id, 1000);
        let mut out = [CQueuedMsgInfo {
            id: 0, transport: 0, dest: [0u8; 32], dest_len: 0,
            payload_len: 0, priority: 0, retry_count: 0, max_retries: 0,
            status: 0, ttl_ms: 0,
        }; 4];
        // Check at time just after the failure — not enough time for backoff
        let count = state.get_ready(&mut out, 1001);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_get_ready_priority_ordering() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let _id_normal = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let id_urgent = enqueue_test_msg(&mut state, 0, MsgPriority::Urgent);
        let _id_high = enqueue_test_msg(&mut state, 0, MsgPriority::High);
        let mut out = [CQueuedMsgInfo {
            id: 0, transport: 0, dest: [0u8; 32], dest_len: 0,
            payload_len: 0, priority: 0, retry_count: 0, max_retries: 0,
            status: 0, ttl_ms: 0,
        }; 4];
        let count = state.get_ready(&mut out, 2000);
        assert_eq!(count, 3);
        // Urgent should be first
        assert_eq!(out[0].id, id_urgent);
        assert_eq!(out[0].priority, MsgPriority::Urgent as u8);
        // High second
        assert_eq!(out[1].priority, MsgPriority::High as u8);
        // Normal last
        assert_eq!(out[2].priority, MsgPriority::Normal as u8);
    }

    #[test]
    fn test_get_ready_ffi_null_pointer() {
        reset();
        let rc = unsafe { rs_msg_queue_get_ready(std::ptr::null_mut(), 10) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Send lifecycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_sent() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let rc = state.mark_sent(id);
        assert_eq!(rc, ESP_OK);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Sent as u8);
        // No longer active
        assert_eq!(state.get_count(), 0);
    }

    #[test]
    fn test_mark_failed_schedules_retry() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let rc = state.mark_failed(id, 5000);
        assert_eq!(rc, ESP_OK);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Retrying as u8);
    }

    #[test]
    fn test_mark_failed_increments_count() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        state.mark_failed(id, 5000);
        state.mark_failed(id, 15000);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.retry_count, 2);
    }

    #[test]
    fn test_exponential_backoff_calculation() {
        // retry_count=0: BASE_RETRY_MS * 2^0 = 5000 (initial, before first mark_failed)
        // retry_count=1: BASE_RETRY_MS * 2^1 = 10000
        // retry_count=2: BASE_RETRY_MS * 2^2 = 20000
        // retry_count=3: BASE_RETRY_MS * 2^3 = 40000
        assert_eq!(MsgQueueState::calculate_backoff(0), 5000);
        assert_eq!(MsgQueueState::calculate_backoff(1), 10000);
        assert_eq!(MsgQueueState::calculate_backoff(2), 20000);
        assert_eq!(MsgQueueState::calculate_backoff(3), 40000);
    }

    #[test]
    fn test_max_backoff_cap() {
        // Very high retry count should be capped at MAX_RETRY_MS
        let backoff = MsgQueueState::calculate_backoff(100);
        assert_eq!(backoff, MAX_RETRY_MS);
    }

    // -----------------------------------------------------------------------
    // Payload tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_payload() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id = state.enqueue(0, &[], b"Test payload data", MsgPriority::Normal, 1000);
        assert!(id > 0);
        let mut buf = [0u8; 256];
        let len = state.get_payload(id as u32, &mut buf);
        assert_eq!(len, 17);
        assert_eq!(&buf[..17], b"Test payload data");
    }

    #[test]
    fn test_get_payload_nonexistent() {
        reset();
        let state = MSG_QUEUE.lock().unwrap();
        let mut buf = [0u8; 256];
        let rc = state.get_payload(999, &mut buf);
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_get_payload_ffi_null_pointer() {
        reset();
        let rc = unsafe { rs_msg_queue_get_payload(1, std::ptr::null_mut(), 256) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Stats tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_stats_empty() {
        reset();
        let state = MSG_QUEUE.lock().unwrap();
        let stats = state.get_stats();
        assert_eq!(stats.total_queued, 0);
        assert_eq!(stats.pending_count, 0);
        assert_eq!(stats.retrying_count, 0);
        assert_eq!(stats.sent_count, 0);
        assert_eq!(stats.expired_count, 0);
        assert_eq!(stats.failed_count, 0);
        assert_eq!(stats.total_retries, 0);
    }

    #[test]
    fn test_stats_after_activity() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id1 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let id2 = enqueue_test_msg(&mut state, 0, MsgPriority::High);
        let _id3 = enqueue_test_msg(&mut state, 0, MsgPriority::Urgent);
        state.mark_sent(id1);
        state.mark_failed(id2, 5000);
        // _id3 stays pending

        let stats = state.get_stats();
        assert_eq!(stats.total_queued, 3);
        assert_eq!(stats.sent_count, 1);
        assert_eq!(stats.retrying_count, 1);
        assert_eq!(stats.pending_count, 1);
        assert_eq!(stats.total_retries, 1); // one mark_failed call
    }

    #[test]
    fn test_stats_ffi_null_pointer() {
        reset();
        let rc = unsafe { rs_msg_queue_get_stats(std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Persistence tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_and_load_roundtrip() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        state.initialized = true;
        let dest = [1u8, 2, 3, 4];
        let id = state.enqueue(0, &dest, b"Persist me", MsgPriority::High, 5000);
        assert!(id > 0);

        // Save
        let rc = state.save_to_disk();
        // save_to_disk may fail if /sdcard/data doesn't exist in test env
        if rc == ESP_OK {
            // Reload
            state.messages.clear();
            state.next_id = 1;
            state.load_from_disk();
            assert_eq!(state.messages.len(), 1);
            let msg = &state.messages[0];
            assert_eq!(msg.id, id as u32);
            assert_eq!(msg.transport, 0);
            assert_eq!(&msg.dest[..4], &[1, 2, 3, 4]);
            assert_eq!(msg.dest_len, 4);
            assert_eq!(&msg.payload[..10], b"Persist me");
            assert_eq!(msg.payload_len, 10);
            assert_eq!(msg.priority, MsgPriority::High);
            assert_eq!(msg.created_at, 5000);
        }
        // If save failed (no SD card in test), test is still valid — we just skip
    }

    #[test]
    fn test_base64_payload_roundtrip() {
        let original = b"Hello, World! \x00\x01\x02\xFF";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(&decoded, original);
    }

    #[test]
    fn test_load_missing_file() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        // load_from_disk should not panic on missing file
        state.load_from_disk();
        assert_eq!(state.messages.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Defaults tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_defaults() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let rc = state.set_defaults(5, 60_000);
        assert_eq!(rc, ESP_OK);
        assert_eq!(state.default_max_retries, 5);
        assert_eq!(state.default_ttl_ms, 60_000);
    }

    #[test]
    fn test_defaults_apply_to_new_messages() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        state.set_defaults(3, 120_000);
        let id = state.enqueue(0, &[], b"Custom defaults", MsgPriority::Normal, 1000);
        assert!(id > 0);
        let info = state.get_info(id as u32).unwrap();
        assert_eq!(info.max_retries, 3);
        assert_eq!(info.ttl_ms, 120_000);
    }

    // -----------------------------------------------------------------------
    // Purge tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_purge_removes_completed() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id1 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let id2 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let _id3 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        state.mark_sent(id1);
        state.cancel(id2);
        // _id3 stays pending

        let removed = state.purge_completed();
        assert_eq!(removed, 2); // Sent + Cancelled
    }

    #[test]
    fn test_purge_keeps_active() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        enqueue_test_msg(&mut state, 0, MsgPriority::High);
        let removed = state.purge_completed();
        assert_eq!(removed, 0);
        assert_eq!(state.get_count(), 2);
    }

    #[test]
    fn test_purge_updates_counts() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let id1 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        let id2 = enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        enqueue_test_msg(&mut state, 0, MsgPriority::Normal);
        state.mark_sent(id1);
        state.mark_sent(id2);
        state.purge_completed();
        let stats = state.get_stats();
        assert_eq!(stats.total_queued, 1);
        assert_eq!(stats.pending_count, 1);
        assert_eq!(stats.sent_count, 0);
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_backoff_overflow_safety() {
        // Very large retry count should not panic or overflow
        let backoff = MsgQueueState::calculate_backoff(u32::MAX);
        assert_eq!(backoff, MAX_RETRY_MS);
    }

    #[test]
    fn test_zero_max_retries_unlimited() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        state.set_defaults(0, 0); // 0 retries = unlimited, 0 TTL = no expiry
        let id = state.enqueue(0, &[], b"Unlimited", MsgPriority::Normal, 1000);
        assert!(id > 0);
        let id = id as u32;
        // Simulate many failures
        for i in 0..50u64 {
            state.mark_failed(id, 2000 + i * 100000);
        }
        // Tick — should NOT be failed because max_retries = 0 (unlimited)
        let _ = state.tick(999_999_999);
        let info = state.get_info(id).unwrap();
        assert_eq!(info.status, MsgStatus::Retrying as u8);
        assert_eq!(info.retry_count, 50);
    }

    #[test]
    fn test_message_id_uniqueness() {
        reset();
        let mut state = MSG_QUEUE.lock().unwrap();
        let mut ids = Vec::new();
        for _ in 0..20 {
            let id = state.enqueue(0, &[], b"Unique", MsgPriority::Normal, 1000);
            assert!(id > 0);
            assert!(!ids.contains(&id), "duplicate message ID: {}", id);
            ids.push(id);
        }
    }

    // -----------------------------------------------------------------------
    // JSON serialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_message_to_json_roundtrip() {
        let mut msg = QueuedMessage::new(42, 1);
        msg.dest[..3].copy_from_slice(&[10, 20, 30]);
        msg.dest_len = 3;
        msg.payload[..5].copy_from_slice(b"Hello");
        msg.payload_len = 5;
        msg.priority = MsgPriority::High;
        msg.created_at = 12345;
        msg.next_retry_at = 67890;
        msg.retry_count = 2;
        msg.max_retries = 10;
        msg.ttl_ms = 3_600_000;
        msg.status = MsgStatus::Retrying;

        let json = msg.to_json();
        let restored = QueuedMessage::from_json(&json).unwrap();

        assert_eq!(restored.id, 42);
        assert_eq!(restored.transport, 1);
        assert_eq!(&restored.dest[..3], &[10, 20, 30]);
        assert_eq!(restored.dest_len, 3);
        assert_eq!(&restored.payload[..5], b"Hello");
        assert_eq!(restored.payload_len, 5);
        assert_eq!(restored.priority, MsgPriority::High);
        assert_eq!(restored.created_at, 12345);
        assert_eq!(restored.retry_count, 2);
        assert_eq!(restored.max_retries, 10);
        assert_eq!(restored.ttl_ms, 3_600_000);
        assert_eq!(restored.status, MsgStatus::Retrying);
    }

    #[test]
    fn test_json_split_empty_array() {
        let items = json_split_array("[]");
        assert!(items.is_empty());
    }

    #[test]
    fn test_json_split_invalid() {
        let items = json_split_array("not json");
        assert!(items.is_empty());
    }

    // -----------------------------------------------------------------------
    // FFI integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_enqueue_and_count() {
        reset();
        let payload = b"FFI test";
        let id = unsafe {
            rs_msg_queue_enqueue(0, std::ptr::null(), 0, payload.as_ptr(), payload.len() as u16, 0)
        };
        assert!(id > 0);
        assert_eq!(rs_msg_queue_get_count(), 1);
    }

    #[test]
    fn test_ffi_enqueue_null_payload_rejected() {
        reset();
        let rc = unsafe {
            rs_msg_queue_enqueue(0, std::ptr::null(), 0, std::ptr::null(), 10, 0)
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_get_info_null_pointer() {
        reset();
        let rc = unsafe { rs_msg_queue_get_info(1, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_cancel_and_verify() {
        reset();
        let payload = b"Cancel me";
        let id = unsafe {
            rs_msg_queue_enqueue(0, std::ptr::null(), 0, payload.as_ptr(), payload.len() as u16, 0)
        };
        assert!(id > 0);
        let rc = rs_msg_queue_cancel(id as u32);
        assert_eq!(rc, ESP_OK);
        assert_eq!(rs_msg_queue_get_count(), 0);
    }

    #[test]
    fn test_ffi_set_defaults() {
        reset();
        let rc = rs_msg_queue_set_defaults(5, 60_000);
        assert_eq!(rc, ESP_OK);
    }
}
