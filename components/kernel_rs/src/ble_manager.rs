// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — ble_manager module
//
// Port of components/kernel/src/ble_manager.c
// Manages ESP-IDF NimBLE: Nordic UART Service GATT server, advertising,
// TX notifications, and RX callbacks.
// On simulator builds all functions are stubs.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_FAIL: i32 = -1;

// BLE state constants — must match ble_manager.h
const BLE_STATE_OFF: u32 = 0;
const BLE_STATE_ADVERTISING: u32 = 1;
const BLE_STATE_CONNECTED: u32 = 2;

const BLE_DEVICE_NAME_MAX: usize = 32;

static TAG: &[u8] = b"ble_mgr\0";

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
// NimBLE FFI (hardware only)
// ---------------------------------------------------------------------------

// Direct NimBLE FFI — replaces ble_shim_* wrappers formerly in kernel_shims.c
#[cfg(target_os = "espidf")]
extern "C" {
    fn nimble_port_init() -> i32;
    fn nimble_port_freertos_init(task_fn: *const c_void);
    fn nimble_port_run();
    fn nimble_port_freertos_deinit();
    fn ble_svc_gap_init();
    fn ble_svc_gatt_init();
    fn ble_svc_gap_device_name_set(name: *const c_char) -> i32;
    fn ble_gap_adv_stop() -> i32;
    fn ble_gap_terminate(conn_handle: u16, hci_reason: u8) -> i32;
    fn ble_gatts_notify_custom(conn_handle: u16, attr_handle: u16, om: *mut c_void) -> i32;
    fn ble_hs_mbuf_from_flat(buf: *const u8, len: u16) -> *mut c_void;
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

type BleRxCb = unsafe extern "C" fn(data: *const u8, len: u16, user_data: *mut c_void);

struct BleState {
    device_name: [u8; BLE_DEVICE_NAME_MAX + 1],
    state: u32,
    conn_handle: u16,
    tx_attr_handle: u16,
    rx_cb: Option<BleRxCb>,
    rx_cb_data: *mut c_void,
    initialized: bool,
}

// SAFETY: Only accessed under Mutex.
unsafe impl Send for BleState {}

impl BleState {
    const fn new() -> Self {
        BleState {
            device_name: [0u8; BLE_DEVICE_NAME_MAX + 1],
            state: BLE_STATE_OFF,
            conn_handle: 0xFFFF, // BLE_HS_CONN_HANDLE_NONE
            tx_attr_handle: 0,
            rx_cb: None,
            rx_cb_data: std::ptr::null_mut(),
            initialized: false,
        }
    }
}

static BLE_STATE: Mutex<BleState> = Mutex::new(BleState::new());

// ---------------------------------------------------------------------------
// NimBLE host task (hardware only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
unsafe extern "C" fn ble_host_task(_param: *mut c_void) {
    nimble_port_run();
    nimble_port_freertos_deinit();
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the BLE manager and start the NimBLE host.
///
/// On simulator, returns ESP_ERR_NOT_SUPPORTED.
///
/// # Safety
/// `device_name` may be NULL (falls back to "ThistleOS").
#[no_mangle]
pub unsafe extern "C" fn ble_manager_init(device_name: *const c_char) -> i32 {
    let already_init = BLE_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if already_init {
        return ESP_OK;
    }

    let name_str = if device_name.is_null() {
        "ThistleOS"
    } else {
        match CStr::from_ptr(device_name).to_str() {
            Ok(s) => s,
            Err(_) => "ThistleOS",
        }
    };

    if let Ok(mut state) = BLE_STATE.lock() {
        let name_bytes = name_str.as_bytes();
        let len = name_bytes.len().min(BLE_DEVICE_NAME_MAX);
        state.device_name[..len].copy_from_slice(&name_bytes[..len]);
        state.device_name[len] = 0;
        state.state = BLE_STATE_OFF;
        state.conn_handle = 0xFFFF;
    }

    #[cfg(target_os = "espidf")]
    {
        let ret = nimble_port_init();
        if ret != ESP_OK {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"nimble_port_init failed: %d\0".as_ptr(), ret);
            return ret;
        }

        ble_svc_gap_init();
        ble_svc_gatt_init();

        // Register the GATT services via C helper (avoids duplicating service definitions)
        extern "C" {
            fn ble_manager_register_gatt_services() -> i32;
        }
        let rc = ble_manager_register_gatt_services();
        if rc != 0 {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"GATT service registration failed: %d\0".as_ptr(), rc);
            return ESP_FAIL;
        }

        // Set device name
        let name_cstr = {
            let guard = BLE_STATE.lock().unwrap();
            guard.device_name.as_ptr() as *const c_char
        };
        ble_svc_gap_device_name_set(name_cstr);

        nimble_port_freertos_init(ble_host_task as *const c_void);
    }

    if let Ok(mut state) = BLE_STATE.lock() {
        state.initialized = true;
    }

    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"BLE manager initialized: '%s'\0".as_ptr(),
        name_str.as_ptr(),
    );

    #[cfg(not(target_os = "espidf"))]
    {
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"BLE: simulator stub\0".as_ptr());
    }

    ESP_OK
}

/// Start BLE advertising.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ble_manager_start_advertising() -> i32 {
    let initialized = BLE_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        extern "C" {
            fn ble_manager_do_advertise() -> i32;
        }
        let rc = ble_manager_do_advertise();
        if rc != 0 {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gap_adv_start failed: %d\0".as_ptr(), rc);
            return ESP_FAIL;
        }
    }

    if let Ok(mut state) = BLE_STATE.lock() {
        state.state = BLE_STATE_ADVERTISING;
    }

    unsafe {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"BLE advertising started\0".as_ptr());
    }

    #[cfg(not(target_os = "espidf"))]
    {
        unsafe {
            esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"BLE advertise: simulator stub\0".as_ptr());
        }
    }

    ESP_OK
}

