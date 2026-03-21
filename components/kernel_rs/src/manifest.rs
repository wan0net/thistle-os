// SPDX-License-Identifier: BSD-3-Clause
// Unified manifest parser for apps, drivers, and firmware.

use std::fs;
use std::path::Path;

use crate::version;

/// Manifest entry types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ManifestType {
    App = 0,
    Driver = 1,
    Firmware = 2,
}

/// Permission flags (must match C PERM_* constants).
pub mod perm {
    pub const RADIO: u32 = 1 << 0;
    pub const GPS: u32 = 1 << 1;
    pub const STORAGE: u32 = 1 << 2;
    pub const NETWORK: u32 = 1 << 3;
    pub const AUDIO: u32 = 1 << 4;
    pub const SYSTEM: u32 = 1 << 5;
    pub const IPC: u32 = 1 << 6;
    pub const ALL: u32 = 0x7F;
}

/// Parsed manifest — covers apps, drivers, and firmware.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub manifest_type: ManifestType,

    // Identity
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,

    // Compatibility
    pub min_os: String,
    pub arch: String,

    // Files
    pub entry: String,
    pub icon: String,

    // App-specific
    pub permissions: u32,
    pub background: bool,
    pub min_memory_kb: u32,

    // Driver-specific
    pub hal_interface: String,

    // Firmware-specific
    pub changelog: String,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            manifest_type: ManifestType::App,
            id: String::new(),
            name: String::new(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            min_os: String::new(),
            arch: String::new(),
            entry: String::new(),
            icon: String::new(),
            permissions: 0,
            background: false,
            min_memory_kb: 0,
            hal_interface: String::new(),
            changelog: String::new(),
        }
    }
}

/// Error type for manifest operations.
#[derive(Debug)]
pub enum ManifestError {
    NotFound,
    ParseError(String),
    Incompatible(String),
    IoError(std::io::Error),
}

impl From<std::io::Error> for ManifestError {
    fn from(e: std::io::Error) -> Self {
        ManifestError::IoError(e)
    }
}

impl Manifest {
    /// Parse a manifest from a JSON string.
    /// Uses simple string scanning (no serde dependency).
    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        let mut m = Manifest::default();

        // Type (required)
        let type_str = json_get_string(json, "type")
            .ok_or_else(|| ManifestError::ParseError("missing 'type' field".into()))?;

        m.manifest_type = match type_str.as_str() {
            "app" => ManifestType::App,
            "driver" => ManifestType::Driver,
            "firmware" => ManifestType::Firmware,
            other => {
                return Err(ManifestError::ParseError(format!(
                    "unknown type: {other}"
                )))
            }
        };

        // Identity
        m.id = json_get_string(json, "id")
            .ok_or_else(|| ManifestError::ParseError("missing 'id' field".into()))?;
        m.name = json_get_string(json, "name").unwrap_or_default();
        m.version = json_get_string(json, "version").unwrap_or_default();
        m.author = json_get_string(json, "author").unwrap_or_default();
        m.description = json_get_string(json, "description").unwrap_or_default();

        // Compatibility
        m.min_os = json_get_string(json, "min_os").unwrap_or_default();
        m.arch = json_get_string(json, "arch").unwrap_or_default();

        // Files
        m.entry = json_get_string(json, "entry").unwrap_or_default();
        m.icon = json_get_string(json, "icon").unwrap_or_default();

        // App-specific
        m.permissions = parse_permissions(json);
        m.background = json_get_bool(json, "background").unwrap_or(false);
        m.min_memory_kb = json_get_int(json, "min_memory_kb").unwrap_or(0) as u32;

        // Driver-specific
        m.hal_interface = json_get_string(json, "hal_interface").unwrap_or_default();

        // Firmware-specific
        m.changelog = json_get_string(json, "changelog").unwrap_or_default();

        Ok(m)
    }

    /// Parse a manifest from a file.
    pub fn from_file(path: &Path) -> Result<Self, ManifestError> {
        let json = fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ManifestError::NotFound
            } else {
                ManifestError::IoError(e)
            }
        })?;
        Self::from_json(&json)
    }

    /// Derive manifest path from ELF path.
    /// e.g., "messenger.app.elf" → "messenger.manifest.json"
    pub fn path_from_elf(elf_path: &str) -> String {
        if let Some(pos) = elf_path.find(".app.elf") {
            format!("{}.manifest.json", &elf_path[..pos])
        } else if let Some(pos) = elf_path.find(".drv.elf") {
            format!("{}.manifest.json", &elf_path[..pos])
        } else {
            format!("{}.manifest.json", elf_path)
        }
    }

    /// Check if this manifest is compatible with the running kernel.
    pub fn is_compatible(&self, current_arch: &str) -> bool {
        // Check architecture
        if !self.arch.is_empty() && self.arch != current_arch {
            return false;
        }

        // Check min_os version
        if !self.min_os.is_empty() && !version::satisfies(&self.min_os) {
            return false;
        }

        true
    }

    /// Type as string.
    pub fn type_str(&self) -> &'static str {
        match self.manifest_type {
            ManifestType::App => "app",
            ManifestType::Driver => "driver",
            ManifestType::Firmware => "firmware",
        }
    }
}

