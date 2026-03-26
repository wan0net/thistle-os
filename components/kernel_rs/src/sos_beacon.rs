// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SOS emergency beacon
//
// Manages emergency distress signaling over LoRa. Produces encoded beacon
// packets that the caller transmits via the radio HAL. The module itself
// does NOT call the radio HAL directly (except in test builds for coverage).

use std::os::raw::c_char;

use crate::hal_registry::HalGpsPosition;

// ── ESP error codes ──────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Constants ────────────────────────────────────────────────────────

/// Magic bytes identifying an SOS packet.
const SOS_MAGIC: [u8; 4] = *b"SOS!";

/// Protocol version.
const SOS_PROTOCOL_VERSION: u8 = 1;

/// Maximum length of the optional text message.
const SOS_MESSAGE_MAX: usize = 64;

/// Total serialized packet size in bytes.
/// 4 (magic) + 1 (version) + 2 (sequence) + 8 (device_id) + 4 (timestamp)
/// + 8 (latitude) + 8 (longitude) + 4 (altitude_m) + 1 (satellites)
/// + 1 (battery_pct) + 1 (status) + 1 (message_len) + 64 (message)
/// + 2 (checksum) = 109
pub const SOS_PACKET_SIZE: usize = 109;

// ── SosStatus ────────────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SosStatus {
    Active = 0,
    Moving = 1,
    Immobile = 2,
    Medical = 3,
    Cancel = 4,
    Test = 5,
}

impl SosStatus {
    /// Convert from raw u8. Returns None for unknown values.
    pub fn from_u8(v: u8) -> Option<SosStatus> {
        match v {
            0 => Some(SosStatus::Active),
            1 => Some(SosStatus::Moving),
            2 => Some(SosStatus::Immobile),
            3 => Some(SosStatus::Medical),
            4 => Some(SosStatus::Cancel),
            5 => Some(SosStatus::Test),
            _ => None,
        }
    }
}

// ── CRC-16/CCITT-FALSE ──────────────────────────────────────────────

/// Compute CRC-16/CCITT-FALSE over the given data.
///
/// Polynomial: 0x1021, initial value: 0xFFFF, no final XOR.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ── SosMessage ───────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct SosMessage {
    pub magic: [u8; 4],
    pub version: u8,
    pub sequence: u16,
    pub device_id: [u8; 8],
    pub timestamp: u32,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: f32,
    pub satellites: u8,
    pub battery_pct: u8,
    pub status: SosStatus,
    pub message_len: u8,
    pub message: [u8; SOS_MESSAGE_MAX],
    pub checksum: u16,
}

impl SosMessage {
    /// Serialize all fields to a big-endian byte vector.
    ///
    /// The checksum is computed over all preceding fields before appending.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(SOS_PACKET_SIZE);

        // magic (4)
        buf.extend_from_slice(&self.magic);
        // version (1)
        buf.push(self.version);
        // sequence (2)
        buf.extend_from_slice(&self.sequence.to_be_bytes());
        // device_id (8)
        buf.extend_from_slice(&self.device_id);
        // timestamp (4)
        buf.extend_from_slice(&self.timestamp.to_be_bytes());
        // latitude (8)
        buf.extend_from_slice(&self.latitude.to_be_bytes());
        // longitude (8)
        buf.extend_from_slice(&self.longitude.to_be_bytes());
        // altitude_m (4)
        buf.extend_from_slice(&self.altitude_m.to_be_bytes());
        // satellites (1)
        buf.push(self.satellites);
        // battery_pct (1)
        buf.push(self.battery_pct);
        // status (1)
        buf.push(self.status as u8);
        // message_len (1)
        buf.push(self.message_len);
        // message (64)
        buf.extend_from_slice(&self.message);

        // checksum — CRC-16/CCITT over everything preceding
        let crc = crc16_ccitt(&buf);
        buf.extend_from_slice(&crc.to_be_bytes());