/// Stop BLE advertising.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ble_manager_stop_advertising() -> i32 {
    let initialized = BLE_STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        ble_gap_adv_stop();
    }

    if let Ok(mut state) = BLE_STATE.lock() {
        state.state = BLE_STATE_OFF;
    }

    ESP_OK
}

/// Disconnect the current BLE peer.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn ble_manager_disconnect() -> i32 {
    let (state_val, conn_handle) = BLE_STATE
        .lock()
        .map(|s| (s.state, s.conn_handle))
        .unwrap_or((BLE_STATE_OFF, 0xFFFF));

    if state_val != BLE_STATE_CONNECTED {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    unsafe {
        ble_gap_terminate(conn_handle, 0x13 /* BLE_ERR_REM_USER_CONN_TERM */);
    }

    #[cfg(not(target_os = "espidf"))]
    let _ = conn_handle;

    ESP_OK
}

/// Return true if a BLE peer is currently connected.
#[no_mangle]
pub extern "C" fn ble_manager_is_connected() -> bool {
    BLE_STATE
        .lock()
        .map(|s| s.state == BLE_STATE_CONNECTED)
        .unwrap_or(false)
}

/// Return the current BLE state (matches ble_state_t enum).
#[no_mangle]
pub extern "C" fn ble_manager_get_state() -> u32 {
    BLE_STATE.lock().map(|s| s.state).unwrap_or(BLE_STATE_OFF)
}

/// Send data via the TX NOTIFY characteristic.
///
/// # Safety
/// `data` must point to at least `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn ble_manager_send(data: *const u8, len: usize) -> i32 {
    if data.is_null() || len == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let (state_val, conn_handle, tx_handle) = BLE_STATE
        .lock()
        .map(|s| (s.state, s.conn_handle, s.tx_attr_handle))
        .unwrap_or((BLE_STATE_OFF, 0, 0));

    if state_val != BLE_STATE_CONNECTED {
        return ESP_ERR_INVALID_STATE;
    }

    #[cfg(target_os = "espidf")]
    {
        let om = ble_hs_mbuf_from_flat(data, len as u16);
        if om.is_null() {
            return ESP_ERR_INVALID_SIZE; // ESP_ERR_NO_MEM
        }

        let rc = ble_gatts_notify_custom(conn_handle, tx_handle, om);
        if rc != 0 {
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gatts_notify_custom failed: %d\0".as_ptr(), rc);
            return ESP_FAIL;
        }
        return ESP_OK;
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (conn_handle, tx_handle);
        esp_log_write(ESP_LOG_WARN, TAG.as_ptr(), b"BLE send: simulator stub\0".as_ptr());
        ESP_ERR_NOT_SUPPORTED
    }
}

/// Send a "NOTIF:title\nbody" notification over BLE.
///
/// # Safety
/// `title` and `body` must be valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn ble_manager_send_notification(
    title: *const c_char,
    body: *const c_char,
) -> i32 {
    if title.is_null() || body.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let title_str = CStr::from_ptr(title).to_str().unwrap_or("");
    let body_str  = CStr::from_ptr(body).to_str().unwrap_or("");

    let msg = format!("NOTIF:{}\n{}", title_str, body_str);
    if msg.len() >= 256 {
        return ESP_ERR_INVALID_SIZE;
    }

    ble_manager_send(msg.as_ptr(), msg.len())
}

/// Register a callback to receive data written to the RX characteristic.
///
/// # Safety
/// `cb` and `user_data` lifetime must outlast the BLE connection.
#[no_mangle]
pub unsafe extern "C" fn ble_manager_register_rx_cb(
    cb: Option<BleRxCb>,
    user_data: *mut c_void,
) -> i32 {
    if let Ok(mut state) = BLE_STATE.lock() {
        state.rx_cb = cb;
        state.rx_cb_data = user_data;
    }
    ESP_OK
}

/// Return the peer device name, or NULL if not connected.
///
/// # Safety
/// Returns a pointer to a static string. Do not free.
#[no_mangle]
pub extern "C" fn ble_manager_get_peer_name() -> *const c_char {
    let connected = BLE_STATE
        .lock()
        .map(|s| s.state == BLE_STATE_CONNECTED)
        .unwrap_or(false);

    if connected {
        b"Companion\0".as_ptr() as *const c_char
    } else {
        std::ptr::null()
    }
}

// ---------------------------------------------------------------------------
// Internal state updates — called from the C NimBLE event shim
// ---------------------------------------------------------------------------

/// Update BLE state and connection handle from the C GAP event shim.
///
/// # Safety
/// May be called from C interrupt/callback context.
#[no_mangle]
pub unsafe extern "C" fn ble_manager_set_conn_state(new_state: u32, conn_handle: u16) {
    if let Ok(mut state) = BLE_STATE.lock() {
        state.state = new_state;
        state.conn_handle = conn_handle;
    }
}

/// Deliver received RX data to the registered callback.
///
/// # Safety
/// `data` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn ble_manager_rx_dispatch(data: *const u8, len: u16) {
    let (cb, user_data) = BLE_STATE
        .lock()
        .map(|s| (s.rx_cb, s.rx_cb_data))
        .unwrap_or((None, std::ptr::null_mut()));

    if let Some(callback) = cb {
        callback(data, len, user_data);
    }
}
