// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — permissions subsystem
//
// Manages per-app permission grants using a fixed-size slot array.
// No heap allocation for the slot table itself; all storage is static.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Permission flags — must match the C constants in permissions.h
// ---------------------------------------------------------------------------

pub const PERM_RADIO:   u32 = 1 << 0; // 0x01
pub const PERM_GPS:     u32 = 1 << 1; // 0x02
pub const PERM_STORAGE: u32 = 1 << 2; // 0x04
pub const PERM_NETWORK: u32 = 1 << 3; // 0x08
pub const PERM_AUDIO:   u32 = 1 << 4; // 0x10
pub const PERM_SYSTEM:  u32 = 1 << 5; // 0x20
pub const PERM_IPC:     u32 = 1 << 6; // 0x40
pub const PERM_ALL:     u32 = 0x7F;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK:                i32 = 0x000;
const ESP_ERR_NO_MEM:        i32 = 0x101;
const ESP_ERR_INVALID_ARG:   i32 = 0x102;
const ESP_ERR_NOT_FOUND:     i32 = 0x105;
/// ESP_ERR_NOT_ALLOWED = ESP_ERR_INVALID_STATE (0x103) + 0x100
#[allow(dead_code)]
const ESP_ERR_NOT_ALLOWED:   i32 = 0x203;

// ---------------------------------------------------------------------------
// Slot table
// ---------------------------------------------------------------------------

pub const MAX_APPS: usize = 16;

/// Maximum length of an app_id string stored in a slot (including null).
const APP_ID_LEN: usize = 64;

/// A single slot in the permissions table.
#[derive(Copy, Clone)]
struct AppPerms {
    /// Null-terminated app identifier. Empty first byte means the slot is free.
    id: [u8; APP_ID_LEN],
    /// Bitfield of granted permissions.
    perms: u32,
}

impl AppPerms {
    const fn empty() -> Self {
        AppPerms {
            id: [0u8; APP_ID_LEN],
            perms: 0,
        }
    }

    fn is_free(&self) -> bool {
        self.id[0] == 0
    }

    fn matches(&self, app_id: &str) -> bool {
        if self.is_free() {
            return false;
        }
        // Compare the stored id (null-terminated) against app_id.
        let stored = match std::str::from_utf8(&self.id) {
            Ok(s) => s.trim_end_matches('\0'),
            Err(_) => return false,
        };
        stored == app_id
    }