        debug_assert_eq!(buf.len(), SOS_PACKET_SIZE);
        buf
    }

    /// Deserialize from big-endian bytes. Verifies magic and checksum.
    pub fn from_bytes(data: &[u8]) -> Result<SosMessage, i32> {
        if data.len() < SOS_PACKET_SIZE {
            return Err(ESP_ERR_INVALID_ARG);
        }

        // Verify magic
        if &data[0..4] != &SOS_MAGIC {
            return Err(ESP_ERR_INVALID_ARG);
        }

        // Verify checksum: CRC over all fields except the last 2 bytes
        let payload = &data[..SOS_PACKET_SIZE - 2];
        let expected_crc = crc16_ccitt(payload);
        let stored_crc = u16::from_be_bytes([
            data[SOS_PACKET_SIZE - 2],
            data[SOS_PACKET_SIZE - 1],
        ]);
        if expected_crc != stored_crc {
            return Err(ESP_FAIL);
        }

        let mut off = 0usize;

        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[off..off + 4]);
        off += 4;

        let version = data[off];
        off += 1;

        let sequence = u16::from_be_bytes([data[off], data[off + 1]]);
        off += 2;

        let mut device_id = [0u8; 8];
        device_id.copy_from_slice(&data[off..off + 8]);
        off += 8;

        let timestamp = u32::from_be_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
        ]);
        off += 4;

        let latitude = f64::from_be_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
            data[off + 4], data[off + 5], data[off + 6], data[off + 7],
        ]);
        off += 8;

        let longitude = f64::from_be_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
            data[off + 4], data[off + 5], data[off + 6], data[off + 7],
        ]);
        off += 8;

        let altitude_m = f32::from_be_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
        ]);
        off += 4;

        let satellites = data[off];
        off += 1;

        let battery_pct = data[off];
        off += 1;

        let status_raw = data[off];
        off += 1;
        let status = SosStatus::from_u8(status_raw).ok_or(ESP_ERR_INVALID_ARG)?;

        let message_len = data[off];
        off += 1;

        let mut message = [0u8; SOS_MESSAGE_MAX];
        message.copy_from_slice(&data[off..off + SOS_MESSAGE_MAX]);
        off += SOS_MESSAGE_MAX;

        let checksum = u16::from_be_bytes([data[off], data[off + 1]]);

        Ok(SosMessage {
            magic,
            version,
            sequence,
            device_id,
            timestamp,
            latitude,
            longitude,
            altitude_m,
            satellites,
            battery_pct,
            status,
            message_len,
            message,
            checksum,
        })
    }
}

// ── SosBeacon ────────────────────────────────────────────────────────

pub struct SosBeacon {
    device_id: [u8; 8],
    active: bool,
    status: Option<SosStatus>,
    sequence: u16,
    packets_sent: u32,
    latitude: f64,
    longitude: f64,
    altitude_m: f32,
    satellites: u8,
    battery_pct: u8,
    timestamp: u32,
    message: [u8; SOS_MESSAGE_MAX],
    message_len: u8,
    activation_timestamp: Option<u32>,
}

impl SosBeacon {
    /// Create a new SOS beacon with the given device identifier.
    pub fn new(device_id: [u8; 8]) -> SosBeacon {
        SosBeacon {
            device_id,
            active: false,
            status: None,
            sequence: 0,
            packets_sent: 0,
            latitude: 0.0,
            longitude: 0.0,
            altitude_m: 0.0,
            satellites: 0,
            battery_pct: 0,
            timestamp: 0,
            message: [0u8; SOS_MESSAGE_MAX],
            message_len: 0,
            activation_timestamp: None,
        }
    }

