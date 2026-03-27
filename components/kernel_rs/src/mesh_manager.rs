// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — mesh_manager module
//
// Rust wrapper around the MeshCore C shim (components/meshcore/include/meshcore_shim.h).
// Provides mesh networking: send/receive messages, discover contacts, broadcast
// advertisements, and query network statistics.
//
// On ESP-IDF targets, calls through to the real MeshCore C functions.
// On simulator/test builds, all hardware calls are stubbed.

use std::os::raw::c_char;
#[cfg(target_os = "espidf")]
use std::os::raw::c_void;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_FAIL: i32 = -1;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const INBOX_SIZE: usize = 32;
const MESH_NAME_MAX: usize = 32;

// ---------------------------------------------------------------------------
// FFI structs — C-compatible
// ---------------------------------------------------------------------------

/// A discovered mesh contact.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CMeshContact {
    pub pub_key: [u8; 32],
    pub name: [u8; 32],
    pub name_len: u8,
    pub node_type: u8,
    pub last_rssi: i8,
    pub path_len: u8,
    pub last_seen: u32,
    pub lat: f64,
    pub lon: f64,
    pub has_position: bool,
}

impl Default for CMeshContact {
    fn default() -> Self {
        Self {
            pub_key: [0u8; 32],
            name: [0u8; 32],
            name_len: 0,
            node_type: 0,
            last_rssi: 0,
            path_len: 0,
            last_seen: 0,
            lat: 0.0,
            lon: 0.0,
            has_position: false,
        }
    }
}

/// A received mesh message stored in the inbox ring buffer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CMeshMessage {
    pub sender_key: [u8; 32],
    pub sender_name: [u8; 32],
    pub sender_name_len: u8,
    pub timestamp: u32,
    pub text: [u8; 200],
    pub text_len: u16,
}

impl Default for CMeshMessage {
    fn default() -> Self {
        Self {
            sender_key: [0u8; 32],
            sender_name: [0u8; 32],
            sender_name_len: 0,
            timestamp: 0,
            text: [0u8; 200],
            text_len: 0,
        }
    }
}

/// Network statistics from the mesh layer.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CMeshStats {
    pub packets_sent: u32,
    pub packets_received: u32,
    pub packets_forwarded: u32,
    pub messages_sent: u32,
    pub messages_received: u32,
    pub contacts_discovered: u32,
}

// ---------------------------------------------------------------------------
// MeshCore C shim FFI (hardware only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn meshcore_init(node_name: *const c_char, node_type: u8) -> i32;
    fn meshcore_deinit() -> i32;
    fn meshcore_loop() -> i32;
    fn meshcore_send_message(dest_pub_key: *const u8, text: *const c_char) -> i32;
    fn meshcore_send_advert() -> i32;
    fn meshcore_send_advert_with_position(lat: f64, lon: f64) -> i32;
    fn meshcore_get_contact_count() -> i32;
    fn meshcore_get_contact(index: i32, out: *mut CMeshContact) -> i32;
    fn meshcore_find_contact(pub_key: *const u8) -> i32;
    fn meshcore_set_message_callback(
        cb: Option<unsafe extern "C" fn(*const CMeshContact, u32, *const c_char, *mut c_void)>,
        user_data: *mut c_void,
    );
    fn meshcore_set_contact_callback(
        cb: Option<unsafe extern "C" fn(*const CMeshContact, *mut c_void)>,
        user_data: *mut c_void,
    );
    fn meshcore_get_self_pub_key(out_key: *mut u8) -> i32;
    fn meshcore_get_self_name() -> *const c_char;
    fn meshcore_get_stats(out: *mut CMeshStats) -> i32;
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct MeshManagerState {
    initialized: bool,
    node_name: [u8; MESH_NAME_MAX],
    node_name_len: usize,
    node_type: u8,
    // Message inbox (ring buffer of recent received messages)
    inbox: [CMeshMessage; INBOX_SIZE],
    inbox_count: usize,
    inbox_head: usize,
}

