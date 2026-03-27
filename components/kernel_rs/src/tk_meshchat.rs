// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — MeshCore Chat App (thistle-tk)
//
// A built-in mesh chat application for ThistleOS. Renders two screens:
//   Contacts — list of discovered mesh peers, navigable with Up/Down/Enter
//   Chat     — message history for the selected contact + compose bar
//
// Uses rs_mesh_* FFI for mesh networking and tk_wm_widget_* for UI.
// On simulator / test builds all external functions are stubbed.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use crate::mesh_manager::{CMeshContact, CMeshMessage};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_FAIL: i32 = -1;

// ---------------------------------------------------------------------------
// Capacity constants
// ---------------------------------------------------------------------------

const MAX_CONTACT_WIDGETS: usize = 16;
const MAX_CHAT_WIDGETS: usize = 32;
const ADVERT_INTERVAL: u32 = 30;

// ---------------------------------------------------------------------------
// Screen enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Screen {
    Empty,
    Contacts,
    Chat,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct MeshChatState {
    initialized: bool,
    screen: Screen,
    // Widget IDs
    root: u32,
    status_bar: u32,
    contacts_container: u32,
    chat_container: u32,
    compose_input: u32,
    compose_bar: u32,
    empty_container: u32,
    contact_widgets: [u32; MAX_CONTACT_WIDGETS],
    contact_count: usize,
    chat_widgets: [u32; MAX_CHAT_WIDGETS],
    chat_count: usize,
    // Selection
    selected_idx: i32,
    selected_key: [u8; 32],
    // Timing / polling
    last_contact_count: i32,
    last_inbox_count: i32,
    advert_counter: u32,
}

impl MeshChatState {
    const fn new() -> Self {
        Self {
            initialized: false,
            screen: Screen::Empty,
            root: 0,
            status_bar: 0,
            contacts_container: 0,
            chat_container: 0,
            compose_input: 0,
            compose_bar: 0,
            empty_container: 0,
            contact_widgets: [0u32; MAX_CONTACT_WIDGETS],
            contact_count: 0,
            chat_widgets: [0u32; MAX_CHAT_WIDGETS],
            chat_count: 0,
            selected_idx: -1,
            selected_key: [0u8; 32],
            last_contact_count: -1,
            last_inbox_count: -1,
            advert_counter: 0,
        }
    }
}

static STATE: Mutex<MeshChatState> = Mutex::new(MeshChatState::new());

// ---------------------------------------------------------------------------
// FFI — mesh_manager (real target only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn rs_mesh_init(name: *const c_char, node_type: u8) -> i32;
    fn rs_mesh_deinit() -> i32;
    fn rs_mesh_loop() -> i32;
    fn rs_mesh_send(dest_key: *const u8, text: *const c_char) -> i32;
    fn rs_mesh_send_advert() -> i32;
    fn rs_mesh_get_contact_count() -> i32;
    fn rs_mesh_get_contact(index: i32, out: *mut CMeshContact) -> i32;
    fn rs_mesh_get_inbox_count() -> i32;
    fn rs_mesh_get_inbox_message(index: i32, out: *mut CMeshMessage) -> i32;
    fn rs_mesh_clear_inbox() -> i32;
    fn rs_mesh_get_self_name() -> *const c_char;
}

// ---------------------------------------------------------------------------
// FFI — tk_wm widget API (real target only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn tk_wm_widget_get_app_root() -> u32;
    fn tk_wm_widget_create_container(parent: u32) -> u32;
    fn tk_wm_widget_create_label(parent: u32, text: *const c_char) -> u32;
    fn tk_wm_widget_create_button(parent: u32, text: *const c_char) -> u32;
    fn tk_wm_widget_create_text_input(parent: u32, placeholder: *const c_char) -> u32;
    fn tk_wm_widget_create_list_item(
        parent: u32,
        title: *const c_char,
        subtitle: *const c_char,
    ) -> u32;
    fn tk_wm_widget_create_status_bar(
        parent: u32,
        left: *const c_char,
        center: *const c_char,
        right: *const c_char,
    ) -> u32;
    fn tk_wm_widget_create_divider(parent: u32) -> u32;
    fn tk_wm_widget_create_spacer(parent: u32) -> u32;
    fn tk_wm_widget_create_progress_bar(parent: u32, value: i32) -> u32;
    fn tk_wm_widget_set_text(widget: u32, text: *const c_char);
    fn tk_wm_widget_set_visible(widget: u32, visible: bool);
    fn tk_wm_widget_set_badge(widget: u32, badge: *const c_char);
    fn tk_wm_widget_set_selected(widget: u32, selected: bool);
    fn tk_wm_widget_destroy(widget: u32);
    fn tk_wm_widget_get_text(widget: u32) -> *const c_char;
}