    /// Activate the SOS beacon with the given status and optional message.
    ///
    /// Returns `ESP_ERR_INVALID_STATE` if already active (unless the new
    /// status is `Cancel`, which goes through `cancel()`).
    pub fn activate(&mut self, status: SosStatus, message: Option<&str>) -> Result<(), i32> {
        if self.active && status != SosStatus::Cancel {
            return Err(ESP_ERR_INVALID_STATE);
        }
        if status == SosStatus::Cancel {
            return self.cancel();
        }

        self.active = true;
        self.status = Some(status);
        self.sequence = 0;
        self.activation_timestamp = Some(self.timestamp);

        if let Some(msg) = message {
            self.set_message(msg);
        } else {
            self.message = [0u8; SOS_MESSAGE_MAX];
            self.message_len = 0;
        }

        Ok(())
    }

    /// Cancel the SOS and deactivate.
    ///
    /// Sets status to Cancel so the next `next_packet()` call emits a cancel
    /// frame, then deactivates after encoding.
    pub fn cancel(&mut self) -> Result<(), i32> {
        if !self.active {
            return Err(ESP_ERR_INVALID_STATE);
        }
        self.status = Some(SosStatus::Cancel);
        // Stay "active" long enough for one cancel packet, then deactivate
        // The caller should call next_packet() to emit the cancel, after
        // which we deactivate.
        Ok(())
    }

    /// Whether the beacon is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Current SOS status, if active.
    pub fn status(&self) -> Option<SosStatus> {
        if self.active {
            self.status
        } else {
            None
        }
    }

    /// Update the latest GPS fix.
    pub fn update_position(&mut self, pos: &HalGpsPosition) {
        self.latitude = pos.latitude;
        self.longitude = pos.longitude;
        self.altitude_m = pos.altitude_m;
        self.satellites = pos.satellites;
        self.timestamp = pos.timestamp;
    }

    /// Update the battery level (0-100).
    pub fn update_battery(&mut self, pct: u8) {
        self.battery_pct = if pct > 100 { 100 } else { pct };
    }

    /// Set the optional text message. Truncated to 64 bytes.
    pub fn set_message(&mut self, msg: &str) {
        let bytes = msg.as_bytes();
        let len = bytes.len().min(SOS_MESSAGE_MAX);
        self.message = [0u8; SOS_MESSAGE_MAX];
        self.message[..len].copy_from_slice(&bytes[..len]);
        self.message_len = len as u8;
    }

    /// Encode the next SOS packet. Returns `None` if not active.
    ///
    /// Increments the sequence number and packets_sent counter.
    /// If the current status is `Cancel`, deactivates after encoding.
    pub fn next_packet(&mut self) -> Option<Vec<u8>> {
        if !self.active {
            return None;
        }

        let msg = SosMessage {
            magic: SOS_MAGIC,
            version: SOS_PROTOCOL_VERSION,
            sequence: self.sequence,
            device_id: self.device_id,
            timestamp: self.timestamp,
            latitude: self.latitude,
            longitude: self.longitude,
            altitude_m: self.altitude_m,
            satellites: self.satellites,
            battery_pct: self.battery_pct,
            status: self.status.unwrap_or(SosStatus::Active),
            message_len: self.message_len,
            message: self.message,
            checksum: 0, // computed inside to_bytes()
        };

        let bytes = msg.to_bytes();
        self.sequence = self.sequence.wrapping_add(1);
        self.packets_sent += 1;

        // Deactivate after emitting a cancel packet
        if self.status == Some(SosStatus::Cancel) {
            self.active = false;
            self.status = None;
            self.activation_timestamp = None;
        }

        Some(bytes)
    }

    /// Current sequence number.
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    /// Total number of packets generated.
    pub fn packets_sent(&self) -> u32 {
        self.packets_sent
    }

    /// Recommended interval in seconds between transmissions.
    pub fn interval_seconds(&self) -> u32 {
        match self.status {
            Some(SosStatus::Active) | Some(SosStatus::Medical) => 30,
            Some(SosStatus::Moving) => 60,
            Some(SosStatus::Immobile) => 120,
            Some(SosStatus::Cancel) | Some(SosStatus::Test) => 10,
            None => 0,
        }
    }

