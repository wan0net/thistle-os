// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — net_manager module
//
// Port of components/kernel/src/net_manager.c
// Transport-agnostic network manager. Each transport (WiFi, 4G, …) registers
// a hal_net_driver_t vtable pointer. Apps call net_is_connected() etc.
//
// The built-in WiFi transport wrapper is included here so that
// wifi_manager.rs needs no modification.

use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_FAIL: i32 = -1;

// hal_net_state_t values — must match hal/net.h
const HAL_NET_STATE_DISCONNECTED: u32 = 0;
const HAL_NET_STATE_CONNECTING: u32 = 1;
const HAL_NET_STATE_CONNECTED: u32 = 2;

// hal_net_type_t values — must match hal/net.h
const HAL_NET_WIFI: u32 = 0;

const MAX_NET_TRANSPORTS: usize = 4;

static TAG: &[u8] = b"net_mgr\0";

// ---------------------------------------------------------------------------
// Logging FFI
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO:  i32 = 3;
const ESP_LOG_WARN:  i32 = 2;

// ---------------------------------------------------------------------------
// hal_net_driver_t vtable — mirrors the C struct exactly.
// All function-pointer fields are optional (may be NULL).
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct HalNetDriver {
    pub transport_type: u32,
    pub name: *const c_char,
    pub init: Option<unsafe extern "C" fn() -> i32>,
    pub connect: Option<unsafe extern "C" fn(*const c_char, *const c_char, u32) -> i32>,
    pub disconnect: Option<unsafe extern "C" fn() -> i32>,
    pub get_state: Option<unsafe extern "C" fn() -> u32>,
    pub get_ip: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_rssi: Option<unsafe extern "C" fn() -> i8>,
    pub is_connected: Option<unsafe extern "C" fn() -> bool>,
}

// SAFETY: All fields are immutable once registered.
unsafe impl Send for HalNetDriver {}
unsafe impl Sync for HalNetDriver {}

// ---------------------------------------------------------------------------
// WiFi manager FFI
// ---------------------------------------------------------------------------

extern "C" {
    fn wifi_manager_init() -> i32;
    fn wifi_manager_connect(ssid: *const c_char, password: *const c_char, timeout_ms: u32) -> i32;
    fn wifi_manager_disconnect() -> i32;
    fn wifi_manager_get_state() -> u32;
    fn wifi_manager_get_ip() -> *const c_char;
    fn wifi_manager_get_rssi() -> i8;
    fn wifi_manager_ntp_sync() -> i32;
}

// ---------------------------------------------------------------------------
// Built-in WiFi transport wrapper
// ---------------------------------------------------------------------------

// WiFi state constants (match wifi_manager.h)
const WIFI_STATE_CONNECTED: u32  = 2;
const WIFI_STATE_CONNECTING: u32 = 1;

unsafe extern "C" fn wifi_net_get_state() -> u32 {
    let ws = wifi_manager_get_state();
    if ws == WIFI_STATE_CONNECTED  { return HAL_NET_STATE_CONNECTED; }
    if ws == WIFI_STATE_CONNECTING { return HAL_NET_STATE_CONNECTING; }
    HAL_NET_STATE_DISCONNECTED
}

unsafe extern "C" fn wifi_net_is_connected() -> bool {
    wifi_manager_get_state() == WIFI_STATE_CONNECTED
}

static WIFI_NET_DRIVER: HalNetDriver = HalNetDriver {
    transport_type: HAL_NET_WIFI,
    name: b"WiFi\0".as_ptr() as *const c_char,
    init: Some(wifi_manager_init),
    connect: Some(wifi_manager_connect),
    disconnect: Some(wifi_manager_disconnect),
    get_state: Some(wifi_net_get_state),
    get_ip: Some(wifi_manager_get_ip),
    get_rssi: Some(wifi_manager_get_rssi),
    is_connected: Some(wifi_net_is_connected),
};

// ---------------------------------------------------------------------------
// Manager state
// ---------------------------------------------------------------------------

struct NetManagerState {
    transports: [*const HalNetDriver; MAX_NET_TRANSPORTS],
    count: usize,
    initialized: bool,
}

impl NetManagerState {
    const fn new() -> Self {
        NetManagerState {
            transports: [std::ptr::null(); MAX_NET_TRANSPORTS],
            count: 0,
            initialized: false,
        }
    }
}

// SAFETY: Only accessed under Mutex.
unsafe impl Send for NetManagerState {}

static STATE: Mutex<NetManagerState> = Mutex::new(NetManagerState::new());

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the network manager.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn net_manager_init() -> i32 {
    if let Ok(mut state) = STATE.lock() {
        state.transports = [std::ptr::null(); MAX_NET_TRANSPORTS];
        state.count = 0;
        state.initialized = true;
    }

    unsafe {
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"Network manager initialized\0".as_ptr());
    }

    ESP_OK
}

