// SPDX-License-Identifier: BSD-3-Clause
//! Strict, streaming installer for Thistle Application Packages (TAP v1).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const MAX_ARCHIVE_SIZE: u64 = 2 * 1024 * 1024;
const MAX_EXTRACTED_SIZE: u64 = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
const MAX_JSON_SIZE: u64 = 64 * 1024;
const MAX_ELF_SIZE: u64 = 1024 * 1024;
static STAGE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, PartialEq, Eq)]
pub enum TapError {
    Io,
    InvalidZip,
    Limit,
    InvalidPath,
    InvalidManifest,
    Mismatch,
    Integrity,
    Signature,
    Downgrade,
    Exists,
}

impl From<std::io::Error> for TapError {
    fn from(_: std::io::Error) -> Self { Self::Io }
}

impl From<serde_json::Error> for TapError {
    fn from(_: serde_json::Error) -> Self { Self::InvalidManifest }
}

#[derive(Clone, Debug)]
struct ZipEntry {
    name: String,
    crc32: u32,
    size: u64,
    local_offset: u64,
}

fn u16le(bytes: &[u8]) -> u16 { u16::from_le_bytes([bytes[0], bytes[1]]) }
fn u32le(bytes: &[u8]) -> u32 { u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) }

fn valid_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 160
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains('\\')
        && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_entries(file: &mut File) -> Result<Vec<ZipEntry>, TapError> {
    let length = file.metadata()?.len();
    if length > MAX_ARCHIVE_SIZE || length < 22 { return Err(TapError::Limit); }
    let tail_len = length.min(65_557) as usize;
    let mut tail = vec![0u8; tail_len];
    file.seek(SeekFrom::End(-(tail_len as i64)))?;
    file.read_exact(&mut tail)?;
    let eocd = (0..=tail.len() - 22).rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or(TapError::InvalidZip)?;
    let end = &tail[eocd..];
    if u16le(&end[4..6]) != 0 || u16le(&end[6..8]) != 0
        || u16le(&end[20..22]) != 0 { return Err(TapError::InvalidZip); }
    let count = u16le(&end[10..12]) as usize;
    let central_size = u32le(&end[12..16]) as u64;
    let central_offset = u32le(&end[16..20]) as u64;
    if count == 0 || count > MAX_ENTRIES || central_offset + central_size > length {
        return Err(TapError::Limit);
    }
    file.seek(SeekFrom::Start(central_offset))?;
    let mut entries = Vec::with_capacity(count);
    let mut total = 0u64;
    for _ in 0..count {
        let mut fixed = [0u8; 46];
        file.read_exact(&mut fixed)?;
        if fixed[0..4] != [0x50, 0x4b, 0x01, 0x02] { return Err(TapError::InvalidZip); }
        let needed = u16le(&fixed[6..8]);
        let flags = u16le(&fixed[8..10]);
        let method = u16le(&fixed[10..12]);
        let compressed = u32le(&fixed[20..24]) as u64;
        let size = u32le(&fixed[24..28]) as u64;
        let name_len = u16le(&fixed[28..30]) as usize;
        let extra_len = u16le(&fixed[30..32]) as usize;
        let comment_len = u16le(&fixed[32..34]) as usize;
        let local_offset = u32le(&fixed[42..46]) as u64;
        if needed >= 45 || flags & 0x0009 != 0 || method != 0 || compressed != size
            || extra_len != 0 || comment_len != 0 || name_len == 0 || name_len > 160 {
            return Err(TapError::InvalidZip);
        }
        let mut name_bytes = vec![0u8; name_len];
        file.read_exact(&mut name_bytes)?;
        let name = std::str::from_utf8(&name_bytes).map_err(|_| TapError::InvalidPath)?.to_string();
        if !valid_path(&name) { return Err(TapError::InvalidPath); }
        total = total.checked_add(size).ok_or(TapError::Limit)?;
        entries.push(ZipEntry { name, crc32: u32le(&fixed[16..20]), size, local_offset });
    }
    if total > MAX_EXTRACTED_SIZE { return Err(TapError::Limit); }
    if !entries.windows(2).all(|pair| pair[0].name < pair[1].name) {
        return Err(TapError::InvalidZip);
    }
    Ok(entries)
}

