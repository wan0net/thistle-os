// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — ble_scanner module
//
// BLE device discovery / scanning.  Complements ble_manager (which handles
// NUS GATT server + advertising) by adding passive and active scan support.
// On ESP-IDF builds the NimBLE `ble_gap_disc()` API is used.  On simulator
// and test builds all hardware calls are stubbed.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_FAIL: i32 = -1;

// ---------------------------------------------------------------------------
// Logging FFI
// ---------------------------------------------------------------------------

extern "C" {
    #[cfg(not(test))]
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

const ESP_LOG_INFO: i32 = 3;
const ESP_LOG_WARN: i32 = 2;
#[allow(dead_code)]
const ESP_LOG_ERROR: i32 = 1;

static TAG: &[u8] = b"ble_scan\0";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 64;
const MAX_NAME_LEN: usize = 32;
const MAX_ADV_LEN: usize = 31;
const MAX_SERVICES: usize = 8;

// BLE AD type constants
const AD_TYPE_FLAGS: u8 = 0x01;
const AD_TYPE_UUID16_INCOMPLETE: u8 = 0x02;
const AD_TYPE_UUID16_COMPLETE: u8 = 0x03;
const AD_TYPE_UUID128_INCOMPLETE: u8 = 0x06;
const AD_TYPE_UUID128_COMPLETE: u8 = 0x07;
const AD_TYPE_SHORT_NAME: u8 = 0x08;
const AD_TYPE_COMPLETE_NAME: u8 = 0x09;
const AD_TYPE_MFG_DATA: u8 = 0xFF;

// ---------------------------------------------------------------------------
// NimBLE FFI (hardware only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn ble_gap_disc(
        own_addr_type: u8,
        duration_ms: i32,
        disc_params: *const BleGapDiscParams,
        cb: unsafe extern "C" fn(event: *mut BleGapDiscEvent, arg: *mut c_void) -> i32,
        cb_arg: *mut c_void,
    ) -> i32;
    fn ble_gap_disc_cancel() -> i32;
}

#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGapDiscParams {
    itvl: u16,
    window: u16,
    filter_policy: u8,
    limited: u8,
    passive: u8,
    filter_duplicates: u8,
}

/// Minimal GAP discovery event — we read fields via raw-pointer offsets.
#[cfg(target_os = "espidf")]
#[repr(C)]
struct BleGapDiscEvent {
    event_type: u8,
    // remainder accessed via raw offsets
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A discovered BLE device.
#[derive(Clone)]
pub(crate) struct BleDevice {
    addr: [u8; 6],
    addr_type: u8,
    name: [u8; MAX_NAME_LEN],
    name_len: usize,
    rssi: i8,
    adv_data: [u8; MAX_ADV_LEN],
    adv_len: usize,
    scan_rsp: [u8; MAX_ADV_LEN],
    scan_rsp_len: usize,
    services: [[u8; 16]; MAX_SERVICES],
    service_count: usize,
    mfg_data: [u8; MAX_ADV_LEN],
    mfg_len: usize,
    company_id: u16,
    last_seen: u32,
    seen_count: u32,
    connectable: bool,
}

impl BleDevice {
    const fn new() -> Self {
        BleDevice {
            addr: [0u8; 6],
            addr_type: 0,
            name: [0u8; MAX_NAME_LEN],
            name_len: 0,
            rssi: -127,
            adv_data: [0u8; MAX_ADV_LEN],
            adv_len: 0,
            scan_rsp: [0u8; MAX_ADV_LEN],
            scan_rsp_len: 0,
            services: [[0u8; 16]; MAX_SERVICES],
            service_count: 0,
            mfg_data: [0u8; MAX_ADV_LEN],
            mfg_len: 0,
            company_id: 0,
            last_seen: 0,
            seen_count: 0,
            connectable: false,
        }
    }
}

/// FFI-compatible device info struct.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CBleDeviceInfo {
    pub addr: [u8; 6],
    pub addr_type: u8,
    pub name: [u8; 32],
    pub name_len: u8,
    pub rssi: i8,
    pub company_id: u16,
    pub service_count: u8,
    pub services: [[u8; 16]; 8],
    pub mfg_data: [u8; 31],
    pub mfg_len: u8,
    pub seen_count: u32,
    pub connectable: bool,
}

impl CBleDeviceInfo {
    const fn zeroed() -> Self {
        CBleDeviceInfo {
            addr: [0u8; 6],
            addr_type: 0,
            name: [0u8; 32],
            name_len: 0,
            rssi: -127,
            company_id: 0,
            service_count: 0,
            services: [[0u8; 16]; 8],
            mfg_data: [0u8; 31],
            mfg_len: 0,
            seen_count: 0,
            connectable: false,
        }
    }
}

/// FFI-compatible scan statistics struct.
#[repr(C)]
pub struct CBleScanStats {
    pub device_count: u32,
    pub total_adv_seen: u32,
    pub scanning: bool,
    pub scan_type: u8,
    pub strongest_rssi: i8,
    pub weakest_rssi: i8,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn device_to_c(dev: &BleDevice) -> CBleDeviceInfo {
    let mut info = CBleDeviceInfo::zeroed();
    info.addr = dev.addr;
    info.addr_type = dev.addr_type;
    info.name = dev.name;
    info.name_len = dev.name_len as u8;
    info.rssi = dev.rssi;
    info.company_id = dev.company_id;
    info.service_count = dev.service_count as u8;
    info.services = dev.services;
    let mfg_len = dev.mfg_len.min(MAX_ADV_LEN);
    info.mfg_data[..mfg_len].copy_from_slice(&dev.mfg_data[..mfg_len]);
    info.mfg_len = mfg_len as u8;
    info.seen_count = dev.seen_count;
    info.connectable = dev.connectable;
    info
}

// ---------------------------------------------------------------------------
// Scanner state
// ---------------------------------------------------------------------------

struct BleScanner {
    devices: [Option<BleDevice>; MAX_DEVICES],
    device_count: usize,
    scanning: bool,
    scan_type: u8,
    filter_rssi: i8,
    filter_name: [u8; MAX_NAME_LEN],
    filter_name_len: usize,
    scan_duration_ms: u32,
    total_adv_seen: u32,
}

impl BleScanner {
    const fn new() -> Self {
        const NONE: Option<BleDevice> = None;
        BleScanner {
            devices: [NONE; MAX_DEVICES],
            device_count: 0,
            scanning: false,
            scan_type: 0,
            filter_rssi: -127,
            filter_name: [0u8; MAX_NAME_LEN],
            filter_name_len: 0,
            scan_duration_ms: 0,
            total_adv_seen: 0,
        }
    }