impl MeshManagerState {
    const fn new() -> Self {
        // const-compatible default initialisation
        Self {
            initialized: false,
            node_name: [0u8; MESH_NAME_MAX],
            node_name_len: 0,
            node_type: 0,
            inbox: [CMeshMessage {
                sender_key: [0u8; 32],
                sender_name: [0u8; 32],
                sender_name_len: 0,
                timestamp: 0,
                text: [0u8; 200],
                text_len: 0,
            }; INBOX_SIZE],
            inbox_count: 0,
            inbox_head: 0,
        }
    }

    /// Push a message into the ring buffer. Overwrites oldest on overflow.
    fn inbox_push(&mut self, msg: CMeshMessage) {
        let write_idx = (self.inbox_head + self.inbox_count) % INBOX_SIZE;
        self.inbox[write_idx] = msg;
        if self.inbox_count < INBOX_SIZE {
            self.inbox_count += 1;
        } else {
            // Buffer full — advance head to drop the oldest message
            self.inbox_head = (self.inbox_head + 1) % INBOX_SIZE;
        }
    }

    /// Read a message by logical index (0 = oldest).
    fn inbox_get(&self, index: usize) -> Option<&CMeshMessage> {
        if index >= self.inbox_count {
            return None;
        }
        let real_idx = (self.inbox_head + index) % INBOX_SIZE;
        Some(&self.inbox[real_idx])
    }

    /// Clear the inbox.
    fn inbox_clear(&mut self) {
        self.inbox_count = 0;
        self.inbox_head = 0;
    }
}

static MESH_STATE: Mutex<MeshManagerState> = Mutex::new(MeshManagerState::new());

// ---------------------------------------------------------------------------
// Callbacks (hardware only)
// ---------------------------------------------------------------------------

/// Called by MeshCore when a message is received. Pushes into the inbox.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn on_mesh_message(
    sender: *const CMeshContact,
    timestamp: u32,
    text: *const c_char,
    _ud: *mut c_void,
) {
    if sender.is_null() || text.is_null() {
        return;
    }
    let contact = &*sender;
    let c_str = std::ffi::CStr::from_ptr(text);
    let text_bytes = c_str.to_bytes();

    let mut msg = CMeshMessage::default();
    msg.sender_key = contact.pub_key;
    msg.sender_name = contact.name;
    msg.sender_name_len = contact.name_len;
    msg.timestamp = timestamp;

    let copy_len = text_bytes.len().min(msg.text.len());
    msg.text[..copy_len].copy_from_slice(&text_bytes[..copy_len]);
    msg.text_len = copy_len as u16;

    if let Ok(mut state) = MESH_STATE.lock() {
        state.inbox_push(msg);
    }
}

/// Called by MeshCore when a contact is discovered or updated.
/// We don't store contacts ourselves — the C shim maintains that list.
/// This callback is a no-op placeholder for future event-bus integration.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn on_mesh_contact(
    _contact: *const CMeshContact,
    _ud: *mut c_void,
) {
    // Future: publish an event via the event bus
}

// ---------------------------------------------------------------------------
// Static null-terminated name for rs_mesh_get_self_name fallback
// ---------------------------------------------------------------------------

static EMPTY_NAME: &[u8] = b"\0";

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the mesh manager. Calls meshcore_init on hardware.
#[no_mangle]
pub extern "C" fn rs_mesh_init(name: *const c_char, node_type: u8) -> i32 {
    if name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let mut state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Copy node name into state
    let name_bytes = unsafe { std::ffi::CStr::from_ptr(name).to_bytes() };
    let copy_len = name_bytes.len().min(MESH_NAME_MAX - 1);
    state.node_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    state.node_name[copy_len] = 0; // null-terminate
    state.node_name_len = copy_len;
    state.node_type = node_type;
    state.inbox_clear();

    #[cfg(target_os = "espidf")]
    {
        let rc = unsafe { meshcore_init(name, node_type) };
        if rc != ESP_OK {
            return rc;
        }
        // Register callbacks
        unsafe {
            meshcore_set_message_callback(Some(on_mesh_message), std::ptr::null_mut());
            meshcore_set_contact_callback(Some(on_mesh_contact), std::ptr::null_mut());
        }
    }

    state.initialized = true;
    ESP_OK
}

