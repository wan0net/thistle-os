// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — signing module
//
// Replaces signing.c / Monocypher with ed25519-dalek (BSD-3-Clause).
// Exposes a C-compatible FFI with the same symbol names as the original C API.

use core::ffi::c_char;
use std::ffi::CStr;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_INVALID_CRC: i32 = 0x109;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static VERIFYING_KEY: Mutex<Option<VerifyingKey>> = Mutex::new(None);

/// 64 hex chars + NUL terminator. Written once during signing_init(),
/// never modified after. Safe to return a pointer without holding a lock.
static mut HEX_BUF: [u8; 65] = [0u8; 65];

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn bytes_to_hex(bytes: &[u8], out: &mut [u8]) {
    const HEX: &[u8] = b"0123456789abcdef";
    for (i, &b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
}

fn sig_path_for(elf_path: &str) -> String {
    format!("{}.sig", elf_path)
}

fn read_file_bounded(path: &str, max_size: usize) -> Result<Vec<u8>, i32> {
    let file = fs::File::open(path).map_err(|_| ESP_ERR_NOT_FOUND)?;
    let mut bytes = Vec::new();
    file.take((max_size + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ESP_ERR_NOT_FOUND)?;
    if bytes.len() > max_size {
        return Err(ESP_ERR_INVALID_SIZE);
    }
    Ok(bytes)
}

/// Verify an already-read file image against the detached signature associated
/// with `path`. Callers that execute or install the image can keep using the
/// same byte slice after this returns, avoiding a pathname-based TOCTOU gap.
pub(crate) fn verify_file_bytes(path: &str, file_bytes: &[u8]) -> i32 {
    const MAX_VERIFY_SIZE: usize = 16 * 1024 * 1024;
    if file_bytes.len() > MAX_VERIFY_SIZE {
        return ESP_ERR_INVALID_SIZE;
    }

    let sig_bytes = match read_file_bounded(&sig_path_for(path), 64) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    if sig_bytes.len() != 64 {
        return ESP_ERR_INVALID_SIZE;
    }

    unsafe { signing_verify(file_bytes.as_ptr(), file_bytes.len(), sig_bytes.as_ptr()) }
}

/// Stream one already-open object through Ed25519 verification and a consumer.
/// The consumer receives the exact byte slices covered by the signature. This
/// lets OTA write those slices to an inactive partition without reopening a
/// mutable pathname between verification and use.
pub(crate) fn verify_exact_reader_with(
    reader: &mut impl Read,
    expected_size: usize,
    signature: &[u8],
    mut consume: impl FnMut(&[u8]) -> Result<(), i32>,
) -> Result<(), i32> {
    if expected_size == 0 || expected_size > 16 * 1024 * 1024 || signature.len() != 64 {
        return Err(ESP_ERR_INVALID_SIZE);
    }

    let key_guard = VERIFYING_KEY.lock().map_err(|_| ESP_ERR_NO_MEM)?;
    let verifying_key = key_guard.as_ref().ok_or(ESP_ERR_INVALID_STATE)?;
    let sig_bytes: [u8; 64] = signature.try_into().map_err(|_| ESP_ERR_INVALID_SIZE)?;
    let signature = Signature::from_bytes(&sig_bytes);
    let mut verifier = verifying_key.verify_stream(&signature).map_err(|_| ESP_ERR_INVALID_CRC)?;

    let mut buffer = [0u8; 4096];
    let mut consumed = 0usize;
    while consumed < expected_size {
        let wanted = buffer.len().min(expected_size - consumed);
        let count = reader.read(&mut buffer[..wanted]).map_err(|_| ESP_ERR_INVALID_SIZE)?;
        if count == 0 {
            return Err(ESP_ERR_INVALID_SIZE);
        }
        verifier.update(&buffer[..count]);
        consume(&buffer[..count])?;
        consumed += count;
    }

    let mut extra = [0u8; 1];
    if reader.read(&mut extra).map_err(|_| ESP_ERR_INVALID_SIZE)? != 0 {
        return Err(ESP_ERR_INVALID_SIZE);
    }

    verifier.finalize_and_verify().map_err(|_| ESP_ERR_INVALID_CRC)
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the signing subsystem with a 32-byte Ed25519 public key.
///
/// # Safety
/// `key` must point to at least 32 valid bytes.
#[no_mangle]
pub unsafe extern "C" fn signing_init(key: *const u8) -> i32 {
    if key.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let key_bytes: [u8; 32] = match std::slice::from_raw_parts(key, 32).try_into() {
        Ok(b) => b,
        Err(_) => return ESP_ERR_INVALID_SIZE,
    };

    let verifying_key = match VerifyingKey::from_bytes(&key_bytes) {
        Ok(k) => k,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    // Build hex representation while we have the bytes.
    let mut hex = [0u8; 65];
    bytes_to_hex(&key_bytes, &mut hex[..64]);
    hex[64] = 0;

    // Write hex buf and verifying key under the same lock to avoid data races.
    match VERIFYING_KEY.lock() {
        Ok(mut guard) => {
            HEX_BUF = hex;
            *guard = Some(verifying_key);
        }
        Err(_) => return ESP_ERR_NO_MEM,
    }

    ESP_OK
}

/// Verify `data_len` bytes at `data` against the 64-byte `signature`.
///
/// Returns `ESP_ERR_INVALID_STATE` if the module has not been initialised.
/// Returns `ESP_ERR_INVALID_CRC` if the signature does not verify.
///
/// # Safety
/// `data` must point to at least `data_len` valid bytes.
/// `signature` must point to at least 64 valid bytes.
#[no_mangle]
pub unsafe extern "C" fn signing_verify(
    data: *const u8,
    data_len: usize,
    signature: *const u8,
) -> i32 {
    if data.is_null() || signature.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let key_guard = match VERIFYING_KEY.lock() {
        Ok(g) => g,
        Err(_) => return ESP_ERR_NO_MEM,
    };

    let verifying_key = match key_guard.as_ref() {
        Some(k) => k,
        None => return ESP_ERR_INVALID_STATE,
    };

    let data_slice = std::slice::from_raw_parts(data, data_len);

    let sig_bytes: [u8; 64] = match std::slice::from_raw_parts(signature, 64).try_into() {
        Ok(b) => b,
        Err(_) => return ESP_ERR_INVALID_SIZE,
    };
    let sig = Signature::from_bytes(&sig_bytes);

    match verifying_key.verify(data_slice, &sig) {
        Ok(()) => ESP_OK,
        Err(_) => ESP_ERR_INVALID_CRC,
    }
}

/// Read `<elf_path>.sig` and the ELF file, then verify the signature.
///
/// # Safety
/// `elf_path` must be a valid, NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn signing_verify_file(elf_path: *const c_char) -> i32 {
    if elf_path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let path_str = match CStr::from_ptr(elf_path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };

    const MAX_VERIFY_SIZE: usize = 16 * 1024 * 1024;
    let elf_bytes = match read_file_bounded(path_str, MAX_VERIFY_SIZE) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    verify_file_bytes(path_str, &elf_bytes)
}

/// Return `true` if `<elf_path>.sig` exists on the filesystem.
///
/// # Safety
/// `elf_path` must be a valid, NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn signing_has_signature(elf_path: *const c_char) -> bool {
    if elf_path.is_null() {
        return false;
    }

    let path_str = match CStr::from_ptr(elf_path).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    Path::new(&sig_path_for(path_str)).exists()
}

/// Return a pointer to a static NUL-terminated hex string of the stored public key.
///
/// Returns a pointer to an empty string if the module has not been initialised.
/// The returned pointer remains valid for the lifetime of the process.
#[no_mangle]
pub extern "C" fn signing_get_public_key_hex() -> *const c_char {
    // Safety: HEX_BUF is written once during signing_init() and never modified after.
    // The returned pointer is valid for the lifetime of the process.
    unsafe { HEX_BUF.as_ptr() as *const c_char }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::ffi::CString;

    /// Reset global state between tests so they are independent.
    fn reset() {
        if let Ok(mut g) = VERIFYING_KEY.lock() {
            *g = None;
        }
        unsafe { HEX_BUF = [0u8; 65]; }
    }

    fn fresh_verifying_key_bytes() -> [u8; 32] {
        // Use a deterministic key derived from a fixed seed so tests are
        // reproducible without a CSPRNG dependency in the test suite.
        let seed = [0x42u8; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        signing_key.verifying_key().to_bytes()
    }

    fn temp_payload_path(case: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "thistle-signing-{case}-{}-{}.elf",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn streamed_verification_binds_exact_consumed_bytes_before_activation() {
        reset();
        let signing_key = SigningKey::from_bytes(&[0x42u8; 32]);
        assert_eq!(unsafe { signing_init(signing_key.verifying_key().as_bytes().as_ptr()) }, ESP_OK);
        let payload = vec![0x5au8; 9000];
        let signature = signing_key.sign(&payload).to_bytes();

        let mut flashed = Vec::new();
        let result = verify_exact_reader_with(
            &mut std::io::Cursor::new(&payload),
            payload.len(),
            &signature,
            |chunk| {
                flashed.extend_from_slice(chunk);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(flashed, payload);

        let mut swapped = payload.clone();
        swapped[4097] ^= 1;
        let mut boot_selected = false;
        let result = verify_exact_reader_with(
            &mut std::io::Cursor::new(&swapped),
            swapped.len(),
            &signature,
            |_| Ok(()),
        );
        if result.is_ok() {
            boot_selected = true;
        }
        assert_eq!(result, Err(ESP_ERR_INVALID_CRC));
        assert!(!boot_selected);
    }

    #[test]
    fn streamed_verification_rejects_truncation_and_overrun() {
        reset();
        let signing_key = SigningKey::from_bytes(&[0x42u8; 32]);
        assert_eq!(unsafe { signing_init(signing_key.verifying_key().as_bytes().as_ptr()) }, ESP_OK);
        let payload = b"signed firmware bytes";
        let signature = signing_key.sign(payload).to_bytes();

        assert_eq!(
            verify_exact_reader_with(
                &mut std::io::Cursor::new(&payload[..payload.len() - 1]),
                payload.len(),
                &signature,
                |_| Ok(()),
            ),
            Err(ESP_ERR_INVALID_SIZE)
        );
        assert_eq!(
            verify_exact_reader_with(
                &mut std::io::Cursor::new(payload),
                payload.len() - 1,
                &signature,
                |_| Ok(()),
            ),
            Err(ESP_ERR_INVALID_SIZE)
        );
    }

    fn remove_payload_fixture(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(sig_path_for(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn test_init() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        let result = unsafe { signing_init(key_bytes.as_ptr()) };
        assert_eq!(result, ESP_OK);
    }

    #[test]
    fn test_verify_rejects_bad_sig() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        unsafe { signing_init(key_bytes.as_ptr()) };

        let data = b"hello thistle";
        let zero_sig = [0u8; 64];
        let result =
            unsafe { signing_verify(data.as_ptr(), data.len(), zero_sig.as_ptr()) };
        assert_eq!(result, ESP_ERR_INVALID_CRC);
    }

    #[test]
    fn test_hex_output() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        unsafe { signing_init(key_bytes.as_ptr()) };

        let ptr = signing_get_public_key_hex();
        assert!(!ptr.is_null());
        let hex_str = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert_eq!(hex_str.len(), 64);
        assert!(hex_str.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_not_initialized() {
        reset();
        let data = b"some data";
        let zero_sig = [0u8; 64];
        let result =
            unsafe { signing_verify(data.as_ptr(), data.len(), zero_sig.as_ptr()) };
        assert_eq!(result, ESP_ERR_INVALID_STATE);
    }

    #[test]
    fn test_verify_valid_signature() {
        use ed25519_dalek::Signer;

        reset();
        let seed = [0x11u8; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key_bytes = signing_key.verifying_key().to_bytes();

        let init_result = unsafe { signing_init(verifying_key_bytes.as_ptr()) };
        assert_eq!(init_result, ESP_OK);

        let data = b"thistleos payload";
        let sig: ed25519_dalek::Signature = signing_key.sign(data);
        let sig_bytes = sig.to_bytes();

        let result = unsafe { signing_verify(data.as_ptr(), data.len(), sig_bytes.as_ptr()) };
        assert_eq!(result, ESP_OK, "valid signature must verify successfully");
    }

    #[test]
    fn test_verify_wrong_key() {
        use ed25519_dalek::Signer;

        reset();
        // Sign with key A
        let seed_a = [0x22u8; 32];
        let signing_key_a = SigningKey::from_bytes(&seed_a);
        let data = b"hello from key a";
        let sig: ed25519_dalek::Signature = signing_key_a.sign(data);
        let sig_bytes = sig.to_bytes();

        // Init with key B (different)
        let seed_b = [0x33u8; 32];
        let signing_key_b = SigningKey::from_bytes(&seed_b);
        let verifying_key_b = signing_key_b.verifying_key().to_bytes();

        unsafe { signing_init(verifying_key_b.as_ptr()) };

        let result = unsafe { signing_verify(data.as_ptr(), data.len(), sig_bytes.as_ptr()) };
        assert_eq!(result, ESP_ERR_INVALID_CRC, "signature from wrong key must fail");
    }

    #[test]
    fn test_verify_tampered_data() {
        use ed25519_dalek::Signer;

        reset();
        let seed = [0x44u8; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key_bytes = signing_key.verifying_key().to_bytes();

        unsafe { signing_init(verifying_key_bytes.as_ptr()) };

        let original_data = b"original data";
        let sig: ed25519_dalek::Signature = signing_key.sign(original_data);
        let sig_bytes = sig.to_bytes();

        // Tamper: use different data with the original signature
        let tampered_data = b"tampered data!!";
        let result =
            unsafe { signing_verify(tampered_data.as_ptr(), tampered_data.len(), sig_bytes.as_ptr()) };
        assert_eq!(result, ESP_ERR_INVALID_CRC, "tampered data must not verify");
    }

    #[test]
    fn test_init_twice() {
        reset();
        // First init with key A
        let seed_a = [0x55u8; 32];
        let vk_a = SigningKey::from_bytes(&seed_a).verifying_key().to_bytes();
        let r1 = unsafe { signing_init(vk_a.as_ptr()) };
        assert_eq!(r1, ESP_OK);

        // Second init with key B — must also succeed (replaces stored key)
        let seed_b = [0x66u8; 32];
        let vk_b = SigningKey::from_bytes(&seed_b).verifying_key().to_bytes();
        let r2 = unsafe { signing_init(vk_b.as_ptr()) };
        assert_eq!(r2, ESP_OK, "calling signing_init a second time must succeed");
    }

    #[test]
    fn test_null_data() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        unsafe { signing_init(key_bytes.as_ptr()) };

        let zero_sig = [0u8; 64];
        // null data pointer
        let result = unsafe { signing_verify(std::ptr::null(), 10, zero_sig.as_ptr()) };
        assert_eq!(result, ESP_ERR_INVALID_ARG, "null data must return ESP_ERR_INVALID_ARG");

        // null signature pointer
        let data = b"some data";
        let result = unsafe { signing_verify(data.as_ptr(), data.len(), std::ptr::null()) };
        assert_eq!(result, ESP_ERR_INVALID_ARG, "null signature must return ESP_ERR_INVALID_ARG");
    }

    // -----------------------------------------------------------------------
    // test_has_signature_nonexistent_path
    // Mirrors test_signing.c: signing_has_signature on a path that doesn't
    // exist on disk must return false.
    // -----------------------------------------------------------------------

    #[test]
    fn test_has_signature_nonexistent_path() {
        let path = b"/nonexistent/path/that/does/not/exist.elf\0";
        let result = unsafe { signing_has_signature(path.as_ptr() as *const c_char) };
        assert!(!result, "signing_has_signature must return false for missing file");
    }

    // -----------------------------------------------------------------------
    // test_verify_file_not_found
    // Mirrors test_signing.c: signing_verify_file on a missing path returns
    // ESP_ERR_NOT_FOUND.
    // -----------------------------------------------------------------------

    #[test]
    fn test_verify_file_not_found() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        unsafe { signing_init(key_bytes.as_ptr()) };

        let path = b"/no/such/file.elf\0";
        let result = unsafe { signing_verify_file(path.as_ptr() as *const c_char) };
        assert_eq!(
            result, ESP_ERR_NOT_FOUND,
            "signing_verify_file on missing path must return ESP_ERR_NOT_FOUND"
        );
    }

    #[test]
    fn test_verify_file_rejects_63_and_65_byte_signatures() {
        reset();
        let payload_path = temp_payload_path("bad-length");
        remove_payload_fixture(&payload_path);
        let signature_path = sig_path_for(payload_path.to_string_lossy().as_ref());
        std::fs::write(&payload_path, b"signed payload").unwrap();
        let c_path = CString::new(payload_path.to_string_lossy().as_bytes()).unwrap();

        for length in [63, 65] {
            std::fs::write(&signature_path, vec![0u8; length]).unwrap();
            assert_eq!(
                unsafe { signing_verify_file(c_path.as_ptr()) },
                ESP_ERR_INVALID_SIZE,
                "{length}-byte signature must fail closed"
            );
        }

        remove_payload_fixture(&payload_path);
    }

    #[test]
    fn test_verify_file_rejects_unreadable_payload() {
        reset();
        let payload_path = temp_payload_path("unreadable");
        remove_payload_fixture(&payload_path);
        std::fs::create_dir(&payload_path).unwrap();
        let signature_path = sig_path_for(payload_path.to_string_lossy().as_ref());
        std::fs::write(&signature_path, [0u8; 64]).unwrap();
        let c_path = CString::new(payload_path.to_string_lossy().as_bytes()).unwrap();

        assert_eq!(
            unsafe { signing_verify_file(c_path.as_ptr()) },
            ESP_ERR_NOT_FOUND
        );

        let _ = std::fs::remove_file(signature_path);
        let _ = std::fs::remove_dir(payload_path);
    }

    #[test]
    fn test_verify_file_accepts_valid_and_rejects_tampered_payload() {
        reset();
        let payload_path = temp_payload_path("valid-and-tampered");
        remove_payload_fixture(&payload_path);
        let signature_path = sig_path_for(payload_path.to_string_lossy().as_ref());
        let payload = b"production driver or firmware payload";
        let signing_key = SigningKey::from_bytes(&[0x77; 32]);
        let verifying_key = signing_key.verifying_key().to_bytes();
        assert_eq!(unsafe { signing_init(verifying_key.as_ptr()) }, ESP_OK);

        std::fs::write(&payload_path, payload).unwrap();
        std::fs::write(&signature_path, signing_key.sign(payload).to_bytes()).unwrap();
        let c_path = CString::new(payload_path.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { signing_verify_file(c_path.as_ptr()) }, ESP_OK);

        std::fs::write(&payload_path, b"tampered payload").unwrap();
        assert_eq!(
            unsafe { signing_verify_file(c_path.as_ptr()) },
            ESP_ERR_INVALID_CRC
        );

        remove_payload_fixture(&payload_path);
    }

    // -----------------------------------------------------------------------
    // test_signing_init_null_key
    // Mirrors test_signing.c: signing_init(NULL) must return INVALID_ARG.
    // -----------------------------------------------------------------------

    #[test]
    fn test_signing_init_null_key() {
        reset();
        let result = unsafe { signing_init(std::ptr::null()) };
        assert_eq!(
            result, ESP_ERR_INVALID_ARG,
            "signing_init(NULL) must return ESP_ERR_INVALID_ARG"
        );
    }

    // -----------------------------------------------------------------------
    // test_verify_zero_length_data
    // Mirrors test_signing.c: signing_verify with data_len == 0 and a bad sig
    // must return INVALID_CRC (not panic).
    // -----------------------------------------------------------------------

    #[test]
    fn test_verify_zero_length_data() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();
        unsafe { signing_init(key_bytes.as_ptr()) };

        let zero_sig = [0u8; 64];
        let dummy: [u8; 1] = [0];
        let result = unsafe { signing_verify(dummy.as_ptr(), 0, zero_sig.as_ptr()) };
        assert_eq!(
            result, ESP_ERR_INVALID_CRC,
            "zero-length data with bad sig must return ESP_ERR_INVALID_CRC"
        );
    }

    // -----------------------------------------------------------------------
    // test_hex_output_deterministic
    // Mirrors test_signing.c: two successive inits with the same key produce
    // the same hex output.
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_output_deterministic() {
        reset();
        let key_bytes = fresh_verifying_key_bytes();

        unsafe { signing_init(key_bytes.as_ptr()) };
        let ptr1 = signing_get_public_key_hex();
        let hex1 = unsafe { CStr::from_ptr(ptr1).to_str().unwrap().to_string() };

        // Re-init with the same key
        reset();
        unsafe { signing_init(key_bytes.as_ptr()) };
        let ptr2 = signing_get_public_key_hex();
        let hex2 = unsafe { CStr::from_ptr(ptr2).to_str().unwrap().to_string() };

        assert_eq!(hex1, hex2, "same key must produce identical hex output on two inits");
        assert_eq!(hex1.len(), 64, "hex output must be exactly 64 characters");
    }

    // -----------------------------------------------------------------------
    // test_verify_file_null_path
    // Mirrors test_signing.c: signing_verify_file(NULL) returns INVALID_ARG.
    // -----------------------------------------------------------------------

    #[test]
    fn test_verify_file_null_path() {
        reset();
        let result = unsafe { signing_verify_file(std::ptr::null()) };
        assert_eq!(
            result, ESP_ERR_INVALID_ARG,
            "signing_verify_file(NULL) must return ESP_ERR_INVALID_ARG"
        );
    }

    // -----------------------------------------------------------------------
    // test_has_signature_null_path
    // Mirrors test_signing.c: signing_has_signature(NULL) must return false.
    // -----------------------------------------------------------------------

    #[test]
    fn test_has_signature_null_path() {
        let result = unsafe { signing_has_signature(std::ptr::null()) };
        assert!(!result, "signing_has_signature(NULL) must return false");
    }
}
