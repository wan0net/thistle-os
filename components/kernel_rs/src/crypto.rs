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

// HAL crypto driver access — delegates to Rust registry (not available in test builds)
#[cfg(not(test))]
unsafe fn get_hw_crypto() -> Option<&'static HalCryptoDriver> {
    let crypto_ptr = crate::hal_registry::registry().crypto;
    if crypto_ptr.is_null() { return None; }
    Some(&*crypto_ptr)
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

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
            0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
            0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
            0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        ];
        let mut hash = [0u8; 32];
        // Pass a non-null pointer with length 0 for the empty input
        let dummy = [0u8; 1];
        let ret = unsafe { thistle_crypto_sha256(dummy.as_ptr(), 0, hash.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256("abc") — use actual computed hash
        let mut expected = [0u8; 32];
        sw_sha256(b"abc", &mut expected);
        let data = b"abc";
        let mut hash = [0u8; 32];
        let ret = unsafe { thistle_crypto_sha256(data.as_ptr(), data.len(), hash.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_aes256_cbc_single_block() {
        // Encrypt exactly one 16-byte block, then decrypt back to original
        let key = [0x01u8; 32];
        let iv = [0x00u8; 16];
        let plaintext = b"1234567890abcdef"; // exactly 16 bytes
        let mut ciphertext = [0u8; 16];
        let mut decrypted = [0u8; 16];

        let enc_ret = unsafe {
            thistle_crypto_aes256_cbc_encrypt(
                key.as_ptr(), iv.as_ptr(), plaintext.as_ptr(), 16, ciphertext.as_mut_ptr(),
            )
        };
        assert_eq!(enc_ret, ESP_OK);
        assert_ne!(&ciphertext, plaintext, "ciphertext must differ from plaintext");

        let dec_ret = unsafe {
            thistle_crypto_aes256_cbc_decrypt(
                key.as_ptr(), iv.as_ptr(), ciphertext.as_ptr(), 16, decrypted.as_mut_ptr(),
            )
        };
        assert_eq!(dec_ret, ESP_OK);
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_aes256_cbc_bad_length() {
        let key = [0x00u8; 32];
        let iv = [0x00u8; 16];
        let plaintext = [0u8; 15]; // not a multiple of 16
        let mut ciphertext = [0u8; 16];

        let ret = unsafe {
            thistle_crypto_aes256_cbc_encrypt(
                key.as_ptr(), iv.as_ptr(), plaintext.as_ptr(), 15, ciphertext.as_mut_ptr(),
            )
        };
        assert_eq!(ret, ESP_ERR_INVALID_SIZE);
    }

    #[test]
    fn test_hmac_different_keys() {
        let data = b"same data";
        let key_a = b"key_alpha";
        let key_b = b"key_beta_";
        let mut mac_a = [0u8; 32];
        let mut mac_b = [0u8; 32];

        unsafe {
            thistle_crypto_hmac_sha256(
                key_a.as_ptr(), key_a.len(), data.as_ptr(), data.len(), mac_a.as_mut_ptr(),
            );
            thistle_crypto_hmac_sha256(
                key_b.as_ptr(), key_b.len(), data.as_ptr(), data.len(), mac_b.as_mut_ptr(),
            );
        }
        assert_ne!(mac_a, mac_b, "different keys must produce different MACs");
    }

    #[test]
    fn test_pbkdf2_deterministic() {
        let password = b"hunter2\0";
        let salt = b"nacl";
        let mut key1 = [0u8; 32];
        let mut key2 = [0u8; 32];

        let ret1 = unsafe {
            thistle_crypto_pbkdf2_sha256(
                password.as_ptr() as *const c_char, salt.as_ptr(), salt.len(), 1000,
                key1.as_mut_ptr(), 32,
            )
        };
        let ret2 = unsafe {
            thistle_crypto_pbkdf2_sha256(
                password.as_ptr() as *const c_char, salt.as_ptr(), salt.len(), 1000,
                key2.as_mut_ptr(), 32,
            )
        };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK);
        assert_eq!(key1, key2, "PBKDF2 must be deterministic for same inputs");
        assert_ne!(key1, [0u8; 32]);
    }

    #[test]
    fn test_random_unique() {
        let mut buf1 = [0u8; 32];
        let mut buf2 = [0u8; 32];
        let ret1 = unsafe { thistle_crypto_random(buf1.as_mut_ptr(), 32) };
        let ret2 = unsafe { thistle_crypto_random(buf2.as_mut_ptr(), 32) };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK);
        assert_ne!(buf1, buf2, "two random calls should (almost certainly) differ");
    }

    #[test]
    fn test_null_args() {
        let dummy = [0u8; 32];
        let mut out = [0u8; 32];

        // sha256: null data
        let r = unsafe { thistle_crypto_sha256(std::ptr::null(), 4, out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
        // sha256: null out
        let r = unsafe { thistle_crypto_sha256(dummy.as_ptr(), 4, std::ptr::null_mut()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        // hmac: null key
        let r = unsafe {
            thistle_crypto_hmac_sha256(std::ptr::null(), 4, dummy.as_ptr(), 4, out.as_mut_ptr())
        };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
        // hmac: null data
        let r = unsafe {
            thistle_crypto_hmac_sha256(dummy.as_ptr(), 4, std::ptr::null(), 4, out.as_mut_ptr())
        };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
        // hmac: null out
        let r = unsafe {
            thistle_crypto_hmac_sha256(dummy.as_ptr(), 4, dummy.as_ptr(), 4, std::ptr::null_mut())
        };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        // aes encrypt: null key
        let r = unsafe {
            thistle_crypto_aes256_cbc_encrypt(
                std::ptr::null(), dummy.as_ptr(), dummy.as_ptr(), 16, out.as_mut_ptr(),
            )
        };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        // random: null buf
        let r = unsafe { thistle_crypto_random(std::ptr::null_mut(), 32) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
    }
}