    /// Seconds since first activation, based on packet timestamps.
    ///
    /// Returns `None` if not active or no activation timestamp recorded.
    pub fn elapsed_since_activation(&self) -> Option<u32> {
        if !self.active {
            return None;
        }
        self.activation_timestamp.map(|act_ts| {
            self.timestamp.saturating_sub(act_ts)
        })
    }

    /// Device identifier.
    pub fn device_id(&self) -> &[u8; 8] {
        &self.device_id
    }
}

// ── C FFI exports ────────────────────────────────────────────────────

/// Create a new SOS beacon. `device_id` must point to 8 bytes.
///
/// # Safety
/// `device_id` must be a valid pointer to at least 8 bytes.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_create(device_id: *const u8) -> *mut SosBeacon {
    if device_id.is_null() {
        return std::ptr::null_mut();
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(std::slice::from_raw_parts(device_id, 8));
    let beacon = Box::new(SosBeacon::new(id));
    Box::into_raw(beacon)
}

/// Destroy an SOS beacon.
///
/// # Safety
/// `beacon` must have been created by `rs_sos_beacon_create` and not
/// previously destroyed.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_destroy(beacon: *mut SosBeacon) {
    if !beacon.is_null() {
        drop(Box::from_raw(beacon));
    }
}

/// Activate the SOS beacon. `status` is a `SosStatus` variant (0-5).
/// `msg` may be NULL.
///
/// # Safety
/// `beacon` must be a valid pointer. `msg` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_activate(
    beacon: *mut SosBeacon,
    status: u8,
    msg: *const c_char,
) -> i32 {
    if beacon.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let sos_status = match SosStatus::from_u8(status) {
        Some(s) => s,
        None => return ESP_ERR_INVALID_ARG,
    };
    let message = if msg.is_null() {
        None
    } else {
        match std::ffi::CStr::from_ptr(msg).to_str() {
            Ok(s) => Some(s),
            Err(_) => None,
        }
    };
    match (*beacon).activate(sos_status, message) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

/// Cancel the SOS beacon.
///
/// # Safety
/// `beacon` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_cancel(beacon: *mut SosBeacon) -> i32 {
    if beacon.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match (*beacon).cancel() {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

/// Check if the SOS beacon is active.
///
/// Returns 1 if active, 0 if inactive, negative on error.
///
/// # Safety
/// `beacon` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_is_active(beacon: *const SosBeacon) -> i32 {
    if beacon.is_null() {
        return ESP_FAIL;
    }
    if (*beacon).is_active() { 1 } else { 0 }
}

/// Update GPS position.
///
/// # Safety
/// `beacon` and `pos` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_update_position(
    beacon: *mut SosBeacon,
    pos: *const HalGpsPosition,
) -> i32 {
    if beacon.is_null() || pos.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    (*beacon).update_position(&*pos);
    ESP_OK
}

/// Update battery percentage.
///
/// # Safety
/// `beacon` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_update_battery(
    beacon: *mut SosBeacon,
    pct: u8,
) -> i32 {
    if beacon.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    (*beacon).update_battery(pct);
    ESP_OK
}

/// Encode the next SOS packet into `buf`. Returns bytes written (always
/// `SOS_PACKET_SIZE` on success), 0 if not active, or negative error.
///
/// # Safety
/// `beacon` must be valid. `buf` must point to at least `buf_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_next_packet(
    beacon: *mut SosBeacon,
    buf: *mut u8,
    buf_len: usize,
) -> i32 {
    if beacon.is_null() || buf.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if buf_len < SOS_PACKET_SIZE {
        return ESP_ERR_INVALID_ARG;
    }
    match (*beacon).next_packet() {
        Some(packet) => {
            std::ptr::copy_nonoverlapping(packet.as_ptr(), buf, SOS_PACKET_SIZE);
            SOS_PACKET_SIZE as i32
        }
        None => 0,
    }
}

