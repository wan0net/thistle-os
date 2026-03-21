// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — crypto module
//
// Cryptographic primitives with hardware acceleration support.
// If a hardware crypto driver is registered via the HAL, it is used.
// Otherwise, falls back to pure Rust software implementations.
//
// Dispatch order per function:
//   1. Check HAL registry for hardware crypto driver
//   2. If driver has the function implemented (non-NULL), call it
//   3. Otherwise, use Rust software fallback
//
// This means hardware can accelerate some primitives (e.g., SHA-256)
// while software handles others (e.g., PBKDF2) — partial acceleration.

use std::os::raw::c_char;

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::{Sha256, Digest};
use aes::cipher::{BlockEncrypt, BlockDecrypt, KeyInit, generic_array::GenericArray};

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_FAIL: i32 = -1;

type HmacSha256 = Hmac<Sha256>;

// ── HAL crypto driver vtable (matches hal/crypto.h) ─────────────────

#[repr(C)]
struct HalCryptoDriver {
    sha256: Option<unsafe extern "C" fn(*const u8, usize, *mut u8) -> i32>,
    aes256_cbc_encrypt: Option<unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32>,
    aes256_cbc_decrypt: Option<unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32>,
    hmac_sha256: Option<unsafe extern "C" fn(*const u8, usize, *const u8, usize, *mut u8) -> i32>,
    random: Option<unsafe extern "C" fn(*mut u8, usize) -> i32>,
    name: *const u8,
}

// HAL crypto driver access — not available in test builds
#[cfg(not(test))]
extern "C" {
    fn hal_crypto_get() -> *const std::os::raw::c_void;
}

#[cfg(not(test))]
unsafe fn get_hw_crypto() -> Option<&'static HalCryptoDriver> {
    let crypto_ptr = hal_crypto_get();
    if crypto_ptr.is_null() { return None; }
    Some(&*(crypto_ptr as *const HalCryptoDriver))
}

#[cfg(test)]
unsafe fn get_hw_crypto() -> Option<&'static HalCryptoDriver> {
    None // Tests always use software fallback
}

// ── Software implementations (pure Rust) ────────────────────────────

fn sw_sha256(data: &[u8], hash_out: &mut [u8; 32]) {
    let result = Sha256::digest(data);
    hash_out.copy_from_slice(&result);
}

fn sw_hmac_sha256(key: &[u8], data: &[u8], mac_out: &mut [u8; 32]) {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).unwrap();
    mac.update(data);
    let result = mac.finalize().into_bytes();
    mac_out.copy_from_slice(&result);
}

fn sw_aes256_cbc_encrypt(key: &[u8; 32], iv: &[u8; 16], pt: &[u8], ct: &mut [u8]) {
    let cipher = aes::Aes256::new(GenericArray::from_slice(key));
    let mut prev = *iv;
    for i in (0..pt.len()).step_by(16) {
        let mut block = [0u8; 16];
        for j in 0..16 { block[j] = pt[i + j] ^ prev[j]; }
        let mut ga = GenericArray::clone_from_slice(&block);
        cipher.encrypt_block(&mut ga);
        ct[i..i+16].copy_from_slice(ga.as_slice());
        prev.copy_from_slice(&ct[i..i+16]);
    }
}

fn sw_aes256_cbc_decrypt(key: &[u8; 32], iv: &[u8; 16], ct: &[u8], pt: &mut [u8]) {
    let cipher = aes::Aes256::new(GenericArray::from_slice(key));
    let mut prev = *iv;
    for i in (0..ct.len()).step_by(16) {
        let mut ga = GenericArray::clone_from_slice(&ct[i..i+16]);
        cipher.decrypt_block(&mut ga);
        for j in 0..16 { pt[i + j] = ga[j] ^ prev[j]; }
        prev.copy_from_slice(&ct[i..i+16]);
    }
}

fn sw_random(buf: &mut [u8]) -> bool {
    getrandom::getrandom(buf).is_ok()
}

