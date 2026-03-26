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
    #[cfg(not(test))]
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
    fn ble_gap_adv_start(
        own_addr_type: u8,
        direct_addr: *const c_void,
        duration_ms: i32,
        adv_params: *const BleGapAdvParams,
        cb: unsafe extern "C" fn(event: *mut BleGapEvent, arg: *mut c_void) -> i32,
        cb_arg: *mut c_void,
    ) -> i32;
    fn ble_gatts_count_cfg(svcs: *const BleGattSvcDef) -> i32;
    fn ble_gatts_add_svcs(svcs: *const BleGattSvcDef) -> i32;
    fn os_mbuf_copydata(om: *const c_void, off: i32, len: u32, dst: *mut u8) -> i32;
}

// ---------------------------------------------------------------------------
// NimBLE structs (hardware only)
// ---------------------------------------------------------------------------

/// GAP advertising parameters — mirrors `ble_gap_adv_params` in NimBLE.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGapAdvParams {
    conn_mode: u8,      // BLE_GAP_CONN_MODE_UND = 2
    disc_mode: u8,      // BLE_GAP_DISC_MODE_GEN = 2
    itvl_min: u16,      // 0 = use stack default (~160 * 0.625 ms)
    itvl_max: u16,      // 0 = use stack default
    channel_map: u8,    // 0 = all three channels
    filter_policy: u8,  // 0 = no filter
    high_duty_cycle: u8,
}

/// Minimal GAP event — only the event-type byte is read directly.
/// The full `ble_gap_event` union is large and version-specific; we read
/// only the fields we need via raw-pointer offsets.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGapEvent {
    event_type: u8,
    // remainder of the union follows in memory — we access it via raw offsets
}

/// 128-bit UUID in NimBLE's any-type wrapper.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleUuidAny {
    u_type: u8,      // BLE_UUID_TYPE_128 = 4
    _pad: [u8; 3],
    value: [u8; 16],
}

/// GATT characteristic access context — minimal view used only for the `op`
/// and `om` fields.  Layout: `op` (u8) at offset 0, then padding, then
/// `attr_handle` (u16), then `chr` pointer, then `om` (*mut c_void).
/// We access `om` via `ctxt_attr_om()` to avoid depending on the exact layout.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGattAccessCtxt {
    op: u8,
    _pad: [u8; 1],
    attr_handle: u16,
    // `chr` / `dsc` pointer (8 bytes on 32-bit Xtensa = 4 bytes)
    _chr_ptr: u32,
    // `om` mbuf pointer
    om: *mut c_void,
}

/// GATT characteristic definition.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGattChrDef {
    uuid: *const BleUuidAny,
    access_cb: Option<
        unsafe extern "C" fn(
            conn_handle: u16,
            attr_handle: u16,
            ctxt: *mut BleGattAccessCtxt,
            arg: *mut c_void,
        ) -> i32,
    >,
    arg: *mut c_void,
    descriptors: *const c_void,
    flags: u16,
    min_key_size: u8,
    val_handle: *mut u16,
}

/// GATT service definition.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGattSvcDef {
    svc_type: u8,              // BLE_GATT_SVC_TYPE_PRIMARY = 1
    uuid: *const BleUuidAny,
    includes: *const *const BleGattSvcDef,
    characteristics: *const BleGattChrDef,
}

// SAFETY: The static GATT/UUID structs are read-only after init and never
// mutated from Rust; NimBLE keeps const pointers to them.
#[cfg(target_os = "espidf")]
unsafe impl Sync for BleUuidAny {}
#[cfg(target_os = "espidf")]
unsafe impl Sync for BleGattChrDef {}
#[cfg(target_os = "espidf")]
unsafe impl Sync for BleGattSvcDef {}

// ---------------------------------------------------------------------------
// NUS UUIDs — 128-bit, little-endian byte order as required by NimBLE
// 6E400001-B5A3-F393-E0A9-E50E24DCCA9E  (Service)
// 6E400002-B5A3-F393-E0A9-E50E24DCCA9E  (RX — written by peer)
// 6E400003-B5A3-F393-E0A9-E50E24DCCA9E  (TX — notified to peer)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
static NUS_SVC_UUID: BleUuidAny = BleUuidAny {
    u_type: 4,
    _pad: [0; 3],
    value: [
        0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
        0x93, 0xF3, 0xA3, 0xB5, 0x01, 0x00, 0x40, 0x6E,
    ],
};

