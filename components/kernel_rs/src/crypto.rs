// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — crypto module
//
// Platform-independent cryptographic primitives available to all apps
// via the syscall table. Uses pure Rust crates — works on ESP32, desktop,
// and WASM without any platform-specific crypto library.
//
// Primitives:
//   SHA-256, HMAC-SHA256, AES-256-CBC, PBKDF2-SHA256, CSPRNG

use std::os::raw::c_char;

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::{Sha256, Digest};
use aes::cipher::{BlockEncrypt, BlockDecrypt, KeyInit, generic_array::GenericArray};

// ESP error codes
const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_FAIL: i32 = -1;

type HmacSha256 = Hmac<Sha256>;

// ── SHA-256 ─────────────────────────────────────────────────────────

/// Compute SHA-256 hash of `data` (len bytes), write 32 bytes to `hash_out`.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_sha256(
    data: *const u8,
    len: usize,
    hash_out: *mut u8,
) -> i32 {
    if data.is_null() || hash_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let input = std::slice::from_raw_parts(data, len);
    let result = Sha256::digest(input);
    std::ptr::copy_nonoverlapping(result.as_ptr(), hash_out, 32);
    ESP_OK
}

// ── HMAC-SHA256 ─────────────────────────────────────────────────────

/// Compute HMAC-SHA256. Key is `key_len` bytes, data is `data_len` bytes.
/// Writes 32 bytes to `mac_out`.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_hmac_sha256(
    key: *const u8,
    key_len: usize,
    data: *const u8,
    data_len: usize,
    mac_out: *mut u8,
) -> i32 {
    if key.is_null() || data.is_null() || mac_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let key_slice = std::slice::from_raw_parts(key, key_len);
    let data_slice = std::slice::from_raw_parts(data, data_len);

    let mut mac = match <HmacSha256 as Mac>::new_from_slice(key_slice) {
        Ok(m) => m,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    mac.update(data_slice);
    let result = mac.finalize().into_bytes();
    std::ptr::copy_nonoverlapping(result.as_ptr(), mac_out, 32);
    ESP_OK
}

/// Constant-time HMAC comparison. Returns 0 if equal, non-zero if different.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_hmac_verify(
    key: *const u8,
    key_len: usize,
    data: *const u8,
    data_len: usize,
    expected_mac: *const u8,
) -> i32 {
    if key.is_null() || data.is_null() || expected_mac.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let key_slice = std::slice::from_raw_parts(key, key_len);
    let data_slice = std::slice::from_raw_parts(data, data_len);
    let expected = std::slice::from_raw_parts(expected_mac, 32);

    let mut mac = match <HmacSha256 as Mac>::new_from_slice(key_slice) {
        Ok(m) => m,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    mac.update(data_slice);

    // verify() is constant-time
    match mac.verify_slice(expected) {
        Ok(()) => 0,  // match
        Err(_) => 1,  // mismatch
    }
}

// ── AES-256-CBC ─────────────────────────────────────────────────────

/// AES-256-CBC encrypt. `len` must be a multiple of 16 (caller pads).
/// `key` is 32 bytes, `iv` is 16 bytes.
/// Writes `len` bytes to `ciphertext_out`.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes256_cbc_encrypt(
    key: *const u8,
    iv: *const u8,
    plaintext: *const u8,
    len: usize,
    ciphertext_out: *mut u8,
) -> i32 {
    if key.is_null() || iv.is_null() || plaintext.is_null() || ciphertext_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if len == 0 || len % 16 != 0 {
        return ESP_ERR_INVALID_SIZE;
    }

    let key_arr: &[u8; 32] = &*(key as *const [u8; 32]);
    let iv_arr: &[u8; 16] = &*(iv as *const [u8; 16]);

    let pt = std::slice::from_raw_parts(plaintext, len);
    let ct = std::slice::from_raw_parts_mut(ciphertext_out, len);
    let cipher = aes::Aes256::new(GenericArray::from_slice(key_arr));

    // CBC encrypt: ct[i] = AES(pt[i] XOR ct[i-1]), ct[-1] = IV
    let mut prev = *iv_arr;
    for i in (0..len).step_by(16) {
        let mut block = [0u8; 16];
        for j in 0..16 { block[j] = pt[i + j] ^ prev[j]; }
        let mut ga = GenericArray::clone_from_slice(&block);
        cipher.encrypt_block(&mut ga);
        ct[i..i+16].copy_from_slice(ga.as_slice());
        prev.copy_from_slice(&ct[i..i+16]);
    }
    ESP_OK
}