// ── Simple JSON helpers (no serde dependency) ──────────────────────────

/// Extract a string value for a given key from JSON text.
pub fn json_get_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];

    // Skip whitespace and colon
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    if !trimmed.starts_with('"') {
        return None;
    }

    let value_start = &trimmed[1..];
    let end = value_start.find('"')?;
    Some(value_start[..end].to_string())
}

/// Extract a boolean value for a given key.
pub fn json_get_bool(json: &str, key: &str) -> Option<bool> {
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

/// Extract an integer value for a given key.
pub fn json_get_int(json: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    // Parse leading digits (possibly with minus)
    let num_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(trimmed.len());
    trimmed[..num_end].parse().ok()
}

/// Parse permissions from the JSON (array or comma-separated string).
fn parse_permissions(json: &str) -> u32 {
    let pattern = "\"permissions\"";
    let start = match json.find(pattern) {
        Some(s) => s,
        None => return 0,
    };
    let after = &json[start + pattern.len()..];
    let after_colon = match after.trim_start().strip_prefix(':') {
        Some(s) => s.trim_start(),
        None => return 0,
    };

    // Find the extent of the permissions value (array or string)
    let chunk = if after_colon.starts_with('[') {
        let end = after_colon.find(']').unwrap_or(after_colon.len());
        &after_colon[..end]
    } else if after_colon.starts_with('"') {
        let inner = &after_colon[1..];
        let end = inner.find('"').unwrap_or(inner.len());
        &inner[..end]
    } else {
        return 0;
    };

    let mut perms = 0u32;
    if chunk.contains("radio") {
        perms |= perm::RADIO;
    }
    if chunk.contains("gps") {
        perms |= perm::GPS;
    }
    if chunk.contains("storage") {
        perms |= perm::STORAGE;
    }
    if chunk.contains("network") {
        perms |= perm::NETWORK;
    }
    if chunk.contains("audio") {
        perms |= perm::AUDIO;
    }
    if chunk.contains("system") {
        perms |= perm::SYSTEM;
    }
    if chunk.contains("ipc") {
        perms |= perm::IPC;
    }
    perms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_app_manifest() {
        let json = r#"{
            "type": "app",
            "id": "com.example.hello",
            "name": "Hello World",
            "version": "1.0.0",
            "author": "ThistleOS",
            "min_os": "0.1.0",
            "arch": "esp32s3",
            "entry": "hello.app.elf",
            "permissions": ["radio", "gps"],
            "background": true,
            "min_memory_kb": 128
        }"#;

        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.manifest_type, ManifestType::App);
        assert_eq!(m.id, "com.example.hello");
        assert_eq!(m.name, "Hello World");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.arch, "esp32s3");
        assert!(m.background);
        assert_eq!(m.min_memory_kb, 128);
        assert_eq!(m.permissions, perm::RADIO | perm::GPS);
    }

    #[test]
    fn test_parse_driver_manifest() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.sx1262",
            "name": "SX1262 LoRa",
            "version": "1.0.0",
            "hal_interface": "radio",
            "entry": "sx1262.drv.elf"
        }"#;

        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.manifest_type, ManifestType::Driver);
        assert_eq!(m.hal_interface, "radio");
    }

    #[test]
    fn test_parse_firmware_manifest() {
        let json = r#"{
            "type": "firmware",
            "id": "com.thistle.os",
            "name": "ThistleOS",
            "version": "0.2.0",
            "arch": "esp32s3",
            "entry": "thistle_os.bin",
            "changelog": "Ed25519 signing"
        }"#;

        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.manifest_type, ManifestType::Firmware);
        assert_eq!(m.changelog, "Ed25519 signing");
        assert_eq!(m.arch, "esp32s3");
    }

    #[test]
    fn test_missing_type_fails() {
        let json = r#"{"id": "test", "name": "Test"}"#;
        assert!(Manifest::from_json(json).is_err());
    }

    #[test]
    fn test_missing_id_fails() {
        let json = r#"{"type": "app", "name": "Test"}"#;
        assert!(Manifest::from_json(json).is_err());
    }

    #[test]
    fn test_path_from_elf() {
        assert_eq!(
            Manifest::path_from_elf("/sdcard/apps/messenger.app.elf"),
            "/sdcard/apps/messenger.manifest.json"
        );
        assert_eq!(
            Manifest::path_from_elf("/sdcard/drivers/sx1262.drv.elf"),
            "/sdcard/drivers/sx1262.manifest.json"
        );
    }

    #[test]
    fn test_compatibility_check() {
        let m = Manifest {
            min_os: "0.1.0".into(),
            arch: "esp32s3".into(),
            ..Default::default()
        };
        assert!(m.is_compatible("esp32s3"));
        assert!(!m.is_compatible("esp32c3"));
    }

    #[test]
    fn test_version_satisfies() {
        assert!(crate::version::satisfies("0.1.0"));
        assert!(crate::version::satisfies("0.0.1"));
        assert!(!crate::version::satisfies("1.0.0"));
    }
}
