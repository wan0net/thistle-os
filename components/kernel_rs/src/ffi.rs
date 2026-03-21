// SPDX-License-Identifier: BSD-3-Clause
// C FFI exports for the Rust kernel.
//
// Each function here is `#[no_mangle] extern "C"` and matches the signature
// of the corresponding C function it replaces. The C code calls these through
// the same headers — no changes needed on the C side.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;

use crate::manifest::Manifest;

/// C-compatible manifest struct — matches thistle_manifest_t exactly.
/// Field sizes must match the C header (manifest.h).
#[repr(C)]
pub struct CManifest {
    pub manifest_type: u8, // ManifestType enum value
    pub id: [u8; 64],
    pub name: [u8; 32],
    pub version: [u8; 16],
    pub author: [u8; 32],
    pub description: [u8; 128],
    pub min_os: [u8; 16],
    pub arch: [u8; 16],
    pub entry: [u8; 64],
    pub icon: [u8; 64],
    pub permissions: u32,
    pub background: bool,
    pub min_memory_kb: u32,
    pub hal_interface: [u8; 16],
    pub changelog: [u8; 256],
}

/// Copy a Rust string into a fixed-size C buffer (null-terminated).
fn copy_to_buf(src: &str, dst: &mut [u8]) {
    let bytes = src.as_bytes();
    let len = bytes.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&bytes[..len]);
    dst[len] = 0;
    // Zero the rest
    for b in &mut dst[len + 1..] {
        *b = 0;
    }
}

impl From<&Manifest> for CManifest {
    fn from(m: &Manifest) -> Self {
        let mut c = CManifest {
            manifest_type: m.manifest_type as u8,
            id: [0; 64],
            name: [0; 32],
            version: [0; 16],
            author: [0; 32],
            description: [0; 128],
            min_os: [0; 16],
            arch: [0; 16],
            entry: [0; 64],
            icon: [0; 64],
            permissions: m.permissions,
            background: m.background,
            min_memory_kb: m.min_memory_kb,
            hal_interface: [0; 16],
            changelog: [0; 256],
        };
        copy_to_buf(&m.id, &mut c.id);
        copy_to_buf(&m.name, &mut c.name);
        copy_to_buf(&m.version, &mut c.version);
        copy_to_buf(&m.author, &mut c.author);
        copy_to_buf(&m.description, &mut c.description);
        copy_to_buf(&m.min_os, &mut c.min_os);
        copy_to_buf(&m.arch, &mut c.arch);
        copy_to_buf(&m.entry, &mut c.entry);
        copy_to_buf(&m.icon, &mut c.icon);
        copy_to_buf(&m.hal_interface, &mut c.hal_interface);
        copy_to_buf(&m.changelog, &mut c.changelog);
        c
    }
}

// ESP-IDF error codes
const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
#[allow(dead_code)]
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;

/// Parse a manifest.json file — drop-in replacement for C manifest_parse_file().
///
/// # Safety
/// `json_path` must be a valid null-terminated C string.
/// `out` must point to a valid CManifest-sized buffer.
#[no_mangle]
pub unsafe extern "C" fn manifest_parse_file(
    json_path: *const c_char,
    out: *mut CManifest,
) -> i32 {
    if json_path.is_null() || out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let path_str = match CStr::from_ptr(json_path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    match Manifest::from_file(Path::new(path_str)) {
        Ok(m) => {
            *out = CManifest::from(&m);
            ESP_OK
        }
        Err(crate::manifest::ManifestError::NotFound) => ESP_ERR_NOT_FOUND,
        Err(_) => ESP_ERR_INVALID_ARG,
    }
}

/// Check manifest compatibility — drop-in for C manifest_is_compatible().
///
/// # Safety
/// `manifest` must point to a valid CManifest.
/// `current_arch` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn manifest_is_compatible(
    manifest: *const CManifest,
    current_arch: *const c_char,
) -> bool {
    if manifest.is_null() {
        return true;
    }

    let m = &*manifest;
    let arch = if current_arch.is_null() {
        ""
    } else {
        CStr::from_ptr(current_arch).to_str().unwrap_or("")
    };

    // Check arch
    let m_arch = CStr::from_ptr(m.arch.as_ptr() as *const c_char)
        .to_str()
        .unwrap_or("");
    if !m_arch.is_empty() && m_arch != arch {
        return false;
    }

    // Check min_os
    let m_min_os = CStr::from_ptr(m.min_os.as_ptr() as *const c_char)
        .to_str()
        .unwrap_or("");
    if !m_min_os.is_empty() && !crate::version::satisfies(m_min_os) {
        return false;
    }

    true
}

/// Derive manifest path from ELF path.
///
/// # Safety
/// `elf_path` must be a valid null-terminated C string.
/// `out_path` must point to a buffer of at least `out_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn manifest_path_from_elf(
    elf_path: *const c_char,
    out_path: *mut c_char,
    out_size: usize,
) {
    if elf_path.is_null() || out_path.is_null() || out_size == 0 {
        return;
    }

    let path_str = match CStr::from_ptr(elf_path).to_str() {
        Ok(s) => s,
        Err(_) => return,
    };

    let result = Manifest::path_from_elf(path_str);
    let bytes = result.as_bytes();
    let len = bytes.len().min(out_size - 1);

    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_path as *mut u8, len);
    *out_path.add(len) = 0;
}

/// Get kernel version string.
///
/// # Safety
/// Returns a pointer to a static string. Do not free.
#[no_mangle]
pub extern "C" fn kernel_version() -> *const c_char {
    // Include the null terminator
    b"0.1.0\0".as_ptr() as *const c_char
}
