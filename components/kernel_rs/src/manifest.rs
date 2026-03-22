// SPDX-License-Identifier: BSD-3-Clause
// Unified manifest parser for apps, drivers, and firmware.

use std::fs;
use std::path::Path;

use crate::version;

/// Return the architecture slug for the current build target.
///
/// Used by the kernel to filter manifests with an `arch` field.  The slug
/// matches the values used in `catalog_example.json` and `detect_chip()` in
/// the recovery crate.
///
/// For Xtensa targets the specific chip variant (esp32 / esp32s2 / esp32s3)
/// is selected via Cargo features because Xtensa has no sub-target distinction
/// at the `target_arch` level.  RISC-V chips are differentiated similarly.
///
/// On the simulator (aarch64 / x86_64 host) the slug is `"host"` so that
/// manifests without an `arch` field (universal) load normally in tests.
pub fn current_arch() -> &'static str {
    #[cfg(target_arch = "xtensa")]
    {
        #[cfg(feature = "esp32")]
        return "esp32";
        #[cfg(feature = "esp32s2")]
        return "esp32s2";
        // Default for xtensa builds — the production target is ESP32-S3.
        "esp32s3"
    }
    #[cfg(target_arch = "riscv32")]
    {
        #[cfg(feature = "esp32c6")]
        return "esp32c6";
        #[cfg(feature = "esp32h2")]
        return "esp32h2";
        // Default for riscv32 builds.
        "esp32c3"
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        "host" // simulator / unit tests on aarch64 or x86_64
    }
}

/// Manifest entry types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ManifestType {
    App = 0,
    Driver = 1,
    Firmware = 2,
    Wm = 3,
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

/// Hardware detection descriptor — each driver declares how its chip is found.
///
/// Recovery probes buses and matches detected devices to catalog entries using
/// these fields.  All fields except `bus` are optional.
#[derive(Debug, Clone, Default)]
pub struct ManifestDetection {
    /// Bus type: "i2c", "spi", "uart", or "gpio".
    pub bus: String,
    /// I2C device address (e.g. 0x34) or SPI chip-select index.
    /// None means the driver matches any device on the bus (use sparingly).
    pub address: Option<u16>,
    /// Register to read for chip-ID verification (e.g. 0x0320 for SX1262).
    pub chip_id_reg: Option<u16>,
    /// Expected value returned by `chip_id_reg`.
    pub chip_id_value: Option<u16>,
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

    // Board compatibility — empty means universal (compatible with all boards)
    pub compatible_boards: Vec<String>,

    // Hardware detection — drivers use this instead of (or alongside) compatible_boards
    pub detection: Option<ManifestDetection>,
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
            compatible_boards: Vec::new(),
            detection: None,
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
            "wm" => ManifestType::Wm,
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

        // Board compatibility (optional array — absent means universal)
        m.compatible_boards = json_get_string_array(json, "compatible_boards");