/// Register a network transport vtable.
///
/// # Safety
/// `driver` must point to a valid HalNetDriver struct with a stable lifetime.
#[no_mangle]
pub unsafe extern "C" fn net_manager_register(driver: *const HalNetDriver) -> i32 {
    if driver.is_null() {
        return ESP_ERR_INVALID_STATE;
    }

    let initialized = STATE.lock().map(|s| s.initialized).unwrap_or(false);
    if !initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if let Ok(mut state) = STATE.lock() {
        if state.count >= MAX_NET_TRANSPORTS {
            return ESP_ERR_NO_MEM;
        }
        let idx = state.count;
        state.transports[idx] = driver;
        state.count = idx + 1;

        let name = if (*driver).name.is_null() {
            b"?\0".as_ptr() as *const c_char
        } else {
            (*driver).name
        };

        esp_log_write(
            ESP_LOG_INFO,
            TAG.as_ptr(),
            b"Registered transport: %s\0".as_ptr(),
            name,
        );
    }

    ESP_OK
}

/// Return true if any registered transport is connected.
#[no_mangle]
pub extern "C" fn net_is_connected() -> bool {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };

    for i in 0..state.count {
        let drv = state.transports[i];
        if drv.is_null() { continue; }
        unsafe {
            if let Some(f) = (*drv).is_connected {
                if f() { return true; }
            }
        }
    }

    false
}

/// Return a pointer to the first connected transport vtable, or NULL.
///
/// # Safety
/// Returns a pointer to static vtable data. Do not free.
#[no_mangle]
pub extern "C" fn net_get_active() -> *const HalNetDriver {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return std::ptr::null(),
    };

    for i in 0..state.count {
        let drv = state.transports[i];
        if drv.is_null() { continue; }
        unsafe {
            if let Some(f) = (*drv).is_connected {
                if f() { return drv; }
            }
        }
    }

    std::ptr::null()
}

/// Return the best current network state across all transports.
#[no_mangle]
pub extern "C" fn net_get_state() -> u32 {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return HAL_NET_STATE_DISCONNECTED,
    };

    let mut best = HAL_NET_STATE_DISCONNECTED;

    for i in 0..state.count {
        let drv = state.transports[i];
        if drv.is_null() { continue; }
        unsafe {
            if let Some(f) = (*drv).get_state {
                let st = f();
                if st == HAL_NET_STATE_CONNECTED  { return HAL_NET_STATE_CONNECTED; }
                if st == HAL_NET_STATE_CONNECTING { best = HAL_NET_STATE_CONNECTING; }
            }
        }
    }

    best
}

/// Return the IP address from the first connected transport, or NULL.
///
/// # Safety
/// Returns a pointer from the underlying transport driver. Do not free.
#[no_mangle]
pub extern "C" fn net_get_ip() -> *const c_char {
    let active = net_get_active();
    if active.is_null() {
        return std::ptr::null();
    }
    unsafe {
        if let Some(f) = (*active).get_ip {
            return f();
        }
    }
    std::ptr::null()
}

/// Return the RSSI of the active transport, or 0.
#[no_mangle]
pub extern "C" fn net_get_rssi() -> i8 {
    let active = net_get_active();
    if active.is_null() {
        return 0;
    }
    unsafe {
        if let Some(f) = (*active).get_rssi {
            return f();
        }
    }
    0
}

/// Return the name of the active transport, or "None".
///
/// # Safety
/// Returns a pointer to static string data. Do not free.
#[no_mangle]
pub extern "C" fn net_get_transport_name() -> *const c_char {
    let active = net_get_active();
    if active.is_null() {
        return b"None\0".as_ptr() as *const c_char;
    }
    unsafe {
        if !(*active).name.is_null() {
            return (*active).name;
        }
    }
    b"None\0".as_ptr() as *const c_char
}

/// Try to connect using the first available transport.
///
/// `timeout_ms` is passed through to the transport's connect function.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub unsafe extern "C" fn net_connect_best(timeout_ms: u32) -> i32 {
    let count = STATE.lock().map(|s| s.count).unwrap_or(0);

    for i in 0..count {
        let drv = STATE.lock().map(|s| s.transports[i]).unwrap_or(std::ptr::null());
        if drv.is_null() { continue; }

        if let Some(is_conn) = (*drv).is_connected {
            if is_conn() { return ESP_OK; }
        }

        if let Some(connect) = (*drv).connect {
            let ret = connect(std::ptr::null(), std::ptr::null(), timeout_ms);
            if ret == ESP_OK {
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"Connected via %s\0".as_ptr(),
                    if (*drv).name.is_null() { b"?\0".as_ptr() as *const c_char } else { (*drv).name },
                );
                return ESP_OK;
            }
        }
    }

    ESP_FAIL
}

/// Trigger NTP sync via the active network transport.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn net_ntp_sync() -> i32 {
    if !net_is_connected() {
        return ESP_ERR_INVALID_STATE;
    }
    unsafe { wifi_manager_ntp_sync() }
}

/// Fill `out` with up to `max` registered transport pointers.
///
/// Returns the number of transports written.
///
/// # Safety
/// `out` must point to an array of at least `max` pointers.
#[no_mangle]
pub unsafe extern "C" fn net_list_transports(
    out: *mut *const HalNetDriver,
    max: i32,
) -> i32 {
    if out.is_null() || max <= 0 {
        return 0;
    }

    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let count = state.count.min(max as usize);
    for i in 0..count {
        *out.add(i) = state.transports[i];
    }

    count as i32
}

/// Register the built-in WiFi transport wrapper.
///
/// Call this after both net_manager_init() and wifi_manager_init() succeed.
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn net_manager_register_wifi() -> i32 {
    unsafe { net_manager_register(&WIFI_NET_DRIVER as *const HalNetDriver) }
}
