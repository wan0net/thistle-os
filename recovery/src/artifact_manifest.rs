// SPDX-License-Identifier: BSD-3-Clause
// Canonical, signed description of a Recovery-installable artifact.

pub const MANIFEST_HEADER: &str = "THISTLE-ARTIFACT-MANIFEST-V1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactType {
    Firmware,
    Board,
    Driver,
    WindowManager,
}

impl ArtifactType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Firmware => "firmware",
            Self::Board => "board",
            Self::Driver => "driver",
            Self::WindowManager => "wm",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "firmware" => Ok(Self::Firmware),
            "board" => Ok(Self::Board),
            "driver" => Ok(Self::Driver),
            "wm" => Ok(Self::WindowManager),
            _ => Err(format!("unsupported artifact type '{value}'")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactManifest {
    pub artifact_type: ArtifactType,
    pub id: String,
    pub version: String,
    pub security_version: u32,
    pub arch: String,
    pub compatible_boards: Vec<String>,
    pub destination: String,
    pub sha256: String,
    pub size: usize,
    pub url: String,
}

impl ArtifactManifest {
    pub fn parse(canonical: &str) -> Result<Self, String> {
        if !canonical.ends_with('\n') || canonical.contains('\r') {
            return Err("manifest must use canonical LF-terminated lines".to_string());
        }
        let lines: Vec<&str> = canonical[..canonical.len() - 1].split('\n').collect();
        if lines.len() != 11 || lines[0] != MANIFEST_HEADER {
            return Err("manifest has an invalid header or field count".to_string());
        }

        let artifact_type = ArtifactType::parse(field(lines[1], "type")?)?;
        let id = field(lines[2], "id")?.to_string();
        let version = field(lines[3], "version")?.to_string();
        let security_version = field(lines[4], "security_version")?
            .parse::<u32>()
            .map_err(|_| "security_version must be a decimal u32".to_string())?;
        let arch = field(lines[5], "arch")?.to_string();
        let compatible_boards = field(lines[6], "compatible_boards")?
            .split(',')
            .map(str::to_string)
            .collect::<Vec<_>>();
        let destination = field(lines[7], "destination")?.to_string();
        let sha256 = field(lines[8], "sha256")?.to_string();
        let size = field(lines[9], "size")?
            .parse::<usize>()
            .map_err(|_| "size must be a decimal usize".to_string())?;
        let url = field(lines[10], "url")?.to_string();

        let manifest = Self {
            artifact_type,
            id,
            version,
            security_version,
            arch,
            compatible_boards,
            destination,
            sha256,
            size,
            url,
        };
        manifest.validate_shape()?;
        if manifest.canonical_bytes() != canonical.as_bytes() {
            return Err("manifest is not in canonical form".to_string());
        }
        Ok(manifest)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        format!(
            concat!(
                "{}\n",
                "type={}\n",
                "id={}\n",
                "version={}\n",
                "security_version={}\n",
                "arch={}\n",
                "compatible_boards={}\n",
                "destination={}\n",
                "sha256={}\n",
                "size={}\n",
                "url={}\n"
            ),
            MANIFEST_HEADER,
            self.artifact_type.as_str(),
            self.id,
            self.version,
            self.security_version,
            self.arch,
            self.compatible_boards.join(","),
            self.destination,
            self.sha256,
            self.size,
            self.url,
        )
        .into_bytes()
    }

    pub fn validate_for_device(
        &self,
        chip: &str,
        board: &str,
        minimum_security_version: u32,
    ) -> Result<(), String> {
        self.validate_shape()?;
        if self.arch != chip {
            return Err(format!(
                "artifact architecture '{}' does not match '{}'",
                self.arch, chip
            ));
        }
        if self.security_version < minimum_security_version {
            return Err(format!(
                "artifact security version {} is below device minimum {}",
                self.security_version, minimum_security_version
            ));
        }
        if !self
            .compatible_boards
            .iter()
            .any(|candidate| candidate == board)
        {
            return Err(format!("artifact is not signed for board '{board}'"));
        }
        if self.artifact_type == ArtifactType::Board && self.id != board {
            return Err("board manifest identity does not match selection".to_string());
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), String> {
        validate_identifier(&self.id, "id")?;
        validate_version(&self.version)?;
        if self.security_version == 0 {
            return Err("security_version must be greater than zero".to_string());
        }
        if !matches!(
            self.arch.as_str(),
            "esp32" | "esp32s2" | "esp32s3" | "esp32c3" | "esp32c6"
        ) {
            return Err(format!("unsupported architecture '{}'", self.arch));
        }
        if self.compatible_boards.is_empty() {
            return Err("compatible_boards must not be empty".to_string());
        }
        let mut previous: Option<&str> = None;
        for board in &self.compatible_boards {
            validate_identifier(board, "compatible board")?;
            if previous.is_some_and(|value| value >= board.as_str()) {
                return Err("compatible_boards must be sorted and unique".to_string());
            }
            previous = Some(board);
        }
        let expected_destination = expected_destination(self.artifact_type, &self.id);
        if self.destination != expected_destination {
            return Err(format!(
                "destination '{}' does not match signed artifact role '{}'",
                self.destination, expected_destination
            ));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("sha256 must be 64 lowercase hexadecimal characters".to_string());
        }
        if self.size == 0 {
            return Err("artifact size must be greater than zero".to_string());
        }
        if !self.url.starts_with("https://") || self.url.bytes().any(|byte| byte <= b' ') {
            return Err("artifact URL must be a whitespace-free HTTPS URL".to_string());
        }
        Ok(())
    }
}

pub fn expected_destination(artifact_type: ArtifactType, id: &str) -> String {
    match artifact_type {
        ArtifactType::Firmware => "ota_1".to_string(),
        ArtifactType::Board => format!("config/boards/{id}.json"),
        ArtifactType::Driver => format!("drivers/{id}.drv.elf"),
        ArtifactType::WindowManager => format!("wm/{id}.wm.elf"),
    }
}

fn field<'a>(line: &'a str, expected_name: &str) -> Result<&'a str, String> {
    line.strip_prefix(expected_name)
        .and_then(|rest| rest.strip_prefix('='))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing or empty canonical field '{expected_name}'"))
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('.')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("invalid {label} '{value}'"));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), String> {
    let normalized = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!(
            "version '{version}' must be a numeric semantic version"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn firmware() -> ArtifactManifest {
        ArtifactManifest {
            artifact_type: ArtifactType::Firmware,
            id: "thistle-os".to_string(),
            version: "v0.5.1".to_string(),
            security_version: 12,
            arch: "esp32s3".to_string(),
            compatible_boards: vec!["tdeck".to_string(), "tdeck-pro".to_string()],
            destination: "ota_1".to_string(),
            sha256: "ab".repeat(32),
            size: 1_048_576,
            url: "https://downloads.example/thistle-os.bin".to_string(),
        }
    }

    #[test]
    fn canonical_manifest_round_trips() {
        let manifest = firmware();
        let bytes = manifest.canonical_bytes();
        assert_eq!(
            ArtifactManifest::parse(std::str::from_utf8(&bytes).unwrap()),
            Ok(manifest)
        );
    }

    #[test]
    fn metadata_changes_change_signed_bytes() {
        let original = firmware();
        for changed in [
            ArtifactManifest {
                artifact_type: ArtifactType::Driver,
                destination: "drivers/thistle-os.drv.elf".to_string(),
                ..original.clone()
            },
            ArtifactManifest {
                id: "other".to_string(),
                ..original.clone()
            },
            ArtifactManifest {
                version: "v0.5.0".to_string(),
                ..original.clone()
            },
            ArtifactManifest {
                security_version: 11,
                ..original.clone()
            },
            ArtifactManifest {
                arch: "esp32c3".to_string(),
                ..original.clone()
            },
            ArtifactManifest {
                compatible_boards: vec!["tdeck".to_string()],
                ..original.clone()
            },
            ArtifactManifest {
                sha256: "cd".repeat(32),
                ..original.clone()
            },
            ArtifactManifest {
                size: original.size + 1,
                ..original.clone()
            },
            ArtifactManifest {
                url: "https://downloads.example/other.bin".to_string(),
                ..original.clone()
            },
        ] {
            assert_ne!(changed.canonical_bytes(), original.canonical_bytes());
        }
    }

    #[test]
    fn device_policy_rejects_relabeling_incompatibility_and_rollback() {
        let manifest = firmware();
        assert!(manifest
            .validate_for_device("esp32s3", "tdeck-pro", 12)
            .is_ok());
        assert!(manifest
            .validate_for_device("esp32c3", "tdeck-pro", 12)
            .is_err());
        assert!(manifest
            .validate_for_device("esp32s3", "c3-mini", 12)
            .is_err());
        assert!(manifest
            .validate_for_device("esp32s3", "tdeck-pro", 13)
            .is_err());

        let board = ArtifactManifest {
            artifact_type: ArtifactType::Board,
            id: "tdeck".to_string(),
            version: "1.0.0".to_string(),
            security_version: 1,
            arch: "esp32s3".to_string(),
            compatible_boards: vec!["tdeck".to_string()],
            destination: "config/boards/tdeck.json".to_string(),
            sha256: "ef".repeat(32),
            size: 1024,
            url: "https://downloads.example/tdeck.json".to_string(),
        };
        assert!(board
            .validate_for_device("esp32s3", "tdeck-pro", 1)
            .is_err());
    }

    #[test]
    fn malformed_or_noncanonical_manifests_fail_closed() {
        let canonical = String::from_utf8(firmware().canonical_bytes()).unwrap();
        assert!(ArtifactManifest::parse(
            &canonical.replace("destination=ota_1", "destination=drivers/x.drv.elf")
        )
        .is_err());
        assert!(ArtifactManifest::parse(&canonical.replace(
            "compatible_boards=tdeck,tdeck-pro",
            "compatible_boards=tdeck-pro,tdeck"
        ))
        .is_err());
        assert!(ArtifactManifest::parse(&canonical.replace("sha256=", "sha256=AB")).is_err());
        assert!(
            ArtifactManifest::parse(&canonical.replace("arch=esp32s3", "arch=esp32h2")).is_err()
        );
        assert!(ArtifactManifest::parse(canonical.trim_end()).is_err());
        assert!(ArtifactManifest::parse(&(canonical.clone() + "extra=x\n")).is_err());
    }
}