/// Deinitialise the mesh manager.
#[no_mangle]
pub extern "C" fn rs_mesh_deinit() -> i32 {
    let mut state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    {
        let rc = unsafe { meshcore_deinit() };
        if rc != ESP_OK {
            return rc;
        }
    }

    state.initialized = false;
    state.inbox_clear();
    ESP_OK
}

/// Run one iteration of the mesh protocol loop. Must be called periodically.
#[no_mangle]
pub extern "C" fn rs_mesh_loop() -> i32 {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state); // release lock before calling into C

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_loop() };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        ESP_OK
    }
}

/// Send a text message to a specific destination (identified by public key).
#[no_mangle]
pub extern "C" fn rs_mesh_send(dest_key: *const u8, text: *const c_char) -> i32 {
    if dest_key.is_null() || text.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_send_message(dest_key, text) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        ESP_OK
    }
}

/// Broadcast a self-advertisement to nearby nodes.
#[no_mangle]
pub extern "C" fn rs_mesh_send_advert() -> i32 {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_send_advert() };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        ESP_OK
    }
}

/// Broadcast a self-advertisement with GPS position.
#[no_mangle]
pub extern "C" fn rs_mesh_send_advert_pos(lat: f64, lon: f64) -> i32 {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_send_advert_with_position(lat, lon) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        ESP_OK
    }
}

/// Return the number of discovered contacts.
#[no_mangle]
pub extern "C" fn rs_mesh_get_contact_count() -> i32 {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    if !state.initialized {
        return -1;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_get_contact_count() };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        0
    }
}

/// Get a contact by index. Writes into the provided output struct.
#[no_mangle]
pub extern "C" fn rs_mesh_get_contact(index: i32, out: *mut CMeshContact) -> i32 {
    if out.is_null() || index < 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_get_contact(index, out) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        ESP_ERR_INVALID_ARG
    }
}

/// Find a contact by public key. Returns the index or -1 if not found.
#[no_mangle]
pub extern "C" fn rs_mesh_find_contact(pub_key: *const u8) -> i32 {
    if pub_key.is_null() {
        return -1;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    if !state.initialized {
        return -1;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_find_contact(pub_key) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        -1
    }
}

/// Return the number of messages in the inbox.
#[no_mangle]
pub extern "C" fn rs_mesh_get_inbox_count() -> i32 {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    if !state.initialized {
        return -1;
    }

    state.inbox_count as i32
}

/// Read a message from the inbox by index (0 = oldest).
#[no_mangle]
pub extern "C" fn rs_mesh_get_inbox_message(index: i32, out: *mut CMeshMessage) -> i32 {
    if out.is_null() || index < 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    match state.inbox_get(index as usize) {
        Some(msg) => {
            unsafe { *out = *msg; }
            ESP_OK
        }
        None => ESP_ERR_INVALID_ARG,
    }
}

/// Clear all messages from the inbox.
#[no_mangle]
pub extern "C" fn rs_mesh_clear_inbox() -> i32 {
    let mut state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    state.inbox_clear();
    ESP_OK
}

/// Get the local node's public key. Writes 32 bytes to `out`.
#[no_mangle]
pub extern "C" fn rs_mesh_get_self_key(out: *mut u8) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_get_self_pub_key(out) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        // Stub: zero-fill
        unsafe { std::ptr::write_bytes(out, 0, 32); }
        ESP_OK
    }
}

/// Get the local node's name. Returns a pointer to a null-terminated string.
/// The returned pointer is valid until the next call to rs_mesh_init or rs_mesh_deinit.
#[no_mangle]
pub extern "C" fn rs_mesh_get_self_name() -> *const c_char {
    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return EMPTY_NAME.as_ptr() as *const c_char,
    };

    if !state.initialized {
        return EMPTY_NAME.as_ptr() as *const c_char;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_get_self_name() };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        // Stub: return empty string
        EMPTY_NAME.as_ptr() as *const c_char
    }
}