// ---------------------------------------------------------------------------
// Simulator / test stubs
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "espidf"))]
mod stubs {
    use super::*;
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

    pub static NEXT_WIDGET_ID: AtomicU32 = AtomicU32::new(1);
    pub static STUB_CONTACT_COUNT: AtomicI32 = AtomicI32::new(0);
    pub static STUB_INBOX_COUNT: AtomicI32 = AtomicI32::new(0);

    fn next_id() -> u32 {
        NEXT_WIDGET_ID.fetch_add(1, Ordering::SeqCst)
    }

    // Mesh stubs
    pub unsafe fn rs_mesh_init(_name: *const c_char, _node_type: u8) -> i32 { ESP_OK }
    pub unsafe fn rs_mesh_deinit() -> i32 { ESP_OK }
    pub unsafe fn rs_mesh_loop() -> i32 { ESP_OK }
    pub unsafe fn rs_mesh_send(_dest_key: *const u8, _text: *const c_char) -> i32 { ESP_OK }
    pub unsafe fn rs_mesh_send_advert() -> i32 { ESP_OK }
    pub unsafe fn rs_mesh_get_contact_count() -> i32 {
        STUB_CONTACT_COUNT.load(Ordering::SeqCst)
    }
    pub unsafe fn rs_mesh_get_contact(index: i32, out: *mut CMeshContact) -> i32 {
        let count = STUB_CONTACT_COUNT.load(Ordering::SeqCst);
        if index < 0 || index >= count { return ESP_FAIL; }
        let c = out.as_mut().unwrap();
        c.name_len = 4;
        c.name[..4].copy_from_slice(b"Node");
        c.node_type = 0;
        c.last_rssi = -70;
        c.path_len = 1;
        c.last_seen = 100;
        ESP_OK
    }
    pub unsafe fn rs_mesh_get_inbox_count() -> i32 {
        STUB_INBOX_COUNT.load(Ordering::SeqCst)
    }
    pub unsafe fn rs_mesh_get_inbox_message(index: i32, out: *mut CMeshMessage) -> i32 {
        let count = STUB_INBOX_COUNT.load(Ordering::SeqCst);
        if index < 0 || index >= count { return ESP_FAIL; }
        let m = out.as_mut().unwrap();
        m.text_len = 5;
        m.text[..5].copy_from_slice(b"hello");
        m.sender_name_len = 4;
        m.sender_name[..4].copy_from_slice(b"Node");
        m.timestamp = 1000;
        ESP_OK
    }
    pub unsafe fn rs_mesh_clear_inbox() -> i32 {
        STUB_INBOX_COUNT.store(0, Ordering::SeqCst);
        ESP_OK
    }
    pub unsafe fn rs_mesh_get_self_name() -> *const c_char {
        b"ThistleOS\0".as_ptr() as *const c_char
    }

    // Widget stubs
    pub unsafe fn tk_wm_widget_get_app_root() -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_container(_parent: u32) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_label(_parent: u32, _text: *const c_char) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_button(_parent: u32, _text: *const c_char) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_text_input(_parent: u32, _placeholder: *const c_char) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_list_item(
        _parent: u32, _title: *const c_char, _subtitle: *const c_char,
    ) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_status_bar(
        _parent: u32, _left: *const c_char, _center: *const c_char, _right: *const c_char,
    ) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_divider(_parent: u32) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_spacer(_parent: u32) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_create_progress_bar(_parent: u32, _value: i32) -> u32 { next_id() }
    pub unsafe fn tk_wm_widget_set_text(_widget: u32, _text: *const c_char) {}
    pub unsafe fn tk_wm_widget_set_visible(_widget: u32, _visible: bool) {}
    pub unsafe fn tk_wm_widget_set_badge(_widget: u32, _badge: *const c_char) {}
    pub unsafe fn tk_wm_widget_set_selected(_widget: u32, _selected: bool) {}
    pub unsafe fn tk_wm_widget_destroy(_widget: u32) {}
    pub unsafe fn tk_wm_widget_get_text(_widget: u32) -> *const c_char {
        b"\0".as_ptr() as *const c_char
    }
}