        // Hardware detection (optional object — drivers use this for bus probing)
        m.detection = json_get_detection(json);

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
        } else if let Some(pos) = elf_path.find(".wm.elf") {
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
            ManifestType::Wm => "wm",
        }
    }

    /// Check if this manifest is compatible with the given board name.
    /// Returns true if `compatible_boards` is empty (universal) or contains `board_name`.
    pub fn is_board_compatible(&self, board_name: &str) -> bool {
        if self.compatible_boards.is_empty() {
            return true;
        }
        self.compatible_boards.iter().any(|b| b == board_name)
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

/// Extract a JSON string array for a given key.
/// Returns an empty Vec if the key is absent or the value is not an array.
/// e.g., `"compatible_boards": ["tdeck-pro", "tdeck"]` → vec!["tdeck-pro", "tdeck"]
pub fn json_get_string_array(json: &str, key: &str) -> Vec<String> {
    let pattern = format!("\"{}\"", key);
    let start = match json.find(&pattern) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let after_key = &json[start + pattern.len()..];
    let after_colon = match after_key.trim_start().strip_prefix(':') {
        Some(s) => s.trim_start(),
        None => return Vec::new(),
    };

    if !after_colon.starts_with('[') {
        return Vec::new();
    }

    let bracket_content = match after_colon[1..].find(']') {
        Some(end) => &after_colon[1..end + 1],
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    let mut remaining = bracket_content;
    loop {
        let q1 = match remaining.find('"') {
            Some(i) => i,
            None => break,
        };
        let inner = &remaining[q1 + 1..];
        let q2 = match inner.find('"') {
            Some(i) => i,
            None => break,
        };
        result.push(inner[..q2].to_string());
        remaining = &inner[q2 + 1..];
    }
    result
}

/// Extract the `detection` object from a manifest JSON string.
///
/// Handles both decimal and `"0x…"` hex string representations for numeric
/// fields, matching the catalog and manifest JSON convention.
///
/// Example JSON shape:
/// ```json
/// "detection": {"bus":"i2c","address":"0x34","chip_id_reg":"0x01","chip_id_value":"0x81"}
/// ```
pub fn json_get_detection(json: &str) -> Option<ManifestDetection> {
    // Find the detection object
    let key = "\"detection\"";
    let pos = json.find(key)?;
    let after_key = &json[pos + key.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }

    // Extract the inner content of the detection object
    let inner_start = &trimmed[1..];
    let brace_end = inner_start.find('}')?;
    let inner = &inner_start[..brace_end];

    let bus = json_get_string(inner, "bus").unwrap_or_default();
    if bus.is_empty() {
        return None;
    }

    let address    = json_get_hex_or_int(inner, "address");
    let chip_id_reg   = json_get_hex_or_int(inner, "chip_id_reg");
    let chip_id_value = json_get_hex_or_int(inner, "chip_id_value");

    Some(ManifestDetection {
        bus,
        address,
        chip_id_reg,
        chip_id_value,
    })
}

/// Extract a numeric value that may be encoded as either a decimal integer
/// or a hex string `"0x…"`.  Returns None if the key is absent.
fn json_get_hex_or_int(json: &str, key: &str) -> Option<u16> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();

    if trimmed.starts_with('"') {
        // Quoted hex or decimal string: "0x34" or "52"
        let inner = &trimmed[1..];
        let end = inner.find('"')?;
        let s = inner[..end].trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u16::from_str_radix(hex, 16).ok()
        } else {
            s.parse::<u16>().ok()
        }
    } else {
        // Bare decimal integer
        let num_end = trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        trimmed[..num_end].parse::<u16>().ok()
    }
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

    #[test]
    fn test_permissions_array() {
        let json = r#"{"type": "app", "id": "x", "permissions": ["radio", "gps", "storage"]}"#;
        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.permissions, perm::RADIO | perm::GPS | perm::STORAGE);
    }

    #[test]
    fn test_permissions_string() {
        let json = r#"{"type": "app", "id": "x", "permissions": "radio,gps"}"#;
        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.permissions, perm::RADIO | perm::GPS);
    }

    #[test]
    fn test_empty_manifest() {
        let json = r#"{}"#;
        let result = Manifest::from_json(json);
        assert!(result.is_err(), "empty manifest must fail with missing 'type'");
        if let Err(ManifestError::ParseError(msg)) = result {
            assert!(msg.contains("type"), "error should mention missing type field");
        }
    }

    #[test]
    fn test_large_values() {
        // Build a description of exactly 200 characters
        let long_desc: String = "x".repeat(200);
        let json = format!(
            r#"{{"type": "app", "id": "x", "description": "{}"}}"#,
            long_desc
        );
        let m = Manifest::from_json(&json).unwrap();
        // The parser stores whatever the JSON contains; verify we get 200 chars back
        // (no silent truncation at the Rust level — truncation is the caller's responsibility)
        assert_eq!(m.description.len(), 200);
    }

    #[test]
    fn test_min_os_too_high() {
        let m = Manifest {
            manifest_type: ManifestType::App,
            id: "x".into(),
            min_os: "99.0.0".into(),
            arch: "esp32s3".into(),
            ..Default::default()
        };
        assert!(
            !m.is_compatible("esp32s3"),
            "min_os 99.0.0 must not be satisfied by current kernel 0.1.0"
        );
    }

    #[test]
    fn test_arch_mismatch() {
        let m = Manifest {
            manifest_type: ManifestType::App,
            id: "x".into(),
            min_os: "0.1.0".into(),
            arch: "riscv".into(),
            ..Default::default()
        };
        assert!(
            !m.is_compatible("esp32s3"),
            "arch riscv must not be compatible with esp32s3"
        );
    }

    #[test]
    fn test_parse_wm_manifest() {
        let json = r#"{
            "type": "wm",
            "id": "com.thistle.wm.thistle-tk",
            "name": "thistle-tk",
            "version": "0.1.0",
            "author": "ThistleOS Contributors",
            "description": "Pure Rust window manager for e-paper displays",
            "min_os": "0.1.0",
            "arch": "esp32s3",
            "entry": "thistle-tk.wm.elf",
            "compatible_boards": ["tdeck-pro"]
        }"#;

        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.manifest_type, ManifestType::Wm);
        assert_eq!(m.id, "com.thistle.wm.thistle-tk");
        assert_eq!(m.type_str(), "wm");
        assert_eq!(m.compatible_boards, vec!["tdeck-pro"]);
    }

    #[test]
    fn test_compatible_boards_empty_is_universal() {
        let m = Manifest {
            manifest_type: ManifestType::Driver,
            id: "x".into(),
            compatible_boards: vec![],
            ..Default::default()
        };
        assert!(m.is_board_compatible("tdeck-pro"), "empty compatible_boards means universal");
        assert!(m.is_board_compatible("tdeck"), "empty compatible_boards means universal");
        assert!(m.is_board_compatible("unknown-board"), "empty compatible_boards means universal");
    }

    #[test]
    fn test_compatible_boards_match() {
        let m = Manifest {
            manifest_type: ManifestType::Driver,
            id: "x".into(),
            compatible_boards: vec!["tdeck-pro".into(), "tdeck".into()],
            ..Default::default()
        };
        assert!(m.is_board_compatible("tdeck-pro"));
        assert!(m.is_board_compatible("tdeck"));
        assert!(!m.is_board_compatible("esp32-devkit"));
    }

    #[test]
    fn test_compatible_boards_parsed_from_json() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.sx1262",
            "name": "SX1262",
            "compatible_boards": ["tdeck-pro", "tdeck"]
        }"#;
        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.compatible_boards.len(), 2);
        assert_eq!(m.compatible_boards[0], "tdeck-pro");
        assert_eq!(m.compatible_boards[1], "tdeck");
    }

    #[test]
    fn test_compatible_boards_absent_is_empty() {
        let json = r#"{"type": "app", "id": "x"}"#;
        let m = Manifest::from_json(json).unwrap();
        assert!(m.compatible_boards.is_empty(), "absent field means universal (empty vec)");
    }

    #[test]
    fn test_path_from_wm_elf() {
        assert_eq!(
            Manifest::path_from_elf("/sdcard/wm/thistle-tk.wm.elf"),
            "/sdcard/wm/thistle-tk.manifest.json"
        );
    }

    #[test]
    fn test_json_get_string_array() {
        let json = r#"{"compatible_boards": ["tdeck-pro", "tdeck"]}"#;
        let arr = json_get_string_array(json, "compatible_boards");
        assert_eq!(arr, vec!["tdeck-pro", "tdeck"]);
    }

    #[test]
    fn test_json_get_string_array_empty() {
        let json = r#"{"compatible_boards": []}"#;
        let arr = json_get_string_array(json, "compatible_boards");
        assert!(arr.is_empty());
    }

    #[test]
    fn test_json_get_string_array_absent() {
        let json = r#"{"type": "app"}"#;
        let arr = json_get_string_array(json, "compatible_boards");
        assert!(arr.is_empty());
    }

    // -----------------------------------------------------------------------
    // detection field tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_detection_i2c_hex_address() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.kbd-tca8418",
            "detection": {"bus": "i2c", "address": "0x34"}
        }"#;
        let m = Manifest::from_json(json).unwrap();
        let det = m.detection.expect("detection must be present");
        assert_eq!(det.bus, "i2c");
        assert_eq!(det.address, Some(0x34));
        assert!(det.chip_id_reg.is_none());
        assert!(det.chip_id_value.is_none());
    }

    #[test]
    fn test_detection_i2c_with_chip_id() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.touch-cst328",
            "detection": {"bus": "i2c", "address": "0x1A", "chip_id_reg": "0x01", "chip_id_value": "0x81"}
        }"#;
        let m = Manifest::from_json(json).unwrap();
        let det = m.detection.expect("detection must be present");
        assert_eq!(det.bus, "i2c");
        assert_eq!(det.address, Some(0x1A));
        assert_eq!(det.chip_id_reg, Some(0x01));
        assert_eq!(det.chip_id_value, Some(0x81));
    }

    #[test]
    fn test_detection_spi_chip_id() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.sx1262",
            "detection": {"bus": "spi", "chip_id_reg": "0x0320", "chip_id_value": "0x0058"}
        }"#;
        let m = Manifest::from_json(json).unwrap();
        let det = m.detection.expect("detection must be present");
        assert_eq!(det.bus, "spi");
        assert!(det.address.is_none());
        assert_eq!(det.chip_id_reg, Some(0x0320));
        assert_eq!(det.chip_id_value, Some(0x0058));
    }

    #[test]
    fn test_detection_decimal_address() {
        // address as bare integer (52 = 0x34)
        let json = r#"{
            "type": "driver",
            "id": "x",
            "detection": {"bus": "i2c", "address": 52}
        }"#;
        let m = Manifest::from_json(json).unwrap();
        let det = m.detection.expect("detection must be present");
        assert_eq!(det.address, Some(52));
    }

    #[test]
    fn test_detection_absent_is_none() {
        let json = r#"{"type": "driver", "id": "x"}"#;
        let m = Manifest::from_json(json).unwrap();
        assert!(m.detection.is_none(), "absent detection field must be None");
    }

    #[test]
    fn test_current_arch_returns_known_slug() {
        let arch = current_arch();
        // On the test host this will be "host"; on ESP32-S3 it will be "esp32s3".
        // The important invariant is that it's a non-empty string.
        assert!(!arch.is_empty(), "current_arch() must not be empty");
    }

    #[test]
    fn test_is_compatible_with_current_arch() {
        // A manifest with no arch constraint must always be compatible.
        let universal = Manifest {
            manifest_type: ManifestType::App,
            id: "x".into(),
            arch: String::new(),
            min_os: "0.1.0".into(),
            ..Default::default()
        };
        assert!(universal.is_compatible(current_arch()), "universal manifest must be compatible");

        // A manifest targeting the current arch must be compatible.
        let same_arch = Manifest {
            manifest_type: ManifestType::App,
            id: "x".into(),
            arch: current_arch().into(),
            min_os: "0.1.0".into(),
            ..Default::default()
        };
        assert!(same_arch.is_compatible(current_arch()), "same-arch manifest must be compatible");

        // A manifest targeting a different arch must not be compatible.
        let other_arch = if current_arch() == "esp32s3" { "esp32c3" } else { "esp32s3" };
        let cross = Manifest {
            manifest_type: ManifestType::App,
            id: "x".into(),
            arch: other_arch.into(),
            min_os: "0.1.0".into(),
            ..Default::default()
        };
        assert!(!cross.is_compatible(current_arch()), "cross-arch manifest must not be compatible");
    }

    #[test]
    fn test_detection_does_not_affect_compatible_boards() {
        let json = r#"{
            "type": "driver",
            "id": "com.thistle.drv.kbd-tca8418",
            "compatible_boards": ["tdeck-pro"],
            "detection": {"bus": "i2c", "address": "0x34"}
        }"#;
        let m = Manifest::from_json(json).unwrap();
        assert_eq!(m.compatible_boards, vec!["tdeck-pro"]);
        assert!(m.detection.is_some());
    }
}