/// AES-256-CBC decrypt. `len` must be a multiple of 16.
/// `key` is 32 bytes, `iv` is 16 bytes.
/// Writes `len` bytes to `plaintext_out`.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes256_cbc_decrypt(
    key: *const u8,
    iv: *const u8,
    ciphertext: *const u8,
    len: usize,
    plaintext_out: *mut u8,
) -> i32 {
    if key.is_null() || iv.is_null() || ciphertext.is_null() || plaintext_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    if len == 0 || len % 16 != 0 {
        return ESP_ERR_INVALID_SIZE;
    }

    let key_arr: &[u8; 32] = &*(key as *const [u8; 32]);
    let iv_arr: &[u8; 16] = &*(iv as *const [u8; 16]);

    let ct_slice = std::slice::from_raw_parts(ciphertext, len);
    let pt = std::slice::from_raw_parts_mut(plaintext_out, len);
    let cipher = aes::Aes256::new(GenericArray::from_slice(key_arr));

    // CBC decrypt: pt[i] = AES_dec(ct[i]) XOR ct[i-1], ct[-1] = IV
    let mut prev = *iv_arr;
    for i in (0..len).step_by(16) {
        let mut ga = GenericArray::clone_from_slice(&ct_slice[i..i+16]);
        cipher.decrypt_block(&mut ga);
        for j in 0..16 { pt[i + j] = ga[j] ^ prev[j]; }
        prev.copy_from_slice(&ct_slice[i..i+16]);
    }
    ESP_OK
}

// ── PBKDF2-SHA256 ───────────────────────────────────────────────────

/// Derive a key using PBKDF2-HMAC-SHA256.
/// `password` is a C string, `salt` is `salt_len` bytes.
/// Writes `key_len` bytes to `key_out`.
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_pbkdf2_sha256(
    password: *const c_char,
    salt: *const u8,
    salt_len: usize,
    iterations: u32,
    key_out: *mut u8,
    key_len: usize,
) -> i32 {
    if password.is_null() || salt.is_null() || key_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let pw = std::ffi::CStr::from_ptr(password).to_bytes();
    let salt_slice = std::slice::from_raw_parts(salt, salt_len);
    let mut dk = vec![0u8; key_len];

    pbkdf2_hmac::<Sha256>(pw, salt_slice, iterations, &mut dk);

    std::ptr::copy_nonoverlapping(dk.as_ptr(), key_out, key_len);
    ESP_OK
}

// ── CSPRNG ──────────────────────────────────────────────────────────

/// Fill `buf` with `len` cryptographically secure random bytes.
/// Uses OS entropy source (ESP-IDF hw RNG, /dev/urandom, or Web Crypto).
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_random(
    buf: *mut u8,
    len: usize,
) -> i32 {
    if buf.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let slice = std::slice::from_raw_parts_mut(buf, len);
    match getrandom::getrandom(slice) {
        Ok(()) => ESP_OK,
        Err(_) => ESP_FAIL,
    }
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
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(hash[0], 0x2c);
        assert_eq!(hash[1], 0xf2);
    }

    #[test]
    fn test_hmac_sha256() {
        let key = b"secret";
        let data = b"message";
        let mut mac = [0u8; 32];
        let ret = unsafe {
            thistle_crypto_hmac_sha256(
                key.as_ptr(), key.len(),
                data.as_ptr(), data.len(),
                mac.as_mut_ptr(),
            )
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(mac, [0u8; 32]); // non-zero

        // Verify passes
        let ret = unsafe {
            thistle_crypto_hmac_verify(
                key.as_ptr(), key.len(),
                data.as_ptr(), data.len(),
                mac.as_ptr(),
            )
        };
        assert_eq!(ret, 0); // match

        // Tamper → verify fails
        let mut bad_mac = mac;
        bad_mac[0] ^= 0xFF;
        let ret = unsafe {
            thistle_crypto_hmac_verify(
                key.as_ptr(), key.len(),
                data.as_ptr(), data.len(),
                bad_mac.as_ptr(),
            )
        };
        assert_eq!(ret, 1); // mismatch
    }

    #[test]
    fn test_pbkdf2() {
        let password = b"password\0";
        let salt = b"saltsalt";
        let mut key = [0u8; 32];
        let ret = unsafe {
            thistle_crypto_pbkdf2_sha256(
                password.as_ptr() as *const c_char,
                salt.as_ptr(), salt.len(),
                1000,
                key.as_mut_ptr(), 32,
            )
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(key, [0u8; 32]);
    }

    #[test]
    fn test_aes256_cbc_roundtrip() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = [0xABu8; 32]; // 2 blocks
        let mut ciphertext = [0u8; 32];
        let mut decrypted = [0u8; 32];

        let ret = unsafe {
            thistle_crypto_aes256_cbc_encrypt(
                key.as_ptr(), iv.as_ptr(),
                plaintext.as_ptr(), 32,
                ciphertext.as_mut_ptr(),
            )
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(ciphertext, plaintext); // encrypted is different

        let ret = unsafe {
            thistle_crypto_aes256_cbc_decrypt(
                key.as_ptr(), iv.as_ptr(),
                ciphertext.as_ptr(), 32,
                decrypted.as_mut_ptr(),
            )
        };
        assert_eq!(ret, ESP_OK);
        assert_eq!(decrypted, plaintext); // roundtrip
    }

    #[test]
    fn test_random() {
        let mut buf = [0u8; 32];
        let ret = unsafe { thistle_crypto_random(buf.as_mut_ptr(), 32) };
        assert_eq!(ret, ESP_OK);
        assert_ne!(buf, [0u8; 32]); // extremely unlikely to be all zeros
    }
}