#[cfg(not(target_os = "espidf"))]
use stubs::*;

// ---------------------------------------------------------------------------
// Internal helpers — thin safe wrappers around the FFI
// ---------------------------------------------------------------------------

fn mesh_init(name: &str, node_type: u8) -> i32 {
    let cs = CString::new(name).unwrap_or_else(|_| CString::new("ThistleOS").unwrap());
    unsafe { rs_mesh_init(cs.as_ptr(), node_type) }
}

fn mesh_contact_count() -> i32 {
    unsafe { rs_mesh_get_contact_count() }
}

fn mesh_get_contact(index: i32) -> Option<CMeshContact> {
    let mut c = CMeshContact::default();
    let r = unsafe { rs_mesh_get_contact(index, &mut c as *mut _) };
    if r == ESP_OK { Some(c) } else { None }
}

fn mesh_inbox_count() -> i32 {
    unsafe { rs_mesh_get_inbox_count() }
}

fn mesh_get_message(index: i32) -> Option<CMeshMessage> {
    let mut m = CMeshMessage::default();
    let r = unsafe { rs_mesh_get_inbox_message(index, &mut m as *mut _) };
    if r == ESP_OK { Some(m) } else { None }
}

fn mesh_send(dest_key: &[u8; 32], text: &str) -> i32 {
    let cs = CString::new(text).unwrap_or_default();
    unsafe { rs_mesh_send(dest_key.as_ptr(), cs.as_ptr()) }
}

fn contact_name(c: &CMeshContact) -> String {
    let len = (c.name_len as usize).min(32);
    String::from_utf8_lossy(&c.name[..len]).into_owned()
}

fn contact_subtitle(c: &CMeshContact) -> String {
    let ntype = if c.node_type == 0 { "CLIENT" } else { "REPEATER" };
    format!("{} | RSSI {} | hops {}", ntype, c.last_rssi, c.path_len)
}

fn message_text(m: &CMeshMessage) -> String {
    let name_len = (m.sender_name_len as usize).min(32);
    let sender = String::from_utf8_lossy(&m.sender_name[..name_len]);
    let text_len = (m.text_len as usize).min(200);
    let body = String::from_utf8_lossy(&m.text[..text_len]);
    format!("[{}] {}", sender, body)
}

fn widget_set_text(widget: u32, text: &str) {
    let cs = CString::new(text).unwrap_or_default();
    unsafe { tk_wm_widget_set_text(widget, cs.as_ptr()) }
}

fn widget_set_visible(widget: u32, visible: bool) {
    unsafe { tk_wm_widget_set_visible(widget, visible) }
}