/// Return recommended interval in seconds between transmissions.
///
/// # Safety
/// `beacon` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn rs_sos_beacon_interval(beacon: *const SosBeacon) -> u32 {
    if beacon.is_null() {
        return 0;
    }
    (*beacon).interval_seconds()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device_id() -> [u8; 8] {
        [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
    }

    fn make_gps_position(lat: f64, lon: f64, alt: f32, sats: u8, ts: u32) -> HalGpsPosition {
        HalGpsPosition {
            latitude: lat,
            longitude: lon,
            altitude_m: alt,
            speed_kmh: 0.0,
            heading_deg: 0.0,
            satellites: sats,
            fix_valid: true,
            timestamp: ts,
        }
    }

    // ── Basic state tests ────────────────────────────────────────────

    #[test]
    fn test_create_beacon_inactive() {
        let beacon = SosBeacon::new(make_device_id());
        assert!(!beacon.is_active());
        assert!(beacon.status().is_none());
        assert_eq!(beacon.sequence(), 0);
        assert_eq!(beacon.packets_sent(), 0);
        assert_eq!(beacon.device_id(), &make_device_id());
    }

    #[test]
    fn test_activate_active_status() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.activate(SosStatus::Active, None).is_ok());
        assert!(beacon.is_active());
        assert_eq!(beacon.status(), Some(SosStatus::Active));
    }

    #[test]
    fn test_activate_moving_status() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.activate(SosStatus::Moving, None).is_ok());
        assert!(beacon.is_active());
        assert_eq!(beacon.status(), Some(SosStatus::Moving));
    }

    #[test]
    fn test_activate_immobile_status() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.activate(SosStatus::Immobile, None).is_ok());
        assert!(beacon.is_active());
        assert_eq!(beacon.status(), Some(SosStatus::Immobile));
    }

    #[test]
    fn test_activate_medical_status() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.activate(SosStatus::Medical, None).is_ok());
        assert!(beacon.is_active());
        assert_eq!(beacon.status(), Some(SosStatus::Medical));
    }

    #[test]
    fn test_activate_test_status() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.activate(SosStatus::Test, None).is_ok());
        assert!(beacon.is_active());
        assert_eq!(beacon.status(), Some(SosStatus::Test));
    }

    #[test]
    fn test_cancel_after_activation() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        assert!(beacon.cancel().is_ok());
        assert_eq!(beacon.status(), Some(SosStatus::Cancel));
        // Still active until cancel packet is emitted
        assert!(beacon.is_active());
        // Emit the cancel packet
        let pkt = beacon.next_packet();
        assert!(pkt.is_some());
        // Now deactivated
        assert!(!beacon.is_active());
        assert!(beacon.status().is_none());
    }

    #[test]
    fn test_double_activate_error() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        let result = beacon.activate(SosStatus::Medical, None);
        assert_eq!(result, Err(ESP_ERR_INVALID_STATE));
    }

    #[test]
    fn test_cancel_when_not_active_error() {
        let mut beacon = SosBeacon::new(make_device_id());
        let result = beacon.cancel();
        assert_eq!(result, Err(ESP_ERR_INVALID_STATE));
    }

    // ── Position and battery tests ───────────────────────────────────

    #[test]
    fn test_update_position_in_packet() {
        let mut beacon = SosBeacon::new(make_device_id());
        let pos = make_gps_position(55.9533, -3.1883, 47.0, 12, 1000);
        beacon.update_position(&pos);
        beacon.activate(SosStatus::Active, None).unwrap();

        let pkt = beacon.next_packet().unwrap();
        let msg = SosMessage::from_bytes(&pkt).unwrap();
        assert!((msg.latitude - 55.9533).abs() < 1e-10);
        assert!((msg.longitude - (-3.1883)).abs() < 1e-10);
        assert!((msg.altitude_m - 47.0).abs() < 1e-5);
        assert_eq!(msg.satellites, 12);
        assert_eq!(msg.timestamp, 1000);
    }

    #[test]
    fn test_update_battery() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.update_battery(85);
        beacon.activate(SosStatus::Active, None).unwrap();

        let pkt = beacon.next_packet().unwrap();
        let msg = SosMessage::from_bytes(&pkt).unwrap();
        assert_eq!(msg.battery_pct, 85);
    }

    #[test]
    fn test_battery_clamped_to_100() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.update_battery(200);
        assert_eq!(beacon.battery_pct, 100);
    }

    // ── Message tests ────────────────────────────────────────────────

    #[test]
    fn test_set_message() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, Some("Help me!")).unwrap();

        let pkt = beacon.next_packet().unwrap();
        let msg = SosMessage::from_bytes(&pkt).unwrap();
        assert_eq!(msg.message_len, 8);
        assert_eq!(&msg.message[..8], b"Help me!");
        // rest is zero-padded
        assert!(msg.message[8..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_set_message_truncated_at_64_bytes() {
        let mut beacon = SosBeacon::new(make_device_id());
        let long_msg = "A".repeat(100);
        beacon.set_message(&long_msg);
        assert_eq!(beacon.message_len, 64);
        assert!(beacon.message.iter().all(|&b| b == b'A'));
    }

    // ── Packet generation tests ──────────────────────────────────────

    #[test]
    fn test_next_packet_none_when_inactive() {
        let mut beacon = SosBeacon::new(make_device_id());
        assert!(beacon.next_packet().is_none());
    }

    #[test]
    fn test_packet_sequence_increments() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();

        let pkt0 = beacon.next_packet().unwrap();
        let msg0 = SosMessage::from_bytes(&pkt0).unwrap();
        assert_eq!(msg0.sequence, 0);
        assert_eq!(beacon.sequence(), 1);

        let pkt1 = beacon.next_packet().unwrap();
        let msg1 = SosMessage::from_bytes(&pkt1).unwrap();
        assert_eq!(msg1.sequence, 1);
        assert_eq!(beacon.sequence(), 2);
    }

    #[test]
    fn test_packets_sent_counter() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        assert_eq!(beacon.packets_sent(), 0);
        beacon.next_packet();
        assert_eq!(beacon.packets_sent(), 1);
        beacon.next_packet();
        assert_eq!(beacon.packets_sent(), 2);
        beacon.next_packet();
        assert_eq!(beacon.packets_sent(), 3);
    }

    // ── Interval tests ───────────────────────────────────────────────

    #[test]
    fn test_interval_active() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        assert_eq!(beacon.interval_seconds(), 30);
    }

    #[test]
    fn test_interval_medical() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Medical, None).unwrap();
        assert_eq!(beacon.interval_seconds(), 30);
    }

    #[test]
    fn test_interval_moving() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Moving, None).unwrap();
        assert_eq!(beacon.interval_seconds(), 60);
    }

    #[test]
    fn test_interval_immobile() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Immobile, None).unwrap();
        assert_eq!(beacon.interval_seconds(), 120);
    }

    #[test]
    fn test_interval_cancel() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        beacon.cancel().unwrap();
        assert_eq!(beacon.interval_seconds(), 10);
    }

    #[test]
    fn test_interval_test() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Test, None).unwrap();
        assert_eq!(beacon.interval_seconds(), 10);
    }

    // ── Elapsed since activation ─────────────────────────────────────

    #[test]
    fn test_elapsed_since_activation() {
        let mut beacon = SosBeacon::new(make_device_id());
        let pos = make_gps_position(0.0, 0.0, 0.0, 0, 1000);
        beacon.update_position(&pos);
        beacon.activate(SosStatus::Active, None).unwrap();

        // Time hasn't advanced yet
        assert_eq!(beacon.elapsed_since_activation(), Some(0));

        // Advance timestamp via GPS update
        let pos2 = make_gps_position(0.0, 0.0, 0.0, 0, 1045);
        beacon.update_position(&pos2);
        assert_eq!(beacon.elapsed_since_activation(), Some(45));
    }

    #[test]
    fn test_elapsed_none_when_inactive() {
        let beacon = SosBeacon::new(make_device_id());
        assert!(beacon.elapsed_since_activation().is_none());
    }

    // ── Serialization roundtrip ──────────────────────────────────────

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut beacon = SosBeacon::new(make_device_id());
        let pos = make_gps_position(55.9533, -3.1883, 47.0, 12, 1711483200);
        beacon.update_position(&pos);
        beacon.update_battery(72);
        beacon.activate(SosStatus::Medical, Some("Broken leg")).unwrap();

        let pkt = beacon.next_packet().unwrap();
        assert_eq!(pkt.len(), SOS_PACKET_SIZE);

        let msg = SosMessage::from_bytes(&pkt).unwrap();
        assert_eq!(msg.magic, SOS_MAGIC);
        assert_eq!(msg.version, SOS_PROTOCOL_VERSION);
        assert_eq!(msg.sequence, 0);
        assert_eq!(msg.device_id, make_device_id());
        assert_eq!(msg.timestamp, 1711483200);
        assert!((msg.latitude - 55.9533).abs() < 1e-10);
        assert!((msg.longitude - (-3.1883)).abs() < 1e-10);
        assert!((msg.altitude_m - 47.0).abs() < 1e-5);
        assert_eq!(msg.satellites, 12);
        assert_eq!(msg.battery_pct, 72);
        assert_eq!(msg.status, SosStatus::Medical);
        assert_eq!(msg.message_len, 10);
        assert_eq!(&msg.message[..10], b"Broken leg");
    }

    #[test]
    fn test_deserialize_bad_magic() {
        let mut data = vec![0u8; SOS_PACKET_SIZE];
        data[0..4].copy_from_slice(b"BAD!");
        let result = SosMessage::from_bytes(&data);
        assert_eq!(result, Err(ESP_ERR_INVALID_ARG));
    }

    #[test]
    fn test_deserialize_bad_checksum() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        let mut pkt = beacon.next_packet().unwrap();
        // Corrupt the checksum (last 2 bytes)
        let len = pkt.len();
        pkt[len - 1] ^= 0xFF;
        let result = SosMessage::from_bytes(&pkt);
        assert_eq!(result, Err(ESP_FAIL));
    }

    #[test]
    fn test_deserialize_truncated_data() {
        let data = vec![0u8; 50]; // too short
        let result = SosMessage::from_bytes(&data);
        assert_eq!(result, Err(ESP_ERR_INVALID_ARG));
    }

    // ── CRC-16/CCITT tests ───────────────────────────────────────────

    #[test]
    fn test_crc16_empty() {
        assert_eq!(crc16_ccitt(b""), 0xFFFF);
    }

    #[test]
    fn test_crc16_known_vector() {
        // Standard CRC-16/CCITT-FALSE test vector
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    // ── Packet size consistency ──────────────────────────────────────

    #[test]
    fn test_packet_size_consistent() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, None).unwrap();
        let pkt = beacon.next_packet().unwrap();
        assert_eq!(pkt.len(), SOS_PACKET_SIZE);
        assert_eq!(SOS_PACKET_SIZE, 109);
    }

    #[test]
    fn test_packet_size_with_message() {
        let mut beacon = SosBeacon::new(make_device_id());
        beacon.activate(SosStatus::Active, Some("Hello world")).unwrap();
        let pkt = beacon.next_packet().unwrap();
        assert_eq!(pkt.len(), SOS_PACKET_SIZE);
    }

    // ── FFI tests ────────────────────────────────────────────────────

    #[test]
    fn test_ffi_create_destroy() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        assert!(!beacon.is_null());
        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_create_null_pointer() {
        let beacon = unsafe { rs_sos_beacon_create(std::ptr::null()) };
        assert!(beacon.is_null());
    }

    #[test]
    fn test_ffi_destroy_null_is_safe() {
        unsafe { rs_sos_beacon_destroy(std::ptr::null_mut()) };
        // Should not crash
    }

    #[test]
    fn test_ffi_is_active_null() {
        let result = unsafe { rs_sos_beacon_is_active(std::ptr::null()) };
        assert!(result < 0); // ESP_FAIL (-1)
    }

    #[test]
    fn test_ffi_activate_cancel() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        assert!(!beacon.is_null());

        // Activate
        let rc = unsafe {
            rs_sos_beacon_activate(beacon, SosStatus::Active as u8, std::ptr::null())
        };
        assert_eq!(rc, ESP_OK);
        assert_eq!(unsafe { rs_sos_beacon_is_active(beacon) }, 1);

        // Cancel
        let rc = unsafe { rs_sos_beacon_cancel(beacon) };
        assert_eq!(rc, ESP_OK);

        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_activate_null_beacon() {
        let rc = unsafe {
            rs_sos_beacon_activate(std::ptr::null_mut(), 0, std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_cancel_null_beacon() {
        let rc = unsafe { rs_sos_beacon_cancel(std::ptr::null_mut()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_activate_invalid_status() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        let rc = unsafe { rs_sos_beacon_activate(beacon, 99, std::ptr::null()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_next_packet() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };

        // Not active — should return 0
        let mut buf = [0u8; 256];
        let rc = unsafe { rs_sos_beacon_next_packet(beacon, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(rc, 0);

        // Activate
        unsafe { rs_sos_beacon_activate(beacon, SosStatus::Active as u8, std::ptr::null()) };

        // Now should return packet
        let rc = unsafe { rs_sos_beacon_next_packet(beacon, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(rc, SOS_PACKET_SIZE as i32);

        // Verify the packet is valid
        let msg = SosMessage::from_bytes(&buf[..SOS_PACKET_SIZE]).unwrap();
        assert_eq!(msg.magic, SOS_MAGIC);
        assert_eq!(msg.device_id, id);

        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_next_packet_buffer_too_small() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        unsafe { rs_sos_beacon_activate(beacon, SosStatus::Active as u8, std::ptr::null()) };

        let mut buf = [0u8; 10]; // too small
        let rc = unsafe { rs_sos_beacon_next_packet(beacon, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);

        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_update_position_null() {
        let rc = unsafe {
            rs_sos_beacon_update_position(std::ptr::null_mut(), std::ptr::null())
        };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_update_battery_null() {
        let rc = unsafe { rs_sos_beacon_update_battery(std::ptr::null_mut(), 50) };
        assert_eq!(rc, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_ffi_interval() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        unsafe { rs_sos_beacon_activate(beacon, SosStatus::Active as u8, std::ptr::null()) };
        assert_eq!(unsafe { rs_sos_beacon_interval(beacon) }, 30);
        unsafe { rs_sos_beacon_destroy(beacon) };
    }

    #[test]
    fn test_ffi_interval_null() {
        assert_eq!(unsafe { rs_sos_beacon_interval(std::ptr::null()) }, 0);
    }

    #[test]
    fn test_ffi_activate_with_message() {
        let id = make_device_id();
        let beacon = unsafe { rs_sos_beacon_create(id.as_ptr()) };
        let msg = std::ffi::CString::new("Help!").unwrap();
        let rc = unsafe {
            rs_sos_beacon_activate(beacon, SosStatus::Medical as u8, msg.as_ptr())
        };
        assert_eq!(rc, ESP_OK);

        let mut buf = [0u8; 256];
        let rc = unsafe { rs_sos_beacon_next_packet(beacon, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(rc, SOS_PACKET_SIZE as i32);

        let parsed = SosMessage::from_bytes(&buf[..SOS_PACKET_SIZE]).unwrap();
        assert_eq!(parsed.status, SosStatus::Medical);
        assert_eq!(parsed.message_len, 5);
        assert_eq!(&parsed.message[..5], b"Help!");

        unsafe { rs_sos_beacon_destroy(beacon) };
    }
}
