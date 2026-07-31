// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

use std::path::{Component, Path, PathBuf};

pub const MAX_CATALOG_ID_LENGTH: usize = 63;

/// Validate an untrusted catalog identifier before it becomes a filename.
///
/// Catalog IDs are a single ASCII component. Dots are retained for reverse-
/// domain IDs, but dot segments and repeated dots are not permitted.
pub fn validate_catalog_id(id: &str) -> Result<(), &'static str> {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_CATALOG_ID_LENGTH {
        return Err("catalog id has an invalid length");
    }
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err("catalog id must start with a lowercase letter or digit");
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return Err("catalog id must end with a lowercase letter or digit");
    }
    if !bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
    }) || id.contains("..")
    {
        return Err("catalog id must use the lowercase component grammar");
    }
    Ok(())
}

/// Construct one artifact destination and prove that its normalized parent is
/// exactly the intended install root.
pub fn catalog_destination(root: &str, id: &str, suffix: &str) -> Result<PathBuf, &'static str> {
    validate_catalog_id(id)?;
    let root = Path::new(root);
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("catalog install root is not normalized");
    }

    let destination = root.join(format!("{id}{suffix}"));
    if destination.parent() != Some(root)
        || destination
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("catalog destination escapes its install root");
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_separators_dot_segments_absolute_prefixes_and_encoding_tricks() {
        let overlong = "a".repeat(MAX_CATALOG_ID_LENGTH + 1);
        for invalid in [
            "",
            ".",
            "..",
            "../driver",
            "driver/child",
            r"driver\child",
            "/absolute",
            "C:driver",
            "%2e%2e%2fdriver",
            "driver..child",
            ".hidden",
            "trailing.",
            "UPPERCASE",
            "non-ascii-é",
            overlong.as_str(),
        ] {
            assert!(
                validate_catalog_id(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn valid_ids_resolve_under_each_type_specific_root() {
        for (root, id, suffix, expected) in [
            (
                "/sdcard/config/boards",
                "waveshare-esp32-s3-touch-amoled-2.06",
                ".json",
                "/sdcard/config/boards/waveshare-esp32-s3-touch-amoled-2.06.json",
            ),
            (
                "/sdcard/drivers",
                "com.thistle.drv.radio-sx1262",
                ".drv.elf",
                "/sdcard/drivers/com.thistle.drv.radio-sx1262.drv.elf",
            ),
            (
                "/sdcard/wm",
                "com.thistle.wm.thistle-tk",
                ".wm.elf",
                "/sdcard/wm/com.thistle.wm.thistle-tk.wm.elf",
            ),
        ] {
            assert_eq!(
                catalog_destination(root, id, suffix).unwrap(),
                PathBuf::from(expected)
            );
        }
    }
}