fn widget_get_text(widget: u32) -> String {
    let ptr = unsafe { tk_wm_widget_get_text(widget) };
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Screen switching helpers
// ---------------------------------------------------------------------------

fn show_empty(s: &mut MeshChatState) {
    widget_set_visible(s.empty_container, true);
    widget_set_visible(s.contacts_container, false);
    widget_set_visible(s.chat_container, false);
    widget_set_visible(s.compose_bar, false);
    s.screen = Screen::Empty;
}

fn show_contacts(s: &mut MeshChatState) {
    widget_set_visible(s.empty_container, false);
    widget_set_visible(s.contacts_container, true);
    widget_set_visible(s.chat_container, false);
    widget_set_visible(s.compose_bar, false);
    s.screen = Screen::Contacts;
}

fn show_chat(s: &mut MeshChatState) {
    widget_set_visible(s.empty_container, false);
    widget_set_visible(s.contacts_container, false);
    widget_set_visible(s.chat_container, true);
    widget_set_visible(s.compose_bar, true);
    s.screen = Screen::Chat;
}

// ---------------------------------------------------------------------------
// Contact list rebuild
// ---------------------------------------------------------------------------

fn rebuild_contacts(s: &mut MeshChatState) {
    // Destroy old widgets
    for i in 0..s.contact_count {
        if s.contact_widgets[i] != 0 {
            unsafe { tk_wm_widget_destroy(s.contact_widgets[i]) };
            s.contact_widgets[i] = 0;
        }
    }
    s.contact_count = 0;

    let n = mesh_contact_count();
    if n <= 0 {
        return;
    }
    let count = (n as usize).min(MAX_CONTACT_WIDGETS);
    for i in 0..count {
        if let Some(c) = mesh_get_contact(i as i32) {
            let title_cs = CString::new(contact_name(&c)).unwrap_or_default();
            let sub_cs = CString::new(contact_subtitle(&c)).unwrap_or_default();
            let wid = unsafe {
                tk_wm_widget_create_list_item(
                    s.contacts_container,
                    title_cs.as_ptr(),
                    sub_cs.as_ptr(),
                )
            };
            if i as i32 == s.selected_idx {
                unsafe { tk_wm_widget_set_selected(wid, true) };
            }
            s.contact_widgets[i] = wid;
            s.contact_count += 1;
        }
    }
    s.last_contact_count = n;
}

// ---------------------------------------------------------------------------
// Chat inbox append
// ---------------------------------------------------------------------------

fn append_inbox_messages(s: &mut MeshChatState) {
    let n = mesh_inbox_count();
    if n <= 0 {
        return;
    }
    for i in 0..n {
        if s.chat_count >= MAX_CHAT_WIDGETS {
            break;
        }
        if let Some(m) = mesh_get_message(i) {
            let text = message_text(&m);
            // Only show messages from/to the selected contact (or if no contact filter)
            let relevant = s.selected_key == [0u8; 32] || m.sender_key == s.selected_key;
            if !relevant {
                continue;
            }
            let cs = CString::new(text).unwrap_or_default();
            let wid = unsafe { tk_wm_widget_create_label(s.chat_container, cs.as_ptr()) };
            let idx = s.chat_count;
            s.chat_widgets[idx] = wid;
            s.chat_count += 1;
        }
    }
    unsafe { rs_mesh_clear_inbox() };
    s.last_inbox_count = 0;
}

// ---------------------------------------------------------------------------
// Update status bar text
// ---------------------------------------------------------------------------

fn refresh_status_bar(s: &MeshChatState) {
    let self_name = unsafe {
        let ptr = rs_mesh_get_self_name();
        if ptr.is_null() {
            "Mesh".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };
    let contact_count = mesh_contact_count();
    let right = format!("{} peers", contact_count);
    let center = match s.screen {
        Screen::Empty => "Scanning...",
        Screen::Contacts => "Contacts",
        Screen::Chat => "Chat",
    };
    widget_set_text(s.status_bar, &format!("{} | {} | {}", self_name, center, right));
}

// ---------------------------------------------------------------------------
// Public FFI exports
// ---------------------------------------------------------------------------

/// Initialise the MeshChat app: build UI structure, start mesh.
#[no_mangle]
pub extern "C" fn rs_meshchat_init() -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if s.initialized {
        return ESP_OK;
    }

    // Start mesh
    let rc = mesh_init("ThistleOS", 0);
    if rc != ESP_OK {
        return rc;
    }

    // Build widget tree
    let root = unsafe { tk_wm_widget_get_app_root() };
    s.root = root;

    let sb_left = CString::new("ThistleOS").unwrap_or_default();
    let sb_center = CString::new("MeshChat").unwrap_or_default();
    let sb_right = CString::new("0 peers").unwrap_or_default();
    s.status_bar = unsafe {
        tk_wm_widget_create_status_bar(
            root,
            sb_left.as_ptr(),
            sb_center.as_ptr(),
            sb_right.as_ptr(),
        )
    };

    // Empty screen — shown until first peer appears
    s.empty_container = unsafe { tk_wm_widget_create_container(root) };
    let scanning_cs = CString::new("Scanning for peers...").unwrap_or_default();
    unsafe { tk_wm_widget_create_label(s.empty_container, scanning_cs.as_ptr()) };

    // Contacts screen (hidden initially)
    s.contacts_container = unsafe { tk_wm_widget_create_container(root) };

    // Chat screen (hidden initially)
    s.chat_container = unsafe { tk_wm_widget_create_container(root) };

    // Compose bar (hidden initially)
    s.compose_bar = unsafe { tk_wm_widget_create_container(root) };
    let placeholder_cs = CString::new("Type a message...").unwrap_or_default();
    s.compose_input =
        unsafe { tk_wm_widget_create_text_input(s.compose_bar, placeholder_cs.as_ptr()) };
    let send_cs = CString::new("Send").unwrap_or_default();
    unsafe { tk_wm_widget_create_button(s.compose_bar, send_cs.as_ptr()) };

    show_empty(&mut s);

    // Send first advertisement
    unsafe { rs_mesh_send_advert() };

    s.initialized = true;
    ESP_OK
}

/// Tear down the MeshChat app and release all resources.
#[no_mangle]
pub extern "C" fn rs_meshchat_deinit() -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if !s.initialized {
        return ESP_OK;
    }
    let rc = unsafe { rs_mesh_deinit() };
    // Destroy all tracked widgets
    for i in 0..s.contact_count {
        if s.contact_widgets[i] != 0 {
            unsafe { tk_wm_widget_destroy(s.contact_widgets[i]) };
            s.contact_widgets[i] = 0;
        }
    }
    for i in 0..s.chat_count {
        if s.chat_widgets[i] != 0 {
            unsafe { tk_wm_widget_destroy(s.chat_widgets[i]) };
            s.chat_widgets[i] = 0;
        }
    }
    *s = MeshChatState::new();
    rc
}