    fn set_id(&mut self, app_id: &str) {
        self.id = [0u8; APP_ID_LEN];
        let bytes = app_id.as_bytes();
        let len = bytes.len().min(APP_ID_LEN - 1);
        self.id[..len].copy_from_slice(&bytes[..len]);
        // Remaining bytes are already zeroed above.
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static PERM_TABLE: Mutex<[AppPerms; MAX_APPS]> = Mutex::new(
    [AppPerms {
        id: [0u8; APP_ID_LEN],
        perms: 0,
    }; MAX_APPS],
);

// ---------------------------------------------------------------------------
// Public Rust API
// ---------------------------------------------------------------------------

/// Initialise the permissions subsystem.
///
/// Clears all slots. Safe to call multiple times (idempotent).
pub fn init() -> i32 {
    match PERM_TABLE.lock() {
        Ok(mut table) => {
            for slot in table.iter_mut() {
                *slot = AppPerms::empty();
            }
            ESP_OK
        }
        Err(_) => ESP_ERR_INVALID_ARG,
    }
}

/// Grant `perms` to `app_id`.
///
/// If the app already has a slot, the new flags are OR-ed in.
/// If no slot exists a new one is allocated.
/// Returns `ESP_ERR_NO_MEM` when the table is full.
pub fn grant(app_id: &str, perms: u32) -> i32 {
    if app_id.is_empty() {
        return ESP_ERR_INVALID_ARG;
    }
    match PERM_TABLE.lock() {
        Err(_) => ESP_ERR_INVALID_ARG,
        Ok(mut table) => {
            // If the app already has a slot, update it.
            if let Some(slot) = table.iter_mut().find(|s| s.matches(app_id)) {
                slot.perms |= perms;
                return ESP_OK;
            }
            // Allocate a new slot.
            if let Some(slot) = table.iter_mut().find(|s| s.is_free()) {
                slot.set_id(app_id);
                slot.perms = perms;
                return ESP_OK;
            }
            ESP_ERR_NO_MEM
        }
    }
}

/// Revoke `perms` from `app_id`.
///
/// Returns `ESP_ERR_NOT_FOUND` if the app has no slot.
pub fn revoke(app_id: &str, perms: u32) -> i32 {
    if app_id.is_empty() {
        return ESP_ERR_INVALID_ARG;
    }
    match PERM_TABLE.lock() {
        Err(_) => ESP_ERR_INVALID_ARG,
        Ok(mut table) => {
            match table.iter_mut().find(|s| s.matches(app_id)) {
                Some(slot) => {
                    slot.perms &= !perms;
                    ESP_OK
                }
                None => ESP_ERR_NOT_FOUND,
            }
        }
    }
}

/// Return `true` if `app_id` holds the single permission bit `perm`.
pub fn check(app_id: &str, perm: u32) -> bool {
    if app_id.is_empty() {
        return false;
    }
    match PERM_TABLE.lock() {
        Err(_) => false,
        Ok(table) => {
            table
                .iter()
                .find(|s| s.matches(app_id))
                .map(|s| s.perms & perm == perm)
                .unwrap_or(false)
        }
    }
}

/// Return the full permission bitmask for `app_id`, or 0 if not found.
pub fn get(app_id: &str) -> u32 {
    if app_id.is_empty() {
        return 0;
    }
    match PERM_TABLE.lock() {
        Err(_) => 0,
        Ok(table) => {
            table
                .iter()
                .find(|s| s.matches(app_id))
                .map(|s| s.perms)
                .unwrap_or(0)
        }
    }
}

/// Parse a comma-separated list of permission names into a bitmask.
///
/// Unknown names are silently ignored (contribute 0).
/// Examples: `"radio"` → `PERM_RADIO`, `"radio,gps"` → `PERM_RADIO | PERM_GPS`.
pub fn parse(name: &str) -> u32 {
    name.split(',')
        .map(|tok| tok.trim())
        .fold(0u32, |acc, tok| {
            acc | match tok.to_ascii_lowercase().as_str() {
                "radio"   => PERM_RADIO,
                "gps"     => PERM_GPS,
                "storage" => PERM_STORAGE,
                "network" => PERM_NETWORK,
                "audio"   => PERM_AUDIO,
                "system"  => PERM_SYSTEM,
                "ipc"     => PERM_IPC,
                "all"     => PERM_ALL,
                _         => 0,
            }
        })
}

/// Format a permission bitmask as a comma-separated string of names.
///
/// Only the named bits are included; unknown bits are omitted.
/// Example: `PERM_RADIO | PERM_GPS` → `"radio,gps"`.
pub fn to_string(perms: u32) -> String {
    let flags: &[(&str, u32)] = &[
        ("radio",   PERM_RADIO),
        ("gps",     PERM_GPS),
        ("storage", PERM_STORAGE),
        ("network", PERM_NETWORK),
        ("audio",   PERM_AUDIO),
        ("system",  PERM_SYSTEM),
        ("ipc",     PERM_IPC),
    ];
    flags
        .iter()
        .filter(|(_, flag)| perms & flag != 0)
        .map(|(name, _)| *name)
        .collect::<Vec<&str>>()
        .join(",")
}

// ---------------------------------------------------------------------------
// FFI — C-callable exports
// ---------------------------------------------------------------------------

/// Initialise the permissions subsystem.
///
/// # Safety
/// May be called from C at any time. Thread-safe.
#[no_mangle]
pub extern "C" fn rs_permissions_init() -> i32 {
    init()
}

/// Grant permissions to an app.
///
/// # Safety
/// `app_id` must be a valid, null-terminated C string. May not be NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_permissions_grant(
    app_id: *const c_char,
    perms: u32,
) -> i32 {
    if app_id.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match CStr::from_ptr(app_id).to_str() {
        Ok(id) => grant(id, perms),
        Err(_) => ESP_ERR_INVALID_ARG,
    }
}

/// Revoke permissions from an app.
///
/// # Safety
/// `app_id` must be a valid, null-terminated C string. May not be NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_permissions_revoke(
    app_id: *const c_char,
    perms: u32,
) -> i32 {
    if app_id.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match CStr::from_ptr(app_id).to_str() {
        Ok(id) => revoke(id, perms),
        Err(_) => ESP_ERR_INVALID_ARG,
    }
}