    fn clear_devices(&mut self) {
        for slot in self.devices.iter_mut() {
            *slot = None;
        }
        self.device_count = 0;
        self.total_adv_seen = 0;
    }

    /// Find device index by BLE MAC address.
    fn find_by_addr(&self, addr: &[u8; 6]) -> Option<usize> {
        for i in 0..MAX_DEVICES {
            if let Some(ref dev) = self.devices[i] {
                if dev.addr == *addr {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Find first empty slot.
    fn find_empty_slot(&self) -> Option<usize> {
        for i in 0..MAX_DEVICES {
            if self.devices[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    /// Find the device with the weakest RSSI (for eviction at capacity).
    fn find_weakest_rssi_index(&self) -> Option<usize> {
        let mut weakest_idx: Option<usize> = None;
        let mut weakest_rssi: i8 = 127;
        for i in 0..MAX_DEVICES {
            if let Some(ref dev) = self.devices[i] {
                if dev.rssi < weakest_rssi {
                    weakest_rssi = dev.rssi;
                    weakest_idx = Some(i);
                }
            }
        }
        weakest_idx
    }

    /// Insert or update a device from an advertising report.
    fn process_adv_report(&mut self, new_dev: &BleDevice) {
        self.total_adv_seen += 1;

        // Apply RSSI filter
        if new_dev.rssi < self.filter_rssi {
            return;
        }

        // Apply name prefix filter
        if self.filter_name_len > 0 {
            if new_dev.name_len < self.filter_name_len {
                return;
            }
            let prefix = &self.filter_name[..self.filter_name_len];
            let dev_name = &new_dev.name[..self.filter_name_len];
            // Case-insensitive prefix match
            let matches = prefix.iter().zip(dev_name.iter()).all(|(&a, &b)| {
                a.to_ascii_lowercase() == b.to_ascii_lowercase()
            });
            if !matches {
                return;
            }
        }

        if let Some(idx) = self.find_by_addr(&new_dev.addr) {
            // Update existing device
            if let Some(ref mut dev) = self.devices[idx] {
                dev.rssi = new_dev.rssi;
                dev.last_seen = new_dev.last_seen;
                dev.seen_count += 1;
                // Update name if the new one is longer
                if new_dev.name_len > dev.name_len {
                    dev.name = new_dev.name;
                    dev.name_len = new_dev.name_len;
                }
                // Merge services
                for si in 0..new_dev.service_count {
                    if dev.service_count >= MAX_SERVICES {
                        break;
                    }
                    // Check for duplicate
                    let mut dup = false;
                    for di in 0..dev.service_count {
                        if dev.services[di] == new_dev.services[si] {
                            dup = true;
                            break;
                        }
                    }
                    if !dup {
                        dev.services[dev.service_count] = new_dev.services[si];
                        dev.service_count += 1;
                    }
                }
                // Update mfg data if new one is present
                if new_dev.mfg_len > 0 {
                    dev.mfg_data = new_dev.mfg_data;
                    dev.mfg_len = new_dev.mfg_len;
                    dev.company_id = new_dev.company_id;
                }
                // Update connectable flag
                dev.connectable = new_dev.connectable;
            }
        } else {
            // Insert new device
            let idx = if let Some(i) = self.find_empty_slot() {
                i
            } else {
                // At capacity — evict weakest RSSI
                match self.find_weakest_rssi_index() {
                    Some(i) => {
                        self.devices[i] = None;
                        self.device_count -= 1;
                        i
                    }
                    None => return,
                }
            };
            let mut dev = new_dev.clone();
            dev.seen_count = 1;
            self.devices[idx] = Some(dev);
            self.device_count += 1;
        }
    }

    /// Get the nth populated device (skipping empty slots).
    fn get_device_by_index(&self, index: usize) -> Option<&BleDevice> {
        let mut count = 0usize;
        for slot in self.devices.iter() {
            if let Some(ref dev) = slot {
                if count == index {
                    return Some(dev);
                }
                count += 1;
            }
        }
        None
    }

    /// Sort devices by RSSI (strongest first).  Uses simple insertion sort
    /// since MAX_DEVICES is small.
    fn sort_by_rssi(&mut self) {
        // Collect all devices into a temporary vec-like array
        let mut sorted: [Option<BleDevice>; MAX_DEVICES] = {
            const NONE: Option<BleDevice> = None;
            [NONE; MAX_DEVICES]
        };
        let mut count = 0usize;
        for slot in self.devices.iter() {
            if let Some(ref dev) = slot {
                sorted[count] = Some(dev.clone());
                count += 1;
            }
        }
        // Insertion sort by RSSI descending
        for i in 1..count {
            let mut j = i;
            while j > 0 {
                let rssi_j = sorted[j].as_ref().map(|d| d.rssi).unwrap_or(-127);
                let rssi_jm1 = sorted[j - 1].as_ref().map(|d| d.rssi).unwrap_or(-127);
                if rssi_j > rssi_jm1 {
                    sorted.swap(j, j - 1);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        self.devices = sorted;
    }
}

// SAFETY: Only accessed under Mutex.
unsafe impl Send for BleScanner {}

static SCANNER: Mutex<BleScanner> = Mutex::new(BleScanner::new());

// ---------------------------------------------------------------------------
// Advertising data parser
// ---------------------------------------------------------------------------

/// Parse BLE advertising data (TLV format) into a BleDevice.
///
/// Handles malformed / truncated data without panicking by breaking out of
/// the parse loop when a field extends beyond the buffer.
fn parse_adv_data(raw: &[u8], device: &mut BleDevice) {
    let len = raw.len();
    let mut pos = 0usize;

    while pos < len {
        let field_len = raw[pos] as usize;
        if field_len == 0 {
            pos += 1;
            continue;
        }
        // field_len includes the type byte but not the length byte itself
        if pos + 1 + field_len > len {
            // Truncated field — stop parsing
            break;
        }
        let ad_type = raw[pos + 1];
        let data_start = pos + 2;
        let data_len = field_len - 1; // subtract the type byte

        match ad_type {
            AD_TYPE_FLAGS => {
                // Flags — nothing to store beyond what we already have
            }
            AD_TYPE_UUID16_INCOMPLETE | AD_TYPE_UUID16_COMPLETE => {
                // 16-bit UUIDs — each is 2 bytes, store as 128-bit with
                // Bluetooth Base UUID: 0000xxxx-0000-1000-8000-00805F9B34FB
                let mut i = 0;
                while i + 1 < data_len && device.service_count < MAX_SERVICES {
                    let mut uuid128 = [0u8; 16];
                    // Bluetooth Base UUID in little-endian
                    uuid128[0] = 0xFB;
                    uuid128[1] = 0x34;
                    uuid128[2] = 0x9B;
                    uuid128[3] = 0x5F;
                    uuid128[4] = 0x80;
                    uuid128[5] = 0x00;
                    uuid128[6] = 0x00;
                    uuid128[7] = 0x80;
                    uuid128[8] = 0x00;
                    uuid128[9] = 0x10;
                    uuid128[10] = 0x00;
                    uuid128[11] = 0x00;
                    // 16-bit UUID in bytes 12-13 (little-endian)
                    uuid128[12] = raw[data_start + i];
                    uuid128[13] = raw[data_start + i + 1];
                    uuid128[14] = 0x00;
                    uuid128[15] = 0x00;
                    device.services[device.service_count] = uuid128;
                    device.service_count += 1;
                    i += 2;
                }
            }
            AD_TYPE_UUID128_INCOMPLETE | AD_TYPE_UUID128_COMPLETE => {
                let mut i = 0;
                while i + 15 < data_len && device.service_count < MAX_SERVICES {
                    let mut uuid128 = [0u8; 16];
                    uuid128.copy_from_slice(&raw[data_start + i..data_start + i + 16]);
                    device.services[device.service_count] = uuid128;
                    device.service_count += 1;
                    i += 16;
                }
            }
            AD_TYPE_SHORT_NAME | AD_TYPE_COMPLETE_NAME => {
                let name_len = data_len.min(MAX_NAME_LEN);
                // Only update name if this is a complete name or longer
                if ad_type == AD_TYPE_COMPLETE_NAME || name_len > device.name_len {
                    device.name[..name_len].copy_from_slice(&raw[data_start..data_start + name_len]);
                    if name_len < MAX_NAME_LEN {
                        device.name[name_len] = 0;
                    }
                    device.name_len = name_len;
                }
            }
            AD_TYPE_MFG_DATA => {
                if data_len >= 2 {
                    device.company_id =
                        (raw[data_start] as u16) | ((raw[data_start + 1] as u16) << 8);
                    let mfg_payload_len = (data_len - 2).min(MAX_ADV_LEN);
                    if mfg_payload_len > 0 {
                        device.mfg_data[..mfg_payload_len]
                            .copy_from_slice(&raw[data_start + 2..data_start + 2 + mfg_payload_len]);
                    }
                    device.mfg_len = mfg_payload_len;
                } else if data_len == 1 {
                    // Malformed — only one byte of company ID
                    device.company_id = raw[data_start] as u16;
                }
            }
            _ => {
                // Unknown AD type — skip
            }
        }

        pos += 1 + field_len;
    }
}

// ---------------------------------------------------------------------------
// NimBLE scan callback (hardware only)
// ---------------------------------------------------------------------------

/// Scan event callback invoked by NimBLE from the host task.
///
/// Event type 0 = BLE_GAP_EVENT_DISC.  The disc struct layout (Xtensa 32-bit):
///   offset 0:  event_type (u8)
///   offset 4:  disc.event_type (u8)      — adv type
///   offset 5:  disc.length_data (u8)
///   offset 6:  disc.addr.type (u8)
///   offset 7:  disc.addr.val[6]
///   offset 13: disc.rssi (i8)
///   offset 16: disc.data (pointer)
///
/// We read these via raw byte-offset pointer arithmetic.
#[cfg(target_os = "espidf")]
unsafe extern "C" fn scan_event_cb(event: *mut BleGapDiscEvent, _arg: *mut c_void) -> i32 {
    if event.is_null() {
        return 0;
    }
    let event_type = (*event).event_type;
    if event_type != 0 {
        // Not a discovery event
        return 0;
    }

    let base = event as *const u8;

    // Parse advertising report fields from known offsets
    let adv_type = *base.add(4);
    let data_len = *base.add(5) as usize;
    let addr_type = *base.add(6);
    let mut addr = [0u8; 6];
    for i in 0..6 {
        addr[i] = *base.add(7 + i);
    }
    let rssi = *base.add(13) as i8;

    // Data pointer at offset 16 (32-bit pointer)
    let data_ptr = *(base.add(16) as *const *const u8);

    let mut dev = BleDevice::new();
    dev.addr = addr;
    dev.addr_type = addr_type;
    dev.rssi = rssi;
    dev.connectable = adv_type == 0 || adv_type == 1; // ADV_IND or ADV_DIRECT_IND

    if !data_ptr.is_null() && data_len > 0 && data_len <= MAX_ADV_LEN {
        let raw = std::slice::from_raw_parts(data_ptr, data_len);
        dev.adv_data[..data_len].copy_from_slice(raw);
        dev.adv_len = data_len;
        parse_adv_data(raw, &mut dev);
    }

    // TODO: read system tick for last_seen
    dev.last_seen = 0;

    if let Ok(mut scanner) = SCANNER.lock() {
        scanner.process_adv_report(&dev);
    }

    0
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the BLE scanner and clear the device list.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_init() -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            s.clear_devices();
            s.scanning = false;
            s.scan_type = 0;
            s.filter_rssi = -127;
            s.filter_name_len = 0;
            s.scan_duration_ms = 0;
            #[cfg(not(test))]
            unsafe {
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"BLE scanner initialized\0".as_ptr(),
                );
            }
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Start BLE scanning.
///
/// `scan_type`: 0 = passive, 1 = active.
/// `duration_ms`: scan duration in milliseconds (0 = indefinite).
///
/// On simulator builds, just sets the scanning flag.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_start(scan_type: u8, duration_ms: u32) -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            if s.scanning {
                return ESP_OK; // Already scanning
            }
            s.scan_type = scan_type;
            s.scan_duration_ms = duration_ms;
            s.scanning = true;

            #[cfg(target_os = "espidf")]
            {
                let params = BleGapDiscParams {
                    itvl: 0,
                    window: 0,
                    filter_policy: 0,
                    limited: 0,
                    passive: if scan_type == 0 { 1 } else { 0 },
                    filter_duplicates: 0,
                };
                let dur = if duration_ms == 0 {
                    0x7FFF_FFFFi32 // BLE_HS_FOREVER
                } else {
                    duration_ms as i32
                };
                // SAFETY: NimBLE FFI call with valid params struct.
                let rc = unsafe {
                    ble_gap_disc(
                        0, // BLE_OWN_ADDR_PUBLIC
                        dur,
                        &params,
                        scan_event_cb,
                        std::ptr::null_mut(),
                    )
                };
                if rc != 0 {
                    s.scanning = false;
                    return rc;
                }
            }

            #[cfg(not(test))]
            unsafe {
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"BLE scan started type=%d dur=%d\0".as_ptr(),
                    scan_type as i32,
                    duration_ms as i32,
                );
            }
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Stop BLE scanning.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_stop() -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            if !s.scanning {
                return ESP_OK; // Already stopped
            }
            s.scanning = false;

            #[cfg(target_os = "espidf")]
            {
                // SAFETY: NimBLE FFI call, no parameters.
                unsafe {
                    ble_gap_disc_cancel();
                }
            }

            #[cfg(not(test))]
            unsafe {
                esp_log_write(
                    ESP_LOG_INFO,
                    TAG.as_ptr(),
                    b"BLE scan stopped\0".as_ptr(),
                );
            }
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Check if scanning is currently active.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_is_scanning() -> bool {
    match SCANNER.lock() {
        Ok(s) => s.scanning,
        Err(_) => false,
    }
}

/// Get the number of discovered devices.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_get_count() -> i32 {
    match SCANNER.lock() {
        Ok(s) => s.device_count as i32,
        Err(_) => 0,
    }
}

/// Get a discovered device by index.
///
/// # Safety
/// `out` must point to a valid `CBleDeviceInfo` struct.
#[no_mangle]
pub unsafe extern "C" fn rs_ble_scanner_get_device(
    index: u32,
    out: *mut CBleDeviceInfo,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match SCANNER.lock() {
        Ok(s) => {
            match s.get_device_by_index(index as usize) {
                Some(dev) => {
                    *out = device_to_c(dev);
                    ESP_OK
                }
                None => ESP_ERR_NOT_FOUND,
            }
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Find a device by its 6-byte BLE MAC address.
///
/// # Safety
/// `addr` must point to at least 6 bytes.  `out` must point to a valid
/// `CBleDeviceInfo` struct.
#[no_mangle]
pub unsafe extern "C" fn rs_ble_scanner_find_by_addr(
    addr: *const u8,
    out: *mut CBleDeviceInfo,
) -> i32 {
    if addr.is_null() || out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mut addr_buf = [0u8; 6];
    addr_buf.copy_from_slice(std::slice::from_raw_parts(addr, 6));

    match SCANNER.lock() {
        Ok(s) => {
            match s.find_by_addr(&addr_buf) {
                Some(idx) => {
                    if let Some(ref dev) = s.devices[idx] {
                        *out = device_to_c(dev);
                        return ESP_OK;
                    }
                    ESP_ERR_NOT_FOUND
                }
                None => ESP_ERR_NOT_FOUND,
            }
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Find devices whose name contains the given substring.
///
/// Returns the number of matched devices (written to `results`), or a
/// negative error code.
///
/// # Safety
/// `name` must be a valid C string.  `results` must point to an array of at
/// least `max` `CBleDeviceInfo` structs.
#[no_mangle]
pub unsafe extern "C" fn rs_ble_scanner_find_by_name(
    name: *const c_char,
    results: *mut CBleDeviceInfo,
    max: u32,
) -> i32 {
    if name.is_null() || results.is_null() || max == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let needle = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let needle_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();

    match SCANNER.lock() {
        Ok(s) => {
            let mut found = 0u32;
            for slot in s.devices.iter() {
                if found >= max {
                    break;
                }
                if let Some(ref dev) = slot {
                    if dev.name_len == 0 {
                        continue;
                    }
                    // Case-insensitive substring search
                    let dev_name = &dev.name[..dev.name_len];
                    if contains_ci(dev_name, &needle_lower) {
                        *results.add(found as usize) = device_to_c(dev);
                        found += 1;
                    }
                }
            }
            found as i32
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Case-insensitive substring search.
fn contains_ci(haystack: &[u8], needle_lower: &[u8]) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.len() < needle_lower.len() {
        return false;
    }
    for start in 0..=(haystack.len() - needle_lower.len()) {
        let mut matched = true;
        for i in 0..needle_lower.len() {
            if haystack[start + i].to_ascii_lowercase() != needle_lower[i] {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}

/// Clear all discovered devices.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_clear() -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            s.clear_devices();
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Set minimum RSSI threshold.  Devices weaker than this are ignored.
/// Pass -127 to disable the filter.
#[no_mangle]
pub extern "C" fn rs_ble_scanner_set_rssi_filter(min_rssi: i8) -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            s.filter_rssi = min_rssi;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Set a name prefix filter.  Only devices whose name starts with the given
/// prefix (case-insensitive) are stored.  Pass NULL to clear the filter.
///
/// # Safety
/// `prefix` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_ble_scanner_set_name_filter(prefix: *const c_char) -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            if prefix.is_null() {
                s.filter_name_len = 0;
                return ESP_OK;
            }
            let cstr = match CStr::from_ptr(prefix).to_str() {
                Ok(v) => v,
                Err(_) => return ESP_ERR_INVALID_ARG,
            };
            let bytes = cstr.as_bytes();
            let len = bytes.len().min(MAX_NAME_LEN);
            s.filter_name[..len].copy_from_slice(&bytes[..len]);
            s.filter_name_len = len;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Get scan statistics.
///
/// # Safety
/// `out` must point to a valid `CBleScanStats` struct.
#[no_mangle]
pub unsafe extern "C" fn rs_ble_scanner_get_stats(out: *mut CBleScanStats) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match SCANNER.lock() {
        Ok(s) => {
            let mut strongest: i8 = -127;
            let mut weakest: i8 = 127;
            for slot in s.devices.iter() {
                if let Some(ref dev) = slot {
                    if dev.rssi > strongest {
                        strongest = dev.rssi;
                    }
                    if dev.rssi < weakest {
                        weakest = dev.rssi;
                    }
                }
            }
            if s.device_count == 0 {
                strongest = 0;
                weakest = 0;
            }
            (*out).device_count = s.device_count as u32;
            (*out).total_adv_seen = s.total_adv_seen;
            (*out).scanning = s.scanning;
            (*out).scan_type = s.scan_type;
            (*out).strongest_rssi = strongest;
            (*out).weakest_rssi = weakest;
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

/// Sort the discovered device list by RSSI (strongest first).
#[no_mangle]
pub extern "C" fn rs_ble_scanner_sort_by_rssi() -> i32 {
    match SCANNER.lock() {
        Ok(mut s) => {
            s.sort_by_rssi();
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_STATE,
    }
}

// ---------------------------------------------------------------------------
// Test helpers (simulator / test builds only)
// ---------------------------------------------------------------------------

#[cfg(any(test, not(target_os = "espidf")))]
/// Inject a test device into the scanner.  Used by simulator and tests.
pub(crate) fn inject_test_device(device: &BleDevice) {
    if let Ok(mut s) = SCANNER.lock() {
        s.process_adv_report(device);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the scanner state to initial condition for test isolation.
    fn reset() {
        if let Ok(mut s) = SCANNER.lock() {
            *s = BleScanner::new();
        }
    }

    /// Create a test device with the given address, name, and RSSI.
    fn make_device(addr: [u8; 6], name: &str, rssi: i8) -> BleDevice {
        let mut dev = BleDevice::new();
        dev.addr = addr;
        dev.rssi = rssi;
        let bytes = name.as_bytes();
        let len = bytes.len().min(MAX_NAME_LEN);
        dev.name[..len].copy_from_slice(&bytes[..len]);
        dev.name_len = len;
        dev.seen_count = 1;
        dev.connectable = true;
        dev
    }

    // -----------------------------------------------------------------------
    // Init tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_returns_ok() {
        reset();
        assert_eq!(rs_ble_scanner_init(), ESP_OK);
    }

    #[test]
    fn test_double_init_is_idempotent() {
        reset();
        assert_eq!(rs_ble_scanner_init(), ESP_OK);
        assert_eq!(rs_ble_scanner_init(), ESP_OK);
        assert_eq!(rs_ble_scanner_get_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Scan lifecycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_start_stop() {
        reset();
        rs_ble_scanner_init();
        assert_eq!(rs_ble_scanner_start(0, 0), ESP_OK);
        assert!(rs_ble_scanner_is_scanning());
        assert_eq!(rs_ble_scanner_stop(), ESP_OK);
        assert!(!rs_ble_scanner_is_scanning());
    }

    #[test]
    fn test_start_while_scanning() {
        reset();
        rs_ble_scanner_init();
        rs_ble_scanner_start(0, 0);
        // Starting again should just return OK
        assert_eq!(rs_ble_scanner_start(1, 5000), ESP_OK);
        assert!(rs_ble_scanner_is_scanning());
    }

    #[test]
    fn test_stop_when_not_scanning() {
        reset();
        rs_ble_scanner_init();
        assert_eq!(rs_ble_scanner_stop(), ESP_OK);
        assert!(!rs_ble_scanner_is_scanning());
    }

    #[test]
    fn test_start_with_duration() {
        reset();
        rs_ble_scanner_init();
        assert_eq!(rs_ble_scanner_start(1, 10000), ESP_OK);
        assert!(rs_ble_scanner_is_scanning());
        // Verify scan_type was set
        if let Ok(s) = SCANNER.lock() {
            assert_eq!(s.scan_type, 1);
            assert_eq!(s.scan_duration_ms, 10000);
        }
    }

    // -----------------------------------------------------------------------
    // Device storage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_inject_device() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], "TestDev", -50);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
    }

    #[test]
    fn test_inject_multiple_devices() {
        reset();
        rs_ble_scanner_init();
        for i in 0..5u8 {
            let dev = make_device([i, 0, 0, 0, 0, 0], "Dev", -(50 + i as i8));
            inject_test_device(&dev);
        }
        assert_eq!(rs_ble_scanner_get_count(), 5);
    }

    #[test]
    fn test_update_existing_device() {
        reset();
        rs_ble_scanner_init();
        let addr = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let dev1 = make_device(addr, "Short", -60);
        inject_test_device(&dev1);
        let dev2 = make_device(addr, "LongerName", -45);
        inject_test_device(&dev2);
        // Should still be one device
        assert_eq!(rs_ble_scanner_get_count(), 1);
        // Check updated fields
        if let Ok(s) = SCANNER.lock() {
            let d = s.find_by_addr(&addr).and_then(|i| s.devices[i].as_ref()).unwrap();
            assert_eq!(d.rssi, -45);
            assert_eq!(d.seen_count, 2);
            assert_eq!(d.name_len, 10); // "LongerName" is longer
        }
    }

    #[test]
    fn test_max_devices() {
        reset();
        rs_ble_scanner_init();
        for i in 0..MAX_DEVICES {
            let addr = [(i & 0xFF) as u8, ((i >> 8) & 0xFF) as u8, 0, 0, 0, 0];
            let dev = make_device(addr, "D", -50);
            inject_test_device(&dev);
        }
        assert_eq!(rs_ble_scanner_get_count(), MAX_DEVICES as i32);
    }

    #[test]
    fn test_clear_devices() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([1, 2, 3, 4, 5, 6], "Test", -50);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
        assert_eq!(rs_ble_scanner_clear(), ESP_OK);
        assert_eq!(rs_ble_scanner_get_count(), 0);
    }

    #[test]
    fn test_device_count() {
        reset();
        rs_ble_scanner_init();
        assert_eq!(rs_ble_scanner_get_count(), 0);
        let dev = make_device([1, 0, 0, 0, 0, 0], "A", -40);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
        let dev2 = make_device([2, 0, 0, 0, 0, 0], "B", -50);
        inject_test_device(&dev2);
        assert_eq!(rs_ble_scanner_get_count(), 2);
    }

    // -----------------------------------------------------------------------
    // Query tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_by_index() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([0x10, 0x20, 0x30, 0x40, 0x50, 0x60], "MyDevice", -55);
        inject_test_device(&dev);
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_get_device(0, &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.addr, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
        assert_eq!(out.rssi, -55);
        assert_eq!(out.name_len, 8);
    }

    #[test]
    fn test_get_out_of_range() {
        reset();
        rs_ble_scanner_init();
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_get_device(0, &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_find_by_addr() {
        reset();
        rs_ble_scanner_init();
        let addr = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01];
        let dev = make_device(addr, "Found", -30);
        inject_test_device(&dev);
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_find_by_addr(addr.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.addr, addr);
    }

    #[test]
    fn test_find_by_addr_not_found() {
        reset();
        rs_ble_scanner_init();
        let addr = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_find_by_addr(addr.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_find_by_name_substring() {
        reset();
        rs_ble_scanner_init();
        let dev1 = make_device([1, 0, 0, 0, 0, 0], "ThistlePhone", -40);
        let dev2 = make_device([2, 0, 0, 0, 0, 0], "OtherDevice", -50);
        let dev3 = make_device([3, 0, 0, 0, 0, 0], "ThistleWatch", -45);
        inject_test_device(&dev1);
        inject_test_device(&dev2);
        inject_test_device(&dev3);
        let mut results = [CBleDeviceInfo::zeroed(); 10];
        let name = b"Thistle\0";
        let count = unsafe {
            rs_ble_scanner_find_by_name(name.as_ptr() as *const c_char, results.as_mut_ptr(), 10)
        };
        assert_eq!(count, 2);
    }

    #[test]
    fn test_find_by_name_case_insensitive() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([1, 0, 0, 0, 0, 0], "MyDevice", -40);
        inject_test_device(&dev);
        let mut results = [CBleDeviceInfo::zeroed(); 5];
        let name = b"mydevice\0";
        let count = unsafe {
            rs_ble_scanner_find_by_name(name.as_ptr() as *const c_char, results.as_mut_ptr(), 5)
        };
        assert_eq!(count, 1);
    }

    #[test]
    fn test_find_by_name_no_results() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([1, 0, 0, 0, 0, 0], "Something", -40);
        inject_test_device(&dev);
        let mut results = [CBleDeviceInfo::zeroed(); 5];
        let name = b"NoMatch\0";
        let count = unsafe {
            rs_ble_scanner_find_by_name(name.as_ptr() as *const c_char, results.as_mut_ptr(), 5)
        };
        assert_eq!(count, 0);
    }

    #[test]
    fn test_null_pointer_safety() {
        reset();
        rs_ble_scanner_init();
        // get_device with null out
        let rc = unsafe { rs_ble_scanner_get_device(0, std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
        // find_by_addr with null addr
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_find_by_addr(std::ptr::null(), &mut out) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
        // find_by_addr with null out
        let addr = [0u8; 6];
        let rc = unsafe { rs_ble_scanner_find_by_addr(addr.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
        // find_by_name with null name
        let mut results = [CBleDeviceInfo::zeroed(); 1];
        let rc = unsafe {
            rs_ble_scanner_find_by_name(std::ptr::null(), results.as_mut_ptr(), 1)
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
        // get_stats with null out
        let rc = unsafe { rs_ble_scanner_get_stats(std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Advertising data parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_adv_empty() {
        let mut dev = BleDevice::new();
        parse_adv_data(&[], &mut dev);
        assert_eq!(dev.name_len, 0);
        assert_eq!(dev.service_count, 0);
        assert_eq!(dev.mfg_len, 0);
    }

    #[test]
    fn test_parse_adv_name_only_short() {
        let mut dev = BleDevice::new();
        // AD: len=4, type=0x08 (shortened name), "abc"
        let data = [4, AD_TYPE_SHORT_NAME, b'a', b'b', b'c'];
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.name_len, 3);
        assert_eq!(&dev.name[..3], b"abc");
    }

    #[test]
    fn test_parse_adv_complete_name() {
        let mut dev = BleDevice::new();
        let data = [8, AD_TYPE_COMPLETE_NAME, b'T', b'h', b'i', b's', b't', b'l', b'e'];
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.name_len, 7);
        assert_eq!(&dev.name[..7], b"Thistle");
    }

    #[test]
    fn test_parse_adv_manufacturer_data() {
        let mut dev = BleDevice::new();
        // Company ID 0x004C (Apple) + payload [0x01, 0x02]
        let data = [5, AD_TYPE_MFG_DATA, 0x4C, 0x00, 0x01, 0x02];
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.company_id, 0x004C);
        assert_eq!(dev.mfg_len, 2);
        assert_eq!(&dev.mfg_data[..2], &[0x01, 0x02]);
    }

    #[test]
    fn test_parse_adv_16bit_uuid() {
        let mut dev = BleDevice::new();
        // 16-bit UUID: 0x180F (Battery Service)
        let data = [3, AD_TYPE_UUID16_COMPLETE, 0x0F, 0x18];
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.service_count, 1);
        // Check the 16-bit UUID was placed at bytes 12-13 of the 128-bit UUID
        assert_eq!(dev.services[0][12], 0x0F);
        assert_eq!(dev.services[0][13], 0x18);
    }

    #[test]
    fn test_parse_adv_128bit_uuid() {
        let mut dev = BleDevice::new();
        let uuid: [u8; 16] = [
            0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
            0x93, 0xF3, 0xA3, 0xB5, 0x01, 0x00, 0x40, 0x6E,
        ];
        let mut data = vec![17, AD_TYPE_UUID128_COMPLETE];
        data.extend_from_slice(&uuid);
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.service_count, 1);
        assert_eq!(dev.services[0], uuid);
    }

    #[test]
    fn test_parse_adv_multiple_fields() {
        let mut dev = BleDevice::new();
        let mut data = Vec::new();
        // Flags
        data.extend_from_slice(&[2, AD_TYPE_FLAGS, 0x06]);
        // Complete name "Hi"
        data.extend_from_slice(&[3, AD_TYPE_COMPLETE_NAME, b'H', b'i']);
        // Manufacturer data: company 0x0059, payload [0xAA]
        data.extend_from_slice(&[4, AD_TYPE_MFG_DATA, 0x59, 0x00, 0xAA]);
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.name_len, 2);
        assert_eq!(&dev.name[..2], b"Hi");
        assert_eq!(dev.company_id, 0x0059);
        assert_eq!(dev.mfg_len, 1);
        assert_eq!(dev.mfg_data[0], 0xAA);
    }

    #[test]
    fn test_parse_adv_truncated_field() {
        let mut dev = BleDevice::new();
        // Length says 10 bytes but only 3 bytes follow — should not panic
        let data = [10, AD_TYPE_COMPLETE_NAME, b'A', b'B'];
        parse_adv_data(&data, &mut dev);
        // Truncated field is skipped, name should be empty
        assert_eq!(dev.name_len, 0);
    }

    #[test]
    fn test_parse_adv_company_id() {
        let mut dev = BleDevice::new();
        // Company ID 0x0006 (Microsoft) with no payload
        let data = [3, AD_TYPE_MFG_DATA, 0x06, 0x00];
        parse_adv_data(&data, &mut dev);
        assert_eq!(dev.company_id, 0x0006);
        assert_eq!(dev.mfg_len, 0);
    }

    #[test]
    fn test_parse_adv_flags() {
        let mut dev = BleDevice::new();
        let data = [2, AD_TYPE_FLAGS, 0x06];
        parse_adv_data(&data, &mut dev);
        // Flags are parsed without error; no field to check but no panic
        assert_eq!(dev.name_len, 0);
    }

    // -----------------------------------------------------------------------
    // Filtering tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rssi_filter_rejects_weak() {
        reset();
        rs_ble_scanner_init();
        rs_ble_scanner_set_rssi_filter(-50);
        let dev = make_device([1, 0, 0, 0, 0, 0], "Weak", -80);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 0);
    }

    #[test]
    fn test_rssi_filter_accepts_strong() {
        reset();
        rs_ble_scanner_init();
        rs_ble_scanner_set_rssi_filter(-50);
        let dev = make_device([1, 0, 0, 0, 0, 0], "Strong", -30);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
    }

    #[test]
    fn test_name_prefix_filter() {
        reset();
        rs_ble_scanner_init();
        let prefix = b"Thistle\0";
        unsafe { rs_ble_scanner_set_name_filter(prefix.as_ptr() as *const c_char) };
        let dev1 = make_device([1, 0, 0, 0, 0, 0], "ThistleDevice", -40);
        let dev2 = make_device([2, 0, 0, 0, 0, 0], "OtherDevice", -40);
        inject_test_device(&dev1);
        inject_test_device(&dev2);
        assert_eq!(rs_ble_scanner_get_count(), 1);
    }

    #[test]
    fn test_clear_name_filter() {
        reset();
        rs_ble_scanner_init();
        let prefix = b"Thistle\0";
        unsafe { rs_ble_scanner_set_name_filter(prefix.as_ptr() as *const c_char) };
        // Clear filter
        unsafe { rs_ble_scanner_set_name_filter(std::ptr::null()) };
        let dev = make_device([1, 0, 0, 0, 0, 0], "AnyDevice", -40);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
    }

    #[test]
    fn test_filter_defaults() {
        reset();
        rs_ble_scanner_init();
        if let Ok(s) = SCANNER.lock() {
            assert_eq!(s.filter_rssi, -127);
            assert_eq!(s.filter_name_len, 0);
        }
    }

    // -----------------------------------------------------------------------
    // Sorting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sort_by_rssi() {
        reset();
        rs_ble_scanner_init();
        let dev1 = make_device([1, 0, 0, 0, 0, 0], "Weak", -80);
        let dev2 = make_device([2, 0, 0, 0, 0, 0], "Strong", -20);
        let dev3 = make_device([3, 0, 0, 0, 0, 0], "Medium", -50);
        inject_test_device(&dev1);
        inject_test_device(&dev2);
        inject_test_device(&dev3);
        assert_eq!(rs_ble_scanner_sort_by_rssi(), ESP_OK);
        // First device should be the strongest
        let mut out = CBleDeviceInfo::zeroed();
        unsafe { rs_ble_scanner_get_device(0, &mut out) };
        assert_eq!(out.rssi, -20);
        unsafe { rs_ble_scanner_get_device(1, &mut out) };
        assert_eq!(out.rssi, -50);
        unsafe { rs_ble_scanner_get_device(2, &mut out) };
        assert_eq!(out.rssi, -80);
    }

    #[test]
    fn test_sort_empty_list() {
        reset();
        rs_ble_scanner_init();
        assert_eq!(rs_ble_scanner_sort_by_rssi(), ESP_OK);
        assert_eq!(rs_ble_scanner_get_count(), 0);
    }

    #[test]
    fn test_sort_single_device() {
        reset();
        rs_ble_scanner_init();
        let dev = make_device([1, 0, 0, 0, 0, 0], "Solo", -42);
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_sort_by_rssi(), ESP_OK);
        let mut out = CBleDeviceInfo::zeroed();
        unsafe { rs_ble_scanner_get_device(0, &mut out) };
        assert_eq!(out.rssi, -42);
    }

    // -----------------------------------------------------------------------
    // Statistics tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_stats_when_empty() {
        reset();
        rs_ble_scanner_init();
        let mut stats = CBleScanStats {
            device_count: 99,
            total_adv_seen: 99,
            scanning: true,
            scan_type: 1,
            strongest_rssi: 0,
            weakest_rssi: 0,
        };
        let rc = unsafe { rs_ble_scanner_get_stats(&mut stats) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(stats.device_count, 0);
        assert_eq!(stats.total_adv_seen, 0);
        assert!(!stats.scanning);
        assert_eq!(stats.strongest_rssi, 0);
        assert_eq!(stats.weakest_rssi, 0);
    }

    #[test]
    fn test_stats_with_devices() {
        reset();
        rs_ble_scanner_init();
        let dev1 = make_device([1, 0, 0, 0, 0, 0], "A", -30);
        let dev2 = make_device([2, 0, 0, 0, 0, 0], "B", -70);
        inject_test_device(&dev1);
        inject_test_device(&dev2);
        let mut stats = CBleScanStats {
            device_count: 0,
            total_adv_seen: 0,
            scanning: false,
            scan_type: 0,
            strongest_rssi: 0,
            weakest_rssi: 0,
        };
        let rc = unsafe { rs_ble_scanner_get_stats(&mut stats) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(stats.device_count, 2);
        assert_eq!(stats.total_adv_seen, 2);
        assert_eq!(stats.strongest_rssi, -30);
        assert_eq!(stats.weakest_rssi, -70);
    }

    #[test]
    fn test_stats_null_pointer() {
        reset();
        let rc = unsafe { rs_ble_scanner_get_stats(std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_device_to_c_roundtrip() {
        let dev = make_device([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF], "RoundTrip", -42);
        let c_info = device_to_c(&dev);
        assert_eq!(c_info.addr, dev.addr);
        assert_eq!(c_info.rssi, dev.rssi);
        assert_eq!(c_info.name_len, dev.name_len as u8);
        assert_eq!(&c_info.name[..dev.name_len], &dev.name[..dev.name_len]);
        assert_eq!(c_info.connectable, dev.connectable);
    }

    #[test]
    fn test_addr_comparison() {
        reset();
        rs_ble_scanner_init();
        let addr1 = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let addr2 = [0x11, 0x22, 0x33, 0x44, 0x55, 0x67]; // differs in last byte
        let dev1 = make_device(addr1, "Dev1", -40);
        let dev2 = make_device(addr2, "Dev2", -50);
        inject_test_device(&dev1);
        inject_test_device(&dev2);
        assert_eq!(rs_ble_scanner_get_count(), 2);
        // Each is findable by its own address
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_find_by_addr(addr1.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.addr, addr1);
        let rc = unsafe { rs_ble_scanner_find_by_addr(addr2.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.addr, addr2);
    }

    #[test]
    fn test_inject_at_capacity_evicts_weakest() {
        reset();
        rs_ble_scanner_init();
        // Fill to capacity
        for i in 0..MAX_DEVICES {
            let addr = [(i & 0xFF) as u8, ((i >> 8) & 0xFF) as u8, 0, 0, 0, 0];
            let rssi = -50i8; // All at -50
            let dev = make_device(addr, "D", rssi);
            inject_test_device(&dev);
        }
        assert_eq!(rs_ble_scanner_get_count(), MAX_DEVICES as i32);

        // Make device at index 0 the weakest
        {
            let mut s = SCANNER.lock().unwrap();
            if let Some(ref mut d) = s.devices[0] {
                d.rssi = -100;
            }
        }

        // Inject one more — should evict the weakest (-100)
        let new_addr = [0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA];
        let new_dev = make_device(new_addr, "New", -30);
        inject_test_device(&new_dev);
        assert_eq!(rs_ble_scanner_get_count(), MAX_DEVICES as i32);

        // The new device should be findable
        let mut out = CBleDeviceInfo::zeroed();
        let rc = unsafe { rs_ble_scanner_find_by_addr(new_addr.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_OK);
        assert_eq!(out.rssi, -30);

        // The old weakest should be gone
        let old_addr = [0u8, 0, 0, 0, 0, 0];
        let rc = unsafe { rs_ble_scanner_find_by_addr(old_addr.as_ptr(), &mut out) };
        assert_eq!(rc, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_name_truncation() {
        reset();
        rs_ble_scanner_init();
        // Name longer than MAX_NAME_LEN
        let long_name = "A".repeat(50);
        let mut dev = BleDevice::new();
        dev.addr = [1, 0, 0, 0, 0, 0];
        dev.rssi = -40;
        let bytes = long_name.as_bytes();
        let len = bytes.len().min(MAX_NAME_LEN);
        dev.name[..len].copy_from_slice(&bytes[..len]);
        dev.name_len = len;
        dev.seen_count = 1;
        inject_test_device(&dev);
        assert_eq!(rs_ble_scanner_get_count(), 1);
        let mut out = CBleDeviceInfo::zeroed();
        unsafe { rs_ble_scanner_get_device(0, &mut out) };
        assert_eq!(out.name_len, MAX_NAME_LEN as u8);
    }
}