/// Per-frame update: poll mesh stack and refresh UI. Called each render tick.
#[no_mangle]
pub extern "C" fn rs_meshchat_update() -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Drive mesh stack
    unsafe { rs_mesh_loop() };

    // Periodic advertisement
    s.advert_counter = s.advert_counter.wrapping_add(1);
    if s.advert_counter >= ADVERT_INTERVAL {
        s.advert_counter = 0;
        unsafe { rs_mesh_send_advert() };
    }

    // Check for new / removed contacts
    let new_count = mesh_contact_count();
    if new_count != s.last_contact_count {
        rebuild_contacts(&mut s);
        if new_count > 0 && s.screen == Screen::Empty {
            show_contacts(&mut s);
        } else if new_count == 0 && s.screen == Screen::Contacts {
            show_empty(&mut s);
        }
    }

    // Check inbox
    let new_inbox = mesh_inbox_count();
    if new_inbox > 0 && s.screen == Screen::Chat {
        append_inbox_messages(&mut s);
    } else if new_inbox > 0 {
        // Badge the contacts container to signal unread messages
        let badge_cs = CString::new("!").unwrap_or_default();
        unsafe { tk_wm_widget_set_badge(s.contacts_container, badge_cs.as_ptr()) };
    }

    refresh_status_bar(&s);
    ESP_OK
}