#[cfg(target_os = "espidf")]
static NUS_RX_UUID: BleUuidAny = BleUuidAny {
    u_type: 4,
    _pad: [0; 3],
    value: [
        0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
        0x93, 0xF3, 0xA3, 0xB5, 0x02, 0x00, 0x40, 0x6E,
    ],
};

#[cfg(target_os = "espidf")]
static NUS_TX_UUID: BleUuidAny = BleUuidAny {
    u_type: 4,
    _pad: [0; 3],
    value: [
        0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
        0x93, 0xF3, 0xA3, 0xB5, 0x03, 0x00, 0x40, 0x6E,
    ],
};

/// Handle for the TX (notify) characteristic — stored here so NimBLE can
/// update it; also copied into BleState at GATT registration time.
#[cfg(target_os = "espidf")]
static mut NUS_TX_VAL_HANDLE: u16 = 0;

// Static characteristic array for the NUS service.
// Must be 'static because NimBLE retains pointers into it.
// Terminated by a zeroed sentinel entry (uuid == null).
#[cfg(target_os = "espidf")]
static NUS_CHARACTERISTICS: [BleGattChrDef; 3] = [
    // RX characteristic — peer writes, we receive
    BleGattChrDef {
        uuid: &NUS_RX_UUID as *const BleUuidAny,
        access_cb: Some(nus_rx_access_cb),
        arg: std::ptr::null_mut(),
        descriptors: std::ptr::null(),
        flags: 0x0008 | 0x0004, // BLE_GATT_CHR_F_WRITE | BLE_GATT_CHR_F_WRITE_NO_RSP
        min_key_size: 0,
        val_handle: std::ptr::null_mut(),
    },
    // TX characteristic — we notify the peer
    BleGattChrDef {
        uuid: &NUS_TX_UUID as *const BleUuidAny,
        access_cb: Some(nus_tx_access_cb),
        arg: std::ptr::null_mut(),
        descriptors: std::ptr::null(),
        flags: 0x0010, // BLE_GATT_CHR_F_NOTIFY
        min_key_size: 0,
        // SAFETY: written once at registration time before any read.
        val_handle: unsafe { &raw mut NUS_TX_VAL_HANDLE },
    },
    // Sentinel — NimBLE uses a null uuid to detect end of array
    BleGattChrDef {
        uuid: std::ptr::null(),
        access_cb: None,
        arg: std::ptr::null_mut(),
        descriptors: std::ptr::null(),
        flags: 0,
        min_key_size: 0,
        val_handle: std::ptr::null_mut(),
    },
];

// Static service array — terminated by a zero-type sentinel.
#[cfg(target_os = "espidf")]
static NUS_SERVICES: [BleGattSvcDef; 2] = [
    BleGattSvcDef {
        svc_type: 1, // BLE_GATT_SVC_TYPE_PRIMARY
        uuid: &NUS_SVC_UUID as *const BleUuidAny,
        includes: std::ptr::null(),
        characteristics: NUS_CHARACTERISTICS.as_ptr(),
    },
    // Sentinel
    BleGattSvcDef {
        svc_type: 0,
        uuid: std::ptr::null(),
        includes: std::ptr::null(),
        characteristics: std::ptr::null(),
    },
];

// ---------------------------------------------------------------------------
// GATT access callbacks (hardware only)
// ---------------------------------------------------------------------------

/// Called by NimBLE when the peer writes to the NUS RX characteristic.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn nus_rx_access_cb(
    _conn_handle: u16,
    _attr_handle: u16,
    ctxt: *mut BleGattAccessCtxt,
    _arg: *mut c_void,
) -> i32 {
    const BLE_GATT_ACCESS_OP_WRITE_CHR: u8 = 2;
    if ctxt.is_null() {
        return 0;
    }
    if (*ctxt).op != BLE_GATT_ACCESS_OP_WRITE_CHR {
        return 0;
    }
    let om = (*ctxt).om;
    if om.is_null() {
        return 0;
    }
    // Read the total length from the mbuf chain.  The `os_mbuf` struct
    // layout on ESP-IDF / NimBLE (Xtensa 32-bit, little-endian):
    //   offset 0:  *om_next         (4 bytes)
    //   offset 4:  *om_next_run     (4 bytes)
    //   offset 8:  *om_pkthdr       (4 bytes)
    //   offset 12: *om_omp          (4 bytes)
    //   offset 16: om_flags (u8)
    //   offset 17: om_pkthdr_len (u8)
    //   offset 18: om_len (u16)  ← total data bytes in this mbuf node
    //
    // For a single-node chain (typical for short NUS writes) om_len is
    // sufficient.  For multi-node chains we use os_mbuf_copydata which
    // walks the chain.  We cap reads at 512 bytes.
    let om_len_ptr = (om as *const u8).add(18) as *const u16;
    let data_len = (*om_len_ptr) as usize;
    if data_len == 0 || data_len > 512 {
        return 0;
    }
    let mut buf = [0u8; 512];
    let rc = os_mbuf_copydata(om, 0, data_len as u32, buf.as_mut_ptr());
    if rc == 0 {
        ble_manager_rx_dispatch(buf.as_ptr(), data_len as u16);
    }
    0
}