fn entry_data_offset(file: &mut File, entry: &ZipEntry) -> Result<u64, TapError> {
    file.seek(SeekFrom::Start(entry.local_offset))?;
    let mut fixed = [0u8; 30];
    file.read_exact(&mut fixed)?;
    if fixed[0..4] != [0x50, 0x4b, 0x03, 0x04]
        || u16le(&fixed[6..8]) & 0x0009 != 0 || u16le(&fixed[8..10]) != 0
        || u32le(&fixed[18..22]) as u64 != entry.size
        || u32le(&fixed[22..26]) as u64 != entry.size {
        return Err(TapError::InvalidZip);
    }
    let name_len = u16le(&fixed[26..28]) as usize;
    let extra_len = u16le(&fixed[28..30]) as usize;
    let mut name = vec![0u8; name_len];
    file.read_exact(&mut name)?;
    if name != entry.name.as_bytes() || extra_len != 0 { return Err(TapError::InvalidZip); }
    Ok(entry.local_offset + 30 + name_len as u64)
}

struct Crc32(u32);
impl Crc32 {
    fn new() -> Self { Self(0xffff_ffff) }
    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= byte as u32;
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(self.0 & 1)));
            }
        }
    }
    fn finish(self) -> u32 { !self.0 }
}

fn read_entry(file: &mut File, entry: &ZipEntry, max: u64) -> Result<Vec<u8>, TapError> {
    if entry.size > max { return Err(TapError::Limit); }
    let offset = entry_data_offset(file, entry)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0u8; entry.size as usize];
    file.read_exact(&mut data)?;
    let mut crc = Crc32::new();
    crc.update(&data);
    if crc.finish() != entry.crc32 { return Err(TapError::Integrity); }
    Ok(data)
}

fn find_entry<'a>(entries: &'a [ZipEntry], name: &str) -> Result<&'a ZipEntry, TapError> {
    entries.iter().find(|entry| entry.name == name).ok_or(TapError::InvalidManifest)
}

fn json_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, TapError> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or(TapError::InvalidManifest)
}

fn validate_manifest(
    package: &Value, metadata: &Value, entries: &[ZipEntry],
    expected_id: &str, expected_version: &str, expected_sequence: u32,
) -> Result<Vec<(String, String, u64)>, TapError> {
    if json_string(package, "schema")? != "thistle.app.package/v1"
        || json_string(package, "type")? != "app" { return Err(TapError::InvalidManifest); }
    let id = json_string(package, "id")?;
    let version = json_string(package, "version")?;
    let sequence = package.get("release_sequence").and_then(Value::as_u64).ok_or(TapError::InvalidManifest)?;
    if id != expected_id || version != expected_version || sequence != expected_sequence as u64
        || json_string(metadata, "id")? != id { return Err(TapError::Mismatch); }
    let releases = metadata.get("releases").and_then(Value::as_array).ok_or(TapError::InvalidManifest)?;
    let latest = releases.first().ok_or(TapError::InvalidManifest)?;
    if json_string(latest, "version")? != version
        || latest.get("release_sequence").and_then(Value::as_u64) != Some(sequence) {
        return Err(TapError::Mismatch);
    }
    if json_string(package, "entry")? != "app.app.elf"
        || json_string(package, "signature")? != "app.app.elf.sig" { return Err(TapError::InvalidManifest); }
    let files = package.get("files").and_then(Value::as_array).ok_or(TapError::InvalidManifest)?;
    let mut declared = Vec::with_capacity(files.len());
    for item in files {
        let path = json_string(item, "path")?;
        let digest = json_string(item, "sha256")?;
        let size = item.get("size_bytes").and_then(Value::as_u64).ok_or(TapError::InvalidManifest)?;
        if !valid_path(path) || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || size == 0 || size > MAX_ELF_SIZE || declared.iter().any(|(p, _, _)| p == path) {
            return Err(TapError::InvalidManifest);
        }
        declared.push((path.to_string(), digest.to_ascii_lowercase(), size));
    }
    let payloads: Vec<&str> = entries.iter().map(|e| e.name.as_str())
        .filter(|name| !matches!(*name, "package.json" | "metadata.json" | "LICENSE" | "NOTICE")).collect();
    if payloads.len() != declared.len() || declared.iter().any(|(path, _, _)| !payloads.contains(&path.as_str())) {
        return Err(TapError::Mismatch);
    }
    Ok(declared)
}

fn extract_entry(file: &mut File, entry: &ZipEntry, target: &Path, expected_hash: Option<&str>) -> Result<(), TapError> {
    let offset = entry_data_offset(file, entry)?;
    file.seek(SeekFrom::Start(offset))?;
    if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
    let mut output = OpenOptions::new().write(true).create_new(true).open(target)?;
    let mut remaining = entry.size;
    let mut buffer = [0u8; 4096];
    let mut crc = Crc32::new();
    let mut hash = Sha256::new();
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])?;
        output.write_all(&buffer[..count])?;
        crc.update(&buffer[..count]);
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    output.flush()?;
    output.sync_all()?;
    if crc.finish() != entry.crc32 { return Err(TapError::Integrity); }
    if let Some(expected) = expected_hash {
        let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected { return Err(TapError::Integrity); }
    }
    Ok(())
}