/// Handle keyboard input.
///
/// Key codes match the ThistleOS virtual key table:
///   0x26 = Up, 0x28 = Down, 0x0D = Enter, 0x1B = Escape, 0x08 = Backspace
#[no_mangle]
pub extern "C" fn rs_meshchat_on_key(key: u16) -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    const KEY_UP: u16 = 0x26;
    const KEY_DOWN: u16 = 0x28;
    const KEY_ENTER: u16 = 0x0D;
    const KEY_ESCAPE: u16 = 0x1B;
    const KEY_BACKSPACE: u16 = 0x08;

    match s.screen {
        Screen::Empty => {
            // No interactive elements on the empty screen
        }

        Screen::Contacts => {
            let max = s.contact_count as i32;
            match key {
                KEY_UP => {
                    if max > 0 {
                        if s.selected_idx >= 0 && (s.selected_idx as usize) < s.contact_count {
                            let old = s.selected_idx as usize;
                            unsafe { tk_wm_widget_set_selected(s.contact_widgets[old], false) };
                        }
                        s.selected_idx =
                            if s.selected_idx <= 0 { max - 1 } else { s.selected_idx - 1 };
                        let new_idx = s.selected_idx as usize;
                        unsafe { tk_wm_widget_set_selected(s.contact_widgets[new_idx], true) };
                    }
                }
                KEY_DOWN => {
                    if max > 0 {
                        if s.selected_idx >= 0 && (s.selected_idx as usize) < s.contact_count {
                            let old = s.selected_idx as usize;
                            unsafe { tk_wm_widget_set_selected(s.contact_widgets[old], false) };
                        }
                        s.selected_idx =
                            if s.selected_idx < 0 || s.selected_idx >= max - 1 {
                                0
                            } else {
                                s.selected_idx + 1
                            };
                        let new_idx = s.selected_idx as usize;
                        unsafe { tk_wm_widget_set_selected(s.contact_widgets[new_idx], true) };
                    }
                }
                KEY_ENTER => {
                    if s.selected_idx >= 0 && (s.selected_idx as usize) < s.contact_count {
                        if let Some(c) = mesh_get_contact(s.selected_idx) {
                            s.selected_key = c.pub_key;
                        }
                        // Clear previous chat widgets
                        for i in 0..s.chat_count {
                            if s.chat_widgets[i] != 0 {
                                unsafe { tk_wm_widget_destroy(s.chat_widgets[i]) };
                                s.chat_widgets[i] = 0;
                            }
                        }
                        s.chat_count = 0;
                        // Drain inbox into chat view
                        append_inbox_messages(&mut s);
                        show_chat(&mut s);
                    }
                }
                _ => {}
            }
        }

        Screen::Chat => {
            match key {
                KEY_ESCAPE | KEY_BACKSPACE => {
                    show_contacts(&mut s);
                }
                KEY_ENTER => {
                    // Release lock before re-entrant send call
                    drop(s);
                    return rs_meshchat_send();
                }
                c if c >= 0x20 && c < 0x7F => {
                    // Printable ASCII — append to compose input
                    let current = widget_get_text(s.compose_input);
                    let new_text = format!("{}{}", current, c as u8 as char);
                    widget_set_text(s.compose_input, &new_text);
                }
                _ => {}
            }
        }
    }

    ESP_OK
}

/// Send the composed message to the currently selected contact.
#[no_mangle]
pub extern "C" fn rs_meshchat_send() -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if s.selected_key == [0u8; 32] {
        return ESP_ERR_NOT_FOUND;
    }
    let text = widget_get_text(s.compose_input);
    if text.is_empty() {
        return ESP_OK;
    }

    let rc = mesh_send(&s.selected_key, &text);
    if rc == ESP_OK {
        // Echo sent message into chat view
        let display = format!("[Me] {}", text);
        if s.chat_count < MAX_CHAT_WIDGETS {
            let cs = CString::new(display).unwrap_or_default();
            let wid = unsafe { tk_wm_widget_create_label(s.chat_container, cs.as_ptr()) };
            let idx = s.chat_count;
            s.chat_widgets[idx] = wid;
            s.chat_count += 1;
        }
        // Clear compose input
        let empty_cs = CString::new("").unwrap_or_default();
        unsafe { tk_wm_widget_set_text(s.compose_input, empty_cs.as_ptr()) };
    }
    rc
}