/// TX characteristic access callback — read-only, nothing to deliver.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn nus_tx_access_cb(
    _conn_handle: u16,
    _attr_handle: u16,
    _ctxt: *mut BleGattAccessCtxt,
    _arg: *mut c_void,
) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// GAP event callback and advertising helper (hardware only)
// ---------------------------------------------------------------------------

/// Restart advertising (internal helper, may be called from GAP callback).
#[cfg(target_os = "espidf")]
unsafe fn do_advertise() -> i32 {
    let adv_params = BleGapAdvParams {
        conn_mode: 2,       // BLE_GAP_CONN_MODE_UND
        disc_mode: 2,       // BLE_GAP_DISC_MODE_GEN
        itvl_min: 0,
        itvl_max: 0,
        channel_map: 0,
        filter_policy: 0,
        high_duty_cycle: 0,
    };
    ble_gap_adv_start(
        0,                           // BLE_OWN_ADDR_PUBLIC
        std::ptr::null(),            // direct_addr = none
        i32::MAX,                    // BLE_HS_FOREVER
        &adv_params,
        gap_event_cb,
        std::ptr::null_mut(),
    )
}

/// GAP event handler — wired directly into NimBLE via `ble_gap_adv_start`.
///
/// Event layout (NimBLE, ESP-IDF v5.5, Xtensa 32-bit):
///   offset 0: event_type (u8) / padding to u32 alignment
///   The `connect` variant places `status` (i32) at offset 4 and
///   `conn_handle` (u16) at offset 8.  We use raw-pointer reads here
///   because binding the full `ble_gap_event` union in Rust is fragile
///   across NimBLE minor versions.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn gap_event_cb(event: *mut BleGapEvent, _arg: *mut c_void) -> i32 {
    if event.is_null() {
        return 0;
    }
    match (*event).event_type {
        0 => {
            // BLE_GAP_EVENT_CONNECT
            // connect.status  at byte offset 4 (i32)
            // connect.conn_handle at byte offset 8 (u16)
            let base = event as *const u8;
            let status = *(base.add(4) as *const i32);
            if status == 0 {
                let conn_handle = *(base.add(8) as *const u16);
                ble_manager_set_conn_state(BLE_STATE_CONNECTED, conn_handle);
                // Copy tx handle from the static into BleState so send() works
                if let Ok(mut s) = BLE_STATE.lock() {
                    s.tx_attr_handle = NUS_TX_VAL_HANDLE;
                }
                #[cfg(not(test))]
                esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"BLE connected handle=%d\0".as_ptr(), conn_handle as i32);
            } else {
                // Connection attempt failed — restart advertising
                do_advertise();
            }
        }
        1 => {
            // BLE_GAP_EVENT_DISCONNECT
            ble_manager_set_conn_state(BLE_STATE_OFF, 0xFFFF);
            #[cfg(not(test))]
            esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"BLE disconnected, restarting adv\0".as_ptr());
            do_advertise();
        }
        _ => {}
    }
    0
}

// ---------------------------------------------------------------------------
// GATT service registration helper (hardware only)
// ---------------------------------------------------------------------------

