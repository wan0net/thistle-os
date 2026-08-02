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
    // `manifest_type_t` is a normal C enum, which occupies four bytes in the
    // public ABI. Do not narrow this to u8: that shifts every following field.
    pub manifest_type: u32,
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
            manifest_type: m.manifest_type as u32,
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
pub unsafe extern "C" fn manifest_parse_file(json_path: *const c_char, out: *mut CManifest) -> i32 {
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

#[cfg(test)]
mod tests {
    use super::{manifest_parse_file, CManifest, ESP_OK};
    use std::ffi::{CStr, CString};
    use std::mem::{align_of, offset_of, size_of, MaybeUninit};

    #[test]
    fn c_manifest_layout_matches_public_c_abi() {
        // These values are derived from thistle_manifest_t in
        // components/kernel/include/thistle/manifest.h, where manifest_type_t
        // is a normal four-byte C enum.
        assert_eq!(size_of::<CManifest>(), 720);
        assert_eq!(align_of::<CManifest>(), 4);
        assert_eq!(offset_of!(CManifest, manifest_type), 0);
        assert_eq!(offset_of!(CManifest, id), 4);
        assert_eq!(offset_of!(CManifest, permissions), 436);
        assert_eq!(offset_of!(CManifest, background), 440);
        assert_eq!(offset_of!(CManifest, min_memory_kb), 444);
        assert_eq!(offset_of!(CManifest, hal_interface), 448);
        assert_eq!(offset_of!(CManifest, changelog), 464);
    }

    #[test]
    fn c_manifest_storage_exceeds_legacy_opaque_buffer() {
        assert!(size_of::<CManifest>() > 512);
    }

    #[test]
    fn manifest_loaders_do_not_allocate_undersized_opaque_buffers() {
        const LEGACY_BUFFER: &str = concat!("heap_caps_malloc(", "512, MALLOC_CAP_SPIRAM)");

        assert!(!include_str!("elf_loader.rs").contains(LEGACY_BUFFER));
        assert!(!include_str!("driver_loader.rs").contains(LEGACY_BUFFER));
    }

    #[test]
    fn maximal_manifest_parse_stays_within_typed_loader_storage() {
        #[repr(C)]
        struct GuardedManifest {
            before: [u8; 32],
            manifest: MaybeUninit<CManifest>,
            after: [u8; 32],
        }

        let json = format!(
            r#"{{
                "type":"firmware",
                "id":"{}",
                "name":"{}",
                "version":"{}",
                "author":"{}",
                "description":"{}",
                "min_os":"{}",
                "arch":"{}",
                "entry":"{}",
                "icon":"{}",
                "permissions":["storage","network","radio","gps","audio","system","ipc"],
                "background":true,
                "min_memory_kb":4294967295,
                "hal_interface":"{}",
                "changelog":"{}"
            }}"#,
            "i".repeat(63),
            "n".repeat(31),
            "1".repeat(15),
            "a".repeat(31),
            "d".repeat(127),
            "0".repeat(15),
            "r".repeat(15),
            "e".repeat(63),
            "o".repeat(63),
            "h".repeat(15),
            "c".repeat(255),
        );

        let path = std::env::temp_dir().join(format!(
            "thistle-maximal-manifest-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, json).expect("write maximal manifest fixture");
        let c_path = CString::new(path.to_string_lossy().as_bytes()).unwrap();

        let mut guarded = GuardedManifest {
            before: [0xA5; 32],
            manifest: MaybeUninit::uninit(),
            after: [0x5A; 32],
        };
        let result = unsafe { manifest_parse_file(c_path.as_ptr(), guarded.manifest.as_mut_ptr()) };
        let _ = std::fs::remove_file(path);

        assert_eq!(result, ESP_OK);
        assert_eq!(guarded.before, [0xA5; 32]);
        assert_eq!(guarded.after, [0x5A; 32]);

        let manifest = unsafe { guarded.manifest.assume_init() };
        assert_eq!(
            unsafe { CStr::from_ptr(manifest.id.as_ptr().cast()) }
                .to_bytes()
                .len(),
            63
        );
        assert_eq!(
            unsafe { CStr::from_ptr(manifest.changelog.as_ptr().cast()) }
                .to_bytes()
                .len(),
            255
        );
    }
}