/// Return to the contacts screen from the chat screen.
#[no_mangle]
pub extern "C" fn rs_meshchat_back() -> i32 {
    let mut s = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return ESP_FAIL,
    };
    if !s.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if s.screen == Screen::Chat {
        show_contacts(&mut s);
    }
    ESP_OK
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use stubs::{NEXT_WIDGET_ID, STUB_CONTACT_COUNT, STUB_INBOX_COUNT};
    use std::sync::atomic::Ordering;

    fn reset() {
        let mut s = STATE.lock().unwrap();
        *s = MeshChatState::new();
        NEXT_WIDGET_ID.store(1, Ordering::SeqCst);
        STUB_CONTACT_COUNT.store(0, Ordering::SeqCst);
        STUB_INBOX_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn test_init_succeeds() {
        reset();
        assert_eq!(rs_meshchat_init(), ESP_OK);
        let s = STATE.lock().unwrap();
        assert!(s.initialized);
    }

    #[test]
    fn test_double_init_is_noop() {
        reset();
        assert_eq!(rs_meshchat_init(), ESP_OK);
        assert_eq!(rs_meshchat_init(), ESP_OK);
    }

    #[test]
    fn test_deinit_resets_state() {
        reset();
        rs_meshchat_init();
        assert_eq!(rs_meshchat_deinit(), ESP_OK);
        let s = STATE.lock().unwrap();
        assert!(!s.initialized);
    }

    #[test]
    fn test_deinit_without_init_is_noop() {
        reset();
        assert_eq!(rs_meshchat_deinit(), ESP_OK);
    }

    #[test]
    fn test_update_without_init_returns_invalid_state() {
        reset();
        assert_eq!(rs_meshchat_update(), ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_update_after_init_returns_ok() {
        reset();
        rs_meshchat_init();
        assert_eq!(rs_meshchat_update(), ESP_OK);
    }

    #[test]
    fn test_starts_on_empty_screen() {
        reset();
        rs_meshchat_init();
        let s = STATE.lock().unwrap();
        assert_eq!(s.screen, Screen::Empty);
    }

    #[test]
    fn test_transitions_to_contacts_when_peer_appears() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(1, Ordering::SeqCst);
        rs_meshchat_update();
        let s = STATE.lock().unwrap();
        assert_eq!(s.screen, Screen::Contacts);
    }

    #[test]
    fn test_contact_list_rebuild_populates_widgets() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(3, Ordering::SeqCst);
        rs_meshchat_update();
        let s = STATE.lock().unwrap();
        assert_eq!(s.contact_count, 3);
        assert!(s.contact_widgets[0] != 0);
        assert!(s.contact_widgets[1] != 0);
        assert!(s.contact_widgets[2] != 0);
    }

    #[test]
    fn test_key_down_selects_first_contact() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(2, Ordering::SeqCst);
        rs_meshchat_update();
        rs_meshchat_on_key(0x28); // Down
        let s = STATE.lock().unwrap();
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn test_key_up_wraps_to_last_contact() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(3, Ordering::SeqCst);
        rs_meshchat_update();
        // selected_idx starts at -1; Up should wrap to last (index 2)
        rs_meshchat_on_key(0x26); // Up
        let s = STATE.lock().unwrap();
        assert_eq!(s.selected_idx, 2);
    }

    #[test]
    fn test_enter_on_contact_opens_chat() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(1, Ordering::SeqCst);
        rs_meshchat_update();
        rs_meshchat_on_key(0x28); // Down — selects index 0
        rs_meshchat_on_key(0x0D); // Enter
        let s = STATE.lock().unwrap();
        assert_eq!(s.screen, Screen::Chat);
    }

    #[test]
    fn test_escape_from_chat_returns_to_contacts() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(1, Ordering::SeqCst);
        rs_meshchat_update();
        rs_meshchat_on_key(0x28);
        rs_meshchat_on_key(0x0D);
        rs_meshchat_on_key(0x1B); // Escape
        let s = STATE.lock().unwrap();
        assert_eq!(s.screen, Screen::Contacts);
    }

    #[test]
    fn test_back_fn_from_chat_returns_contacts() {
        reset();
        rs_meshchat_init();
        STUB_CONTACT_COUNT.store(1, Ordering::SeqCst);
        rs_meshchat_update();
        rs_meshchat_on_key(0x28);
        rs_meshchat_on_key(0x0D);
        assert_eq!(rs_meshchat_back(), ESP_OK);
        let s = STATE.lock().unwrap();
        assert_eq!(s.screen, Screen::Contacts);
    }

    #[test]
    fn test_send_with_no_selected_contact_fails() {
        reset();
        rs_meshchat_init();
        // selected_key is all zeros — should return NOT_FOUND
        assert_eq!(rs_meshchat_send(), ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_widget_id_counter_increments() {
        reset();
        let id1 = NEXT_WIDGET_ID.fetch_add(1, Ordering::SeqCst);
        let id2 = NEXT_WIDGET_ID.fetch_add(1, Ordering::SeqCst);
        assert_eq!(id2, id1 + 1);
    }
}