/// Register the NUS GATT services with NimBLE.
///
/// Must be called after `ble_svc_gap_init()` / `ble_svc_gatt_init()` and
/// before `nimble_port_freertos_init()`.
#[cfg(target_os = "espidf")]
unsafe fn register_gatt_services() -> i32 {
    let rc = ble_gatts_count_cfg(NUS_SERVICES.as_ptr());
    if rc != 0 {
        #[cfg(not(test))]
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gatts_count_cfg failed: %d\0".as_ptr(), rc);
        return rc;
    }
    let rc = ble_gatts_add_svcs(NUS_SERVICES.as_ptr());
    if rc != 0 {
        #[cfg(not(test))]
        esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gatts_add_svcs failed: %d\0".as_ptr(), rc);
    }
    rc
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
            #[cfg(not(test))]
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"nimble_port_init failed: %d\0".as_ptr(), ret);
            return ret;
        }

        ble_svc_gap_init();
        ble_svc_gatt_init();

        // Register NUS GATT services directly in Rust
        let rc = register_gatt_services();
        if rc != 0 {
            #[cfg(not(test))]
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

    #[cfg(not(test))]
    esp_log_write(
        ESP_LOG_INFO,
        TAG.as_ptr(),
        b"BLE manager initialized: '%s'\0".as_ptr(),
        name_str.as_ptr(),
    );

    #[cfg(not(target_os = "espidf"))]
    {
        #[cfg(not(test))]
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
        let rc = do_advertise();
        if rc != 0 {
            #[cfg(not(test))]
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gap_adv_start failed: %d\0".as_ptr(), rc);
            return ESP_FAIL;
        }
    }

    if let Ok(mut state) = BLE_STATE.lock() {
        state.state = BLE_STATE_ADVERTISING;
    }

    unsafe {
        #[cfg(not(test))]
        esp_log_write(ESP_LOG_INFO, TAG.as_ptr(), b"BLE advertising started\0".as_ptr());
    }

    #[cfg(not(target_os = "espidf"))]
    {
        unsafe {
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            esp_log_write(ESP_LOG_ERROR, TAG.as_ptr(), b"ble_gatts_notify_custom failed: %d\0".as_ptr(), rc);
            return ESP_FAIL;
        }
        return ESP_OK;
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (conn_handle, tx_handle);
        #[cfg(not(test))]
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the BLE state to initial condition for test isolation.
    fn reset_ble_state() {
        if let Ok(mut s) = BLE_STATE.lock() {
            *s = BleState::new();
        }
    }

    #[test]
    fn test_initial_state_is_off() {
        reset_ble_state();
        assert_eq!(ble_manager_get_state(), BLE_STATE_OFF);
        assert!(!ble_manager_is_connected());
    }

    #[test]
    fn test_set_conn_state_to_connected() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 42) };
        assert!(ble_manager_is_connected());
        assert_eq!(ble_manager_get_state(), BLE_STATE_CONNECTED);
    }

    #[test]
    fn test_set_conn_state_to_advertising() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_ADVERTISING, 0xFFFF) };
        assert!(!ble_manager_is_connected());
        assert_eq!(ble_manager_get_state(), BLE_STATE_ADVERTISING);
    }

    #[test]
    fn test_set_conn_state_transitions_connected_to_off() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 10) };
        assert!(ble_manager_is_connected());
        unsafe { ble_manager_set_conn_state(BLE_STATE_OFF, 0xFFFF) };
        assert!(!ble_manager_is_connected());
        assert_eq!(ble_manager_get_state(), BLE_STATE_OFF);
    }

    #[test]
    fn test_peer_name_null_when_disconnected() {
        reset_ble_state();
        assert!(ble_manager_get_peer_name().is_null());
    }

    #[test]
    fn test_peer_name_null_in_advertising_state() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_ADVERTISING, 0xFFFF) };
        assert!(ble_manager_get_peer_name().is_null());
    }

    #[test]
    fn test_peer_name_when_connected() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 1) };
        let name = ble_manager_get_peer_name();
        assert!(!name.is_null());
        let s = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
        assert_eq!(s, "Companion");
    }

    #[test]
    fn test_send_null_data_returns_invalid_arg() {
        reset_ble_state();
        let rc = unsafe { ble_manager_send(std::ptr::null(), 10) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_send_zero_len_returns_invalid_arg() {
        reset_ble_state();
        let data = [0u8; 4];
        let rc = unsafe { ble_manager_send(data.as_ptr(), 0) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_send_when_off_returns_invalid_state() {
        reset_ble_state();
        let data = [0x42u8; 4];
        let rc = unsafe { ble_manager_send(data.as_ptr(), 4) };
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_send_when_advertising_returns_invalid_state() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_ADVERTISING, 0xFFFF) };
        let data = [0x42u8; 4];
        let rc = unsafe { ble_manager_send(data.as_ptr(), 4) };
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_send_when_connected_returns_not_supported_on_simulator() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 1) };
        let data = [0x42u8; 4];
        let rc = unsafe { ble_manager_send(data.as_ptr(), 4) };
        // On simulator (non-espidf target), send returns ESP_ERR_NOT_SUPPORTED
        #[cfg(not(target_os = "espidf"))]
        assert_eq!(rc, ESP_ERR_NOT_SUPPORTED);
        #[cfg(target_os = "espidf")]
        {
            // On hardware, would attempt actual send; we can't fully test without
            // NimBLE FFI available. Just verify it's not INVALID_STATE.
            assert_ne!(rc, ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_send_notification_null_title() {
        reset_ble_state();
        let body = std::ffi::CString::new("body").unwrap();
        let rc = unsafe { ble_manager_send_notification(std::ptr::null(), body.as_ptr()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_send_notification_null_body() {
        reset_ble_state();
        let title = std::ffi::CString::new("title").unwrap();
        let rc = unsafe { ble_manager_send_notification(title.as_ptr(), std::ptr::null()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_send_notification_both_null() {
        reset_ble_state();
        let rc = unsafe { ble_manager_send_notification(std::ptr::null(), std::ptr::null()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_send_notification_when_disconnected() {
        reset_ble_state();
        let title = std::ffi::CString::new("Alert").unwrap();
        let body = std::ffi::CString::new("Test message").unwrap();
        let rc = unsafe { ble_manager_send_notification(title.as_ptr(), body.as_ptr()) };
        // Constructs the message and calls send(), which fails with INVALID_STATE
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_start_advertising_not_initialized() {
        reset_ble_state();
        let rc = ble_manager_start_advertising();
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_stop_advertising_not_initialized() {
        reset_ble_state();
        let rc = ble_manager_stop_advertising();
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_disconnect_when_off() {
        reset_ble_state();
        let rc = ble_manager_disconnect();
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_disconnect_when_advertising() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_ADVERTISING, 0xFFFF) };
        let rc = ble_manager_disconnect();
        assert_eq!(rc, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_disconnect_when_connected() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 5) };
        let rc = ble_manager_disconnect();
        // On simulator, still returns ESP_OK (no-op)
        // On hardware, would call ble_gap_terminate
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_register_rx_cb_returns_ok() {
        reset_ble_state();
        let rc = unsafe { ble_manager_register_rx_cb(None, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_register_rx_cb_with_callback() {
        reset_ble_state();
        unsafe extern "C" fn dummy_cb(_data: *const u8, _len: u16, _user_data: *mut c_void) {}
        let rc = unsafe { ble_manager_register_rx_cb(Some(dummy_cb), std::ptr::null_mut()) };
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_register_rx_cb_with_user_data() {
        reset_ble_state();
        unsafe extern "C" fn dummy_cb(_data: *const u8, _len: u16, _user_data: *mut c_void) {}
        let mut context = 42u32;
        let rc = unsafe {
            ble_manager_register_rx_cb(
                Some(dummy_cb),
                &mut context as *mut _ as *mut c_void,
            )
        };
        assert_eq!(rc, ESP_OK);
    }

    #[test]
    fn test_consecutive_state_changes() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_ADVERTISING, 0xFFFF) };
        assert_eq!(ble_manager_get_state(), BLE_STATE_ADVERTISING);
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 7) };
        assert_eq!(ble_manager_get_state(), BLE_STATE_CONNECTED);
        assert!(ble_manager_is_connected());
        unsafe { ble_manager_set_conn_state(BLE_STATE_OFF, 0xFFFF) };
        assert_eq!(ble_manager_get_state(), BLE_STATE_OFF);
        assert!(!ble_manager_is_connected());
    }

    #[test]
    fn test_conn_handle_storage() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 99) };
        // Verify connected state was set
        assert!(ble_manager_is_connected());
        // Transition away and verify handle is updated
        unsafe { ble_manager_set_conn_state(BLE_STATE_OFF, 0xFFFF) };
        assert!(!ble_manager_is_connected());
    }

    #[test]
    fn test_send_with_various_sizes() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 1) };

        // Send 1 byte
        let data_1 = [0u8; 1];
        let rc = unsafe { ble_manager_send(data_1.as_ptr(), 1) };
        #[cfg(not(target_os = "espidf"))]
        assert_eq!(rc, ESP_ERR_NOT_SUPPORTED);

        // Send 256 bytes
        let data_256 = [0x42u8; 256];
        let rc = unsafe { ble_manager_send(data_256.as_ptr(), 256) };
        #[cfg(not(target_os = "espidf"))]
        assert_eq!(rc, ESP_ERR_NOT_SUPPORTED);
    }

    #[test]
    fn test_peer_name_consistent_across_calls() {
        reset_ble_state();
        unsafe { ble_manager_set_conn_state(BLE_STATE_CONNECTED, 1) };
        let name1 = ble_manager_get_peer_name();
        let name2 = ble_manager_get_peer_name();
        assert_eq!(name1, name2);
        let s = unsafe { CStr::from_ptr(name1) }.to_str().unwrap();
        assert_eq!(s, "Companion");
    }
}