// ── FFI exports (dispatch: hardware first, software fallback) ───────

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_sha256(
    data: *const u8, len: usize, hash_out: *mut u8,
) -> i32 {
    if data.is_null() || hash_out.is_null() { return ESP_ERR_INVALID_ARG; }

    // Try hardware
    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.sha256 {
            return f(data, len, hash_out);
        }
    }

    // Software fallback
    let input = std::slice::from_raw_parts(data, len);
    let mut hash = [0u8; 32];
    sw_sha256(input, &mut hash);
    std::ptr::copy_nonoverlapping(hash.as_ptr(), hash_out, 32);
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_hmac_sha256(
    key: *const u8, key_len: usize,
    data: *const u8, data_len: usize,
    mac_out: *mut u8,
) -> i32 {
    if key.is_null() || data.is_null() || mac_out.is_null() { return ESP_ERR_INVALID_ARG; }

    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.hmac_sha256 {
            return f(key, key_len, data, data_len, mac_out);
        }
    }

    let key_slice = std::slice::from_raw_parts(key, key_len);
    let data_slice = std::slice::from_raw_parts(data, data_len);
    let mut mac = [0u8; 32];
    sw_hmac_sha256(key_slice, data_slice, &mut mac);
    std::ptr::copy_nonoverlapping(mac.as_ptr(), mac_out, 32);
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_hmac_verify(
    key: *const u8, key_len: usize,
    data: *const u8, data_len: usize,
    expected_mac: *const u8,
) -> i32 {
    if key.is_null() || data.is_null() || expected_mac.is_null() { return ESP_ERR_INVALID_ARG; }

    let key_slice = std::slice::from_raw_parts(key, key_len);
    let data_slice = std::slice::from_raw_parts(data, data_len);
    let expected = std::slice::from_raw_parts(expected_mac, 32);

    let mut mac = <HmacSha256 as Mac>::new_from_slice(key_slice).unwrap();
    mac.update(data_slice);
    match mac.verify_slice(expected) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes256_cbc_encrypt(
    key: *const u8, iv: *const u8,
    plaintext: *const u8, len: usize,
    ciphertext_out: *mut u8,
) -> i32 {
    if key.is_null() || iv.is_null() || plaintext.is_null() || ciphertext_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if len == 0 || len % 16 != 0 { return ESP_ERR_INVALID_SIZE; }

    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.aes256_cbc_encrypt {
            return f(key, iv, plaintext, len, ciphertext_out);
        }
    }

    let k = &*(key as *const [u8; 32]);
    let i = &*(iv as *const [u8; 16]);
    let pt = std::slice::from_raw_parts(plaintext, len);
    let ct = std::slice::from_raw_parts_mut(ciphertext_out, len);
    sw_aes256_cbc_encrypt(k, i, pt, ct);
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes256_cbc_decrypt(
    key: *const u8, iv: *const u8,
    ciphertext: *const u8, len: usize,
    plaintext_out: *mut u8,
) -> i32 {
    if key.is_null() || iv.is_null() || ciphertext.is_null() || plaintext_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if len == 0 || len % 16 != 0 { return ESP_ERR_INVALID_SIZE; }

    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.aes256_cbc_decrypt {
            return f(key, iv, ciphertext, len, plaintext_out);
        }
    }

    let k = &*(key as *const [u8; 32]);
    let i = &*(iv as *const [u8; 16]);
    let ct = std::slice::from_raw_parts(ciphertext, len);
    let pt = std::slice::from_raw_parts_mut(plaintext_out, len);
    sw_aes256_cbc_decrypt(k, i, ct, pt);
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_pbkdf2_sha256(
    password: *const c_char, salt: *const u8, salt_len: usize,
    iterations: u32, key_out: *mut u8, key_len: usize,
) -> i32 {
    if password.is_null() || salt.is_null() || key_out.is_null() { return ESP_ERR_INVALID_ARG; }

    // PBKDF2 is always software — no hardware accelerator provides it directly.
    // It uses HMAC-SHA256 internally, which may be hardware-accelerated.
    let pw = std::ffi::CStr::from_ptr(password).to_bytes();
    let salt_slice = std::slice::from_raw_parts(salt, salt_len);
    let mut dk = vec![0u8; key_len];
    pbkdf2_hmac::<Sha256>(pw, salt_slice, iterations, &mut dk);
    std::ptr::copy_nonoverlapping(dk.as_ptr(), key_out, key_len);
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_random(buf: *mut u8, len: usize) -> i32 {
    if buf.is_null() { return ESP_ERR_INVALID_ARG; }

    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.random {
            return f(buf, len);
        }
    }

    let slice = std::slice::from_raw_parts_mut(buf, len);
    if sw_random(slice) { ESP_OK } else { ESP_FAIL }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let data = b"hello";
        let mut hash = [0u8; 32];
        let ret = unsafe { thistle_crypto_sha256(data.as_ptr(), data.len(), hash.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(hash[0], 0x2c);
        assert_eq!(hash[1], 0xf2);
    }

    #[test]
    fn test_hmac_sha256() {
        let key = b"secret";
        let data = b"message";
        let mut mac = [0u8; 32];
        let ret = unsafe {
            thistle_crypto_hmac_sha256(key.as_ptr(), key.len(), data.as_ptr(), data.len(), mac.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(mac, [0u8; 32]);

        let ret = unsafe {
            thistle_crypto_hmac_verify(key.as_ptr(), key.len(), data.as_ptr(), data.len(), mac.as_ptr())
        };
        assert_eq!(ret, 0);

        let mut bad_mac = mac;
        bad_mac[0] ^= 0xFF;
        let ret = unsafe {
            thistle_crypto_hmac_verify(key.as_ptr(), key.len(), data.as_ptr(), data.len(), bad_mac.as_ptr())
        };
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_pbkdf2() {
        let password = b"password\0";
        let salt = b"saltsalt";
        let mut key = [0u8; 32];
        let ret = unsafe {
            thistle_crypto_pbkdf2_sha256(
                password.as_ptr() as *const c_char, salt.as_ptr(), salt.len(), 1000, key.as_mut_ptr(), 32,
            )
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(key, [0u8; 32]);
    }

    #[test]
    fn test_aes256_cbc_roundtrip() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = [0xABu8; 32];
        let mut ciphertext = [0u8; 32];
        let mut decrypted = [0u8; 32];

        let ret = unsafe {
            thistle_crypto_aes256_cbc_encrypt(key.as_ptr(), iv.as_ptr(), plaintext.as_ptr(), 32, ciphertext.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(ciphertext, plaintext);

        let ret = unsafe {
            thistle_crypto_aes256_cbc_decrypt(key.as_ptr(), iv.as_ptr(), ciphertext.as_ptr(), 32, decrypted.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_random() {
        let mut buf = [0u8; 32];
        let ret = unsafe { thistle_crypto_random(buf.as_mut_ptr(), 32) };
        assert_eq!(ret, ESP_OK);
        assert_ne!(buf, [0u8; 32]);
    }
}