/// Check whether an app holds a specific permission.
///
/// Returns 1 if granted, 0 if not.
///
/// # Safety
/// `app_id` must be a valid, null-terminated C string. May not be NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_permissions_check(
    app_id: *const c_char,
    perm: u32,
) -> i32 {
    if app_id.is_null() {
        return 0;
    }
    match CStr::from_ptr(app_id).to_str() {
        Ok(id) => check(id, perm) as i32,
        Err(_) => 0,
    }
}

/// Return the full permission bitmask for an app, or 0 if not found.
///
/// # Safety
/// `app_id` must be a valid, null-terminated C string. May not be NULL.
#[no_mangle]
pub unsafe extern "C" fn rs_permissions_get(app_id: *const c_char) -> u32 {
    if app_id.is_null() {
        return 0;
    }
    match CStr::from_ptr(app_id).to_str() {
        Ok(id) => get(id),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the table before each test to avoid cross-test pollution.
    fn reset() {
        init();
    }

    #[test]
    fn test_grant_and_check() {
        reset();
        assert_eq!(grant("app.radio_gps", PERM_RADIO | PERM_GPS), ESP_OK);
        assert!(check("app.radio_gps", PERM_RADIO), "RADIO should be granted");
        assert!(check("app.radio_gps", PERM_GPS),   "GPS should be granted");
        assert!(!check("app.radio_gps", PERM_AUDIO), "AUDIO should NOT be granted");
    }

    #[test]
    fn test_revoke() {
        reset();
        assert_eq!(grant("app.revoke", PERM_ALL), ESP_OK);
        assert!(check("app.revoke", PERM_RADIO), "RADIO should be granted before revoke");
        assert_eq!(revoke("app.revoke", PERM_RADIO), ESP_OK);
        assert!(!check("app.revoke", PERM_RADIO), "RADIO should be revoked");
        // Other permissions must remain intact.
        assert!(check("app.revoke", PERM_GPS),    "GPS should still be granted");
        assert!(check("app.revoke", PERM_STORAGE), "STORAGE should still be granted");
    }

    #[test]
    fn test_parse() {
        assert_eq!(parse("radio"),   PERM_RADIO);
        assert_eq!(parse("gps"),     PERM_GPS);
        assert_eq!(parse("unknown"), 0);
        assert_eq!(parse("radio,gps"), PERM_RADIO | PERM_GPS);
        // Case-insensitive
        assert_eq!(parse("RADIO"), PERM_RADIO);
        assert_eq!(parse("all"),   PERM_ALL);
    }

    #[test]
    fn test_to_string() {
        let s = to_string(PERM_RADIO | PERM_GPS);
        // Order is canonical (defined by the flags slice above).
        assert_eq!(s, "radio,gps");

        assert_eq!(to_string(0), "");
        assert_eq!(to_string(PERM_AUDIO), "audio");

        // Round-trip
        let rt = to_string(parse("storage,ipc,network"));
        assert_eq!(rt, "storage,network,ipc");
    }

    #[test]
    fn test_max_apps() {
        reset();
        // Fill all 16 slots.
        for i in 0..MAX_APPS {
            let id = format!("app.slot{}", i);
            let result = grant(&id, PERM_RADIO);
            assert_eq!(
                result, ESP_OK,
                "slot {} should succeed (result=0x{:x})", i, result
            );
        }
        // The 17th app must be rejected.
        let result = grant("app.overflow", PERM_RADIO);
        assert_eq!(
            result, ESP_ERR_NO_MEM,
            "17th app should fail with ESP_ERR_NO_MEM (got 0x{:x})", result
        );
        // Granting to an existing app must still work (no new slot needed).
        assert_eq!(grant("app.slot0", PERM_GPS), ESP_OK);
    }

    #[test]
    fn test_revoke_not_found() {
        reset();
        assert_eq!(revoke("app.ghost", PERM_RADIO), ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_invalid_args() {
        reset();
        assert_eq!(grant("", PERM_RADIO), ESP_ERR_INVALID_ARG);
        assert_eq!(revoke("", PERM_RADIO), ESP_ERR_INVALID_ARG);
        assert!(!check("", PERM_RADIO));
        assert_eq!(get(""), 0);
    }

    #[test]
    fn test_grant_accumulates() {
        reset();
        assert_eq!(grant("app.accum", PERM_RADIO), ESP_OK);
        assert_eq!(grant("app.accum", PERM_GPS),   ESP_OK);
        assert_eq!(get("app.accum"), PERM_RADIO | PERM_GPS);
    }
}