/// Get mesh network statistics.
#[no_mangle]
pub extern "C" fn rs_mesh_get_stats(out: *mut CMeshStats) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let state = match MESH_STATE.lock() {
        Ok(s) => s,
        Err(_) => return ESP_FAIL,
    };

    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    drop(state);

    #[cfg(target_os = "espidf")]
    {
        return unsafe { meshcore_get_stats(out) };
    }

    #[cfg(not(target_os = "espidf"))]
    {
        unsafe { *out = CMeshStats::default(); }
        ESP_OK
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Reset the global state between tests. Must be called at the start of
    /// each test because the Mutex singleton persists across test runs.
    fn reset_state() {
        let mut state = MESH_STATE.lock().unwrap();
        *state = MeshManagerState::new();
    }

    // -- Init / deinit state management ------------------------------------

    #[test]
    fn test_init_success() {
        reset_state();
        let name = CString::new("TestNode").unwrap();
        let rc = rs_mesh_init(name.as_ptr(), 1);
        assert_eq!(rc, ESP_OK, "init must succeed");
        rs_mesh_deinit();
    }

    #[test]
    fn test_init_null_name() {
        reset_state();
        let rc = rs_mesh_init(std::ptr::null(), 0);
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "init with null name must fail");
    }

    #[test]
    fn test_double_init_fails() {
        reset_state();
        let name = CString::new("Node").unwrap();
        assert_eq!(rs_mesh_init(name.as_ptr(), 0), ESP_OK);
        assert_eq!(rs_mesh_init(name.as_ptr(), 0), ESP_ERR_INVALID_STATE,
            "double init must return INVALID_STATE");
        rs_mesh_deinit();
    }

    #[test]
    fn test_deinit_without_init() {
        reset_state();
        let rc = rs_mesh_deinit();
        assert_eq!(rc, ESP_ERR_INVALID_STATE, "deinit without init must fail");
    }

    #[test]
    fn test_init_deinit_reinit() {
        reset_state();
        let name = CString::new("Node").unwrap();
        assert_eq!(rs_mesh_init(name.as_ptr(), 1), ESP_OK);
        assert_eq!(rs_mesh_deinit(), ESP_OK);
        assert_eq!(rs_mesh_init(name.as_ptr(), 2), ESP_OK, "re-init after deinit must succeed");
        rs_mesh_deinit();
    }

    // -- Inbox ring buffer -------------------------------------------------

    #[test]
    fn test_inbox_push_and_read() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        // Push a message directly into state
        {
            let mut state = MESH_STATE.lock().unwrap();
            let mut msg = CMeshMessage::default();
            msg.timestamp = 42;
            msg.text[0] = b'H';
            msg.text[1] = b'i';
            msg.text_len = 2;
            state.inbox_push(msg);
        }

        assert_eq!(rs_mesh_get_inbox_count(), 1);

        let mut out = CMeshMessage::default();
        let rc = rs_mesh_get_inbox_message(0, &mut out as *mut CMeshMessage);
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.timestamp, 42);
        assert_eq!(out.text_len, 2);
        assert_eq!(out.text[0], b'H');
        assert_eq!(out.text[1], b'i');

        rs_mesh_deinit();
    }

    #[test]
    fn test_inbox_overflow_wraps() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        // Push INBOX_SIZE + 5 messages
        {
            let mut state = MESH_STATE.lock().unwrap();
            for i in 0..(INBOX_SIZE + 5) {
                let mut msg = CMeshMessage::default();
                msg.timestamp = i as u32;
                state.inbox_push(msg);
            }
        }

        // Count should be capped at INBOX_SIZE
        assert_eq!(rs_mesh_get_inbox_count(), INBOX_SIZE as i32);

        // Oldest should be message #5 (first 5 were overwritten)
        let mut out = CMeshMessage::default();
        rs_mesh_get_inbox_message(0, &mut out as *mut CMeshMessage);
        assert_eq!(out.timestamp, 5, "oldest message should be #5 after overflow");

        // Newest should be INBOX_SIZE + 4
        rs_mesh_get_inbox_message((INBOX_SIZE - 1) as i32, &mut out as *mut CMeshMessage);
        assert_eq!(out.timestamp, (INBOX_SIZE + 4) as u32, "newest message should be last pushed");

        rs_mesh_deinit();
    }

    #[test]
    fn test_inbox_clear() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        {
            let mut state = MESH_STATE.lock().unwrap();
            for _ in 0..5 {
                state.inbox_push(CMeshMessage::default());
            }
        }

        assert_eq!(rs_mesh_get_inbox_count(), 5);
        assert_eq!(rs_mesh_clear_inbox(), ESP_OK);
        assert_eq!(rs_mesh_get_inbox_count(), 0);

        rs_mesh_deinit();
    }

    #[test]
    fn test_inbox_read_out_of_range() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let mut out = CMeshMessage::default();
        let rc = rs_mesh_get_inbox_message(0, &mut out as *mut CMeshMessage);
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "reading empty inbox must fail");

        let rc = rs_mesh_get_inbox_message(-1, &mut out as *mut CMeshMessage);
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "negative index must fail");

        rs_mesh_deinit();
    }

    #[test]
    fn test_inbox_read_null_out() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let rc = rs_mesh_get_inbox_message(0, std::ptr::null_mut());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null output pointer must fail");

        rs_mesh_deinit();
    }

    // -- Struct size/alignment ---------------------------------------------

    #[test]
    fn test_mesh_contact_size() {
        // Ensure the struct is reasonably sized for FFI
        let size = std::mem::size_of::<CMeshContact>();
        assert!(size > 0, "CMeshContact must have non-zero size");
        assert!(size <= 256, "CMeshContact should not exceed 256 bytes");
    }

    #[test]
    fn test_mesh_message_size() {
        let size = std::mem::size_of::<CMeshMessage>();
        assert!(size > 0, "CMeshMessage must have non-zero size");
        assert!(size <= 512, "CMeshMessage should not exceed 512 bytes");
    }

    #[test]
    fn test_mesh_stats_size() {
        let size = std::mem::size_of::<CMeshStats>();
        assert_eq!(size, 24, "CMeshStats should be 6 x u32 = 24 bytes");
    }

    // -- Null pointer safety on FFI functions -------------------------------

    #[test]
    fn test_send_null_dest() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let text = CString::new("hello").unwrap();
        let rc = rs_mesh_send(std::ptr::null(), text.as_ptr());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null dest_key must fail");

        rs_mesh_deinit();
    }

    #[test]
    fn test_send_null_text() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let key = [0u8; 32];
        let rc = rs_mesh_send(key.as_ptr(), std::ptr::null());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null text must fail");

        rs_mesh_deinit();
    }

    #[test]
    fn test_get_self_key_null() {
        reset_state();
        let rc = rs_mesh_get_self_key(std::ptr::null_mut());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null key output must fail");
    }

    #[test]
    fn test_get_stats_null() {
        reset_state();
        let rc = rs_mesh_get_stats(std::ptr::null_mut());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null stats output must fail");
    }

    #[test]
    fn test_get_contact_null() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let rc = rs_mesh_get_contact(0, std::ptr::null_mut());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "null contact output must fail");

        rs_mesh_deinit();
    }

    #[test]
    fn test_find_contact_null() {
        reset_state();
        let name = CString::new("Node").unwrap();
        rs_mesh_init(name.as_ptr(), 0);

        let rc = rs_mesh_find_contact(std::ptr::null());
        assert_eq!(rc, -1, "null pub_key must return -1");

        rs_mesh_deinit();
    }

    // -- Operations require init -------------------------------------------

    #[test]
    fn test_loop_requires_init() {
        reset_state();
        let rc = rs_mesh_loop();
        assert_eq!(rc, ESP_ERR_INVALID_STATE, "loop without init must fail");
    }

    #[test]
    fn test_send_requires_init() {
        reset_state();
        let key = [0u8; 32];
        let text = CString::new("hi").unwrap();
        let rc = rs_mesh_send(key.as_ptr(), text.as_ptr());
        assert_eq!(rc, ESP_ERR_INVALID_STATE, "send without init must fail");
    }

    #[test]
    fn test_send_advert_requires_init() {
        reset_state();
        assert_eq!(rs_mesh_send_advert(), ESP_ERR_INVALID_STATE);
        assert_eq!(rs_mesh_send_advert_pos(0.0, 0.0), ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_get_self_name_without_init() {
        reset_state();
        let ptr = rs_mesh_get_self_name();
        assert!(!ptr.is_null(), "get_self_name must return non-null even without init");
        let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
        assert_eq!(c_str.to_bytes().len(), 0, "name should be empty string without init");
    }
}