fn active_sequence(path: &Path) -> Result<Option<u64>, TapError> {
    if !path.exists() { return Ok(None); }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    value.get("release_sequence").and_then(Value::as_u64).map(Some).ok_or(TapError::InvalidManifest)
}

/// Install an already package-signature-verified TAP and atomically activate it.
pub fn install_tap_with<F>(
    tap_path: &Path, apps_root: &Path, expected_id: &str,
    expected_version: &str, expected_sequence: u32,
    verify_package: impl FnOnce(&Path) -> bool,
    verify_elf: F,
) -> Result<PathBuf, TapError>
where F: FnOnce(&Path) -> bool {
    let mut file = File::open(tap_path)?;
    let entries = read_entries(&mut file)?;
    for required in ["package.json", "metadata.json", "app.app.elf", "app.app.elf.sig", "LICENSE"] {
        find_entry(&entries, required)?;
    }
    let package: Value = serde_json::from_slice(&read_entry(&mut file, find_entry(&entries, "package.json")?, MAX_JSON_SIZE)?)?;
    let metadata: Value = serde_json::from_slice(&read_entry(&mut file, find_entry(&entries, "metadata.json")?, MAX_JSON_SIZE)?)?;
    let declared = validate_manifest(&package, &metadata, &entries, expected_id, expected_version, expected_sequence)?;
    let app_dir = apps_root.join(expected_id);
    let generations = app_dir.join("generations");
    let destination = generations.join(expected_sequence.to_string());
    if destination.exists() { return Err(TapError::Exists); }
    if active_sequence(&app_dir.join("active.json"))?.is_some_and(|active| (expected_sequence as u64) < active) {
        return Err(TapError::Downgrade);
    }
    std::fs::create_dir_all(&generations)?;
    let stage = generations.join(format!(".{expected_sequence}.stage-{}", STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&stage)?;
    let result = (|| {
        for entry in &entries {
            let expected = declared.iter().find(|(path, _, _)| path == &entry.name);
            if let Some((_, _, size)) = expected {
                if *size != entry.size { return Err(TapError::Integrity); }
            }
            let digest = expected.map(|(_, digest, _)| digest.as_str());
            extract_entry(&mut file, entry, &stage.join(&entry.name), digest)?;
        }
        let elf = stage.join("app.app.elf");
        // Recheck the complete archive immediately before activation. The
        // caller also verifies before parsing, so neither untrusted metadata
        // nor a package changed during extraction can become active.
        if !verify_package(tap_path) { return Err(TapError::Signature); }
        if !verify_elf(&elf) { return Err(TapError::Signature); }
        std::fs::rename(&stage, &destination)?;
        let active_tmp = app_dir.join(".active.json.tmp");
        std::fs::write(&active_tmp, format!("{{\n  \"release_sequence\": {expected_sequence},\n  \"version\": \"{expected_version}\"\n}}\n"))?;
        std::fs::rename(active_tmp, app_dir.join("active.json"))?;
        Ok(destination.clone())
    })();
    if result.is_err() { let _ = std::fs::remove_dir_all(&stage); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
    fn push_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }

    fn crc(bytes: &[u8]) -> u32 {
        let mut value = Crc32::new();
        value.update(bytes);
        value.finish()
    }

    fn write_stored_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
        let mut archive = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = archive.len() as u32;
            push_u32(&mut archive, 0x0403_4b50);
            push_u16(&mut archive, 20); push_u16(&mut archive, 0x800);
            push_u16(&mut archive, 0); push_u16(&mut archive, 0); push_u16(&mut archive, 0);
            push_u32(&mut archive, crc(data)); push_u32(&mut archive, data.len() as u32);
            push_u32(&mut archive, data.len() as u32); push_u16(&mut archive, name.len() as u16);
            push_u16(&mut archive, 0); archive.extend_from_slice(name.as_bytes()); archive.extend_from_slice(data);

            push_u32(&mut central, 0x0201_4b50); push_u16(&mut central, 0x0314);
            push_u16(&mut central, 20); push_u16(&mut central, 0x800); push_u16(&mut central, 0);
            push_u16(&mut central, 0); push_u16(&mut central, 0); push_u32(&mut central, crc(data));
            push_u32(&mut central, data.len() as u32); push_u32(&mut central, data.len() as u32);
            push_u16(&mut central, name.len() as u16); push_u16(&mut central, 0); push_u16(&mut central, 0);
            push_u16(&mut central, 0); push_u16(&mut central, 0); push_u32(&mut central, 0o100644 << 16);
            push_u32(&mut central, offset); central.extend_from_slice(name.as_bytes());
        }
        let central_offset = archive.len() as u32;
        let central_size = central.len() as u32;
        archive.extend_from_slice(&central);
        push_u32(&mut archive, 0x0605_4b50); push_u16(&mut archive, 0); push_u16(&mut archive, 0);
        push_u16(&mut archive, entries.len() as u16); push_u16(&mut archive, entries.len() as u16);
        push_u32(&mut archive, central_size); push_u32(&mut archive, central_offset); push_u16(&mut archive, 0);
        std::fs::write(path, archive).unwrap();
    }

    fn fixture(root: &Path) -> PathBuf {
        let elf = b"host-elf".to_vec();
        let signature = vec![0x5a; 64];
        let digest = |data: &[u8]| -> String {
            Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
        };
        let package = format!(
            "{{\"schema\":\"thistle.app.package/v1\",\"type\":\"app\",\"id\":\"com.thistle.notes\",\"name\":\"Notes\",\"version\":\"1.0.0\",\"release_sequence\":1,\"author\":\"ThistleOS\",\"description\":\"Notes\",\"min_os\":\"0.3.0\",\"arch\":\"host\",\"entry\":\"app.app.elf\",\"signature\":\"app.app.elf.sig\",\"permissions\":[\"storage\"],\"background\":false,\"min_memory_kb\":48,\"files\":[{{\"path\":\"app.app.elf\",\"sha256\":\"{}\",\"size_bytes\":{}}},{{\"path\":\"app.app.elf.sig\",\"sha256\":\"{}\",\"size_bytes\":64}}]}}",
            digest(&elf), elf.len(), digest(&signature)
        ).into_bytes();
        let metadata = b"{\"schema\":\"thistle.app.metadata/v1\",\"id\":\"com.thistle.notes\",\"category\":\"productivity\",\"summary\":\"Notes\",\"description\":\"Notes\",\"license\":\"BSD-3-Clause\",\"releases\":[{\"version\":\"1.0.0\",\"release_sequence\":1,\"released\":\"2026-08-29\",\"changes\":[\"Initial\"]}]}".to_vec();
        let path = root.join("notes.tap");
        write_stored_zip(&path, &[
            ("LICENSE", b"BSD-3-Clause".to_vec()),
            ("app.app.elf", elf),
            ("app.app.elf.sig", signature),
            ("metadata.json", metadata),
            ("package.json", package),
        ]);
        path
    }

    #[test]
    fn paths_reject_traversal_and_noncanonical_names() {
        for invalid in ["", "/app", "../app", "a/../b", "a//b", "a/./b", "a\\b", "dir/"] {
            assert!(!valid_path(invalid), "accepted {invalid:?}");
        }
        assert!(valid_path("assets/icon-1bit.bin"));
    }

    #[test]
    fn crc32_matches_zip_reference_vector() {
        let mut crc = Crc32::new();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xcbf4_3926);
    }

    #[test]
    fn signed_generation_is_extracted_then_activated() {
        let root = std::env::temp_dir().join(format!("thistle-tap-test-{}", STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let tap = fixture(&root);
        let apps = root.join("apps");
        let destination = install_tap_with(
            &tap, &apps, "com.thistle.notes", "1.0.0", 1,
            |_| true,
            |elf| elf.is_file() && PathBuf::from(format!("{}.sig", elf.display())).is_file(),
        ).unwrap();
        assert_eq!(std::fs::read(destination.join("app.app.elf")).unwrap(), b"host-elf");
        let active = std::fs::read_to_string(apps.join("com.thistle.notes/active.json")).unwrap();
        assert!(active.contains("\"release_sequence\": 1"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_elf_signature_leaves_no_active_generation() {
        let root = std::env::temp_dir().join(format!("thistle-tap-badsig-{}", STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let tap = fixture(&root);
        let apps = root.join("apps");
        assert_eq!(
            install_tap_with(&tap, &apps, "com.thistle.notes", "1.0.0", 1, |_| true, |_| false),
            Err(TapError::Signature)
        );
        assert!(!apps.join("com.thistle.notes/active.json").exists());
        assert!(!apps.join("com.thistle.notes/generations/1").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_final_package_signature_leaves_no_active_generation() {
        let root = std::env::temp_dir().join(format!("thistle-tap-bad-package-sig-{}", STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let tap = fixture(&root);
        let apps = root.join("apps");
        assert_eq!(
            install_tap_with(&tap, &apps, "com.thistle.notes", "1.0.0", 1, |_| false, |_| true),
            Err(TapError::Signature)
        );
        assert!(!apps.join("com.thistle.notes/active.json").exists());
        assert!(!apps.join("com.thistle.notes/generations/1").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
