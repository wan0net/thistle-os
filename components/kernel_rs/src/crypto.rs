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
use ed25519_dalek::{Signer, Verifier};

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_FAIL: i32 = -1;

type HmacSha256 = Hmac<Sha256>;

// ── HAL crypto driver vtable — use the type from hal_registry ────────

use crate::hal_registry::HalCryptoDriver;

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

fn sw_aes128_ecb_encrypt_block(key: &[u8; 16], block_in: &[u8; 16], block_out: &mut [u8; 16]) {
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut ga = GenericArray::clone_from_slice(block_in);
    cipher.encrypt_block(&mut ga);
    block_out.copy_from_slice(ga.as_slice());
}

fn sw_aes128_ecb_decrypt_block(key: &[u8; 16], block_in: &[u8; 16], block_out: &mut [u8; 16]) {
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut ga = GenericArray::clone_from_slice(block_in);
    cipher.decrypt_block(&mut ga);
    block_out.copy_from_slice(ga.as_slice());
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

    // Compute HMAC via hardware if available, then compare
    let mut computed = [0u8; 32];
    let ret = thistle_crypto_hmac_sha256(key, key_len, data, data_len, computed.as_mut_ptr());
    if ret != ESP_OK { return ret; }

    // Constant-time comparison
    let expected = std::slice::from_raw_parts(expected_mac, 32);
    let mut diff: u8 = 0;
    for i in 0..32 { diff |= computed[i] ^ expected[i]; }
    if diff == 0 { ESP_OK } else { ESP_FAIL }
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

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes128_ecb_encrypt(
    key: *const u8, plaintext: *const u8, len: usize, ciphertext_out: *mut u8,
) -> i32 {
    if key.is_null() || plaintext.is_null() || ciphertext_out.is_null() { return ESP_ERR_INVALID_ARG; }
    if len == 0 || len % 16 != 0 { return ESP_ERR_INVALID_SIZE; }

    // Try hardware
    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.aes128_ecb_encrypt {
            return f(key, plaintext, len, ciphertext_out);
        }
    }
    // Software fallback
    let k = &*(key as *const [u8; 16]);
    let pt = std::slice::from_raw_parts(plaintext, len);
    let ct = std::slice::from_raw_parts_mut(ciphertext_out, len);
    for i in (0..len).step_by(16) {
        let block_in: &[u8; 16] = pt[i..i+16].try_into().unwrap();
        let block_out: &mut [u8; 16] = (&mut ct[i..i+16]).try_into().unwrap();
        sw_aes128_ecb_encrypt_block(k, block_in, block_out);
    }
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_aes128_ecb_decrypt(
    key: *const u8, ciphertext: *const u8, len: usize, plaintext_out: *mut u8,
) -> i32 {
    if key.is_null() || ciphertext.is_null() || plaintext_out.is_null() { return ESP_ERR_INVALID_ARG; }
    if len == 0 || len % 16 != 0 { return ESP_ERR_INVALID_SIZE; }

    // Try hardware
    if let Some(hw) = get_hw_crypto() {
        if let Some(f) = hw.aes128_ecb_decrypt {
            return f(key, ciphertext, len, plaintext_out);
        }
    }
    // Software fallback
    let k = &*(key as *const [u8; 16]);
    let ct = std::slice::from_raw_parts(ciphertext, len);
    let pt = std::slice::from_raw_parts_mut(plaintext_out, len);
    for i in (0..len).step_by(16) {
        let block_in: &[u8; 16] = ct[i..i+16].try_into().unwrap();
        let block_out: &mut [u8; 16] = (&mut pt[i..i+16]).try_into().unwrap();
        sw_aes128_ecb_decrypt_block(k, block_in, block_out);
    }
    ESP_OK
}

// ── Ed25519 (software-only, uses ed25519-dalek) ─────────────────────

/// Generate a new Ed25519 keypair.
/// private_key_out: 32 bytes (seed)
/// public_key_out: 32 bytes
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_ed25519_keygen(
    private_key_out: *mut u8,  // 32 bytes seed
    public_key_out: *mut u8,   // 32 bytes
) -> i32 {
    if private_key_out.is_null() || public_key_out.is_null() { return ESP_ERR_INVALID_ARG; }

    // Generate 32 random bytes for the seed
    let mut seed = [0u8; 32];
    let ret = thistle_crypto_random(seed.as_mut_ptr(), 32);
    if ret != ESP_OK { return ret; }

    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    std::ptr::copy_nonoverlapping(seed.as_ptr(), private_key_out, 32);
    std::ptr::copy_nonoverlapping(verifying_key.as_bytes().as_ptr(), public_key_out, 32);
    ESP_OK
}

/// Sign a message with Ed25519.
/// private_key: 32 bytes (seed)
/// signature_out: 64 bytes
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_ed25519_sign(
    private_key: *const u8,    // 32 bytes seed
    message: *const u8, msg_len: usize,
    signature_out: *mut u8,    // 64 bytes
) -> i32 {
    if private_key.is_null() || message.is_null() || signature_out.is_null() { return ESP_ERR_INVALID_ARG; }

    let seed = &*(private_key as *const [u8; 32]);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
    let msg = std::slice::from_raw_parts(message, msg_len);
    let sig = signing_key.sign(msg);
    std::ptr::copy_nonoverlapping(sig.to_bytes().as_ptr(), signature_out, 64);
    ESP_OK
}

/// Verify an Ed25519 signature.
/// Returns ESP_OK (0) if valid, 1 if invalid.
/// public_key: 32 bytes
/// signature: 64 bytes
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_ed25519_verify(
    public_key: *const u8,     // 32 bytes
    message: *const u8, msg_len: usize,
    signature: *const u8,      // 64 bytes
) -> i32 {
    if public_key.is_null() || message.is_null() || signature.is_null() { return ESP_ERR_INVALID_ARG; }

    let pk_bytes = &*(public_key as *const [u8; 32]);
    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(pk_bytes) {
        Ok(vk) => vk,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let sig_bytes = &*(signature as *const [u8; 64]);
    let sig = ed25519_dalek::Signature::from_bytes(sig_bytes);
    let msg = std::slice::from_raw_parts(message, msg_len);
    match verifying_key.verify(msg, &sig) {
        Ok(()) => 0,  // Valid
        Err(_) => 1,  // Invalid signature
    }
}

/// Derive public key from private key (seed).
/// private_key: 32 bytes (seed)
/// public_key_out: 32 bytes
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_ed25519_derive_public(
    private_key: *const u8,    // 32 bytes seed
    public_key_out: *mut u8,   // 32 bytes
) -> i32 {
    if private_key.is_null() || public_key_out.is_null() { return ESP_ERR_INVALID_ARG; }

    let seed = &*(private_key as *const [u8; 32]);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
    let verifying_key = signing_key.verifying_key();
    std::ptr::copy_nonoverlapping(verifying_key.as_bytes().as_ptr(), public_key_out, 32);
    ESP_OK
}

// ── X25519 key exchange (Ed25519 → X25519 conversion + ECDH) ─────────

/// X25519 Diffie-Hellman key exchange.
/// Converts Ed25519 keys to X25519 format, then performs ECDH.
/// ed25519_private_key: 32 bytes (Ed25519 seed)
/// other_ed25519_public_key: 32 bytes (Ed25519 public key)
/// shared_secret_out: 32 bytes
#[no_mangle]
pub unsafe extern "C" fn thistle_crypto_x25519_key_exchange(
    ed25519_private_key: *const u8,      // 32 bytes seed
    other_ed25519_public_key: *const u8,  // 32 bytes
    shared_secret_out: *mut u8,           // 32 bytes
) -> i32 {
    if ed25519_private_key.is_null() || other_ed25519_public_key.is_null() || shared_secret_out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let seed = std::slice::from_raw_parts(ed25519_private_key, 32);

    // Ed25519 → X25519 private key conversion: SHA-512(seed)[0..32], clamped
    use sha2::{Sha512, Digest as Sha512Digest};
    let hash = Sha512::digest(seed);
    let mut x_prv = [0u8; 32];
    x_prv.copy_from_slice(&hash[..32]);
    x_prv[0] &= 248;
    x_prv[31] &= 127;
    x_prv[31] |= 64;

    let secret = x25519_dalek::StaticSecret::from(x_prv);

    // Ed25519 → X25519 public key conversion: Edwards → Montgomery
    let ed_pub_bytes = &*(other_ed25519_public_key as *const [u8; 32]);
    let compressed = curve25519_dalek::edwards::CompressedEdwardsY(*ed_pub_bytes);
    match compressed.decompress() {
        Some(edwards) => {
            let montgomery = edwards.to_montgomery();
            let x25519_pub = x25519_dalek::PublicKey::from(montgomery.to_bytes());
            let shared = secret.diffie_hellman(&x25519_pub);
            std::ptr::copy_nonoverlapping(shared.as_bytes().as_ptr(), shared_secret_out, 32);
            ESP_OK
        }
        None => ESP_FAIL,
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
        assert_eq!(ret, ESP_FAIL);
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

    #[test]
    fn test_aes128_ecb_roundtrip() {
        let key = [0x42u8; 16];
        let plaintext = [0xABu8; 32]; // two blocks
        let mut ciphertext = [0u8; 32];
        let mut decrypted = [0u8; 32];

        let ret = unsafe {
            thistle_crypto_aes128_ecb_encrypt(key.as_ptr(), plaintext.as_ptr(), 32, ciphertext.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(ciphertext, plaintext);
        // ECB: identical plaintext blocks produce identical ciphertext blocks
        assert_eq!(ciphertext[..16], ciphertext[16..32]);

        let ret = unsafe {
            thistle_crypto_aes128_ecb_decrypt(key.as_ptr(), ciphertext.as_ptr(), 32, decrypted.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_aes128_ecb_single_block() {
        let key = [0x01u8; 16];
        let plaintext = b"1234567890abcdef";
        let mut ciphertext = [0u8; 16];
        let mut decrypted = [0u8; 16];

        let ret = unsafe {
            thistle_crypto_aes128_ecb_encrypt(key.as_ptr(), plaintext.as_ptr(), 16, ciphertext.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(&ciphertext, plaintext);

        let ret = unsafe {
            thistle_crypto_aes128_ecb_decrypt(key.as_ptr(), ciphertext.as_ptr(), 16, decrypted.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_aes128_ecb_bad_length() {
        let key = [0u8; 16];
        let plaintext = [0u8; 15];
        let mut ciphertext = [0u8; 16];
        let ret = unsafe {
            thistle_crypto_aes128_ecb_encrypt(key.as_ptr(), plaintext.as_ptr(), 15, ciphertext.as_mut_ptr())
        };
        assert_eq!(ret, ESP_ERR_INVALID_SIZE);
    }

    #[test]
    fn test_ed25519_sign_verify() {
        let mut privkey = [0u8; 32];
        let mut pubkey = [0u8; 32];
        let ret = unsafe { thistle_crypto_ed25519_keygen(privkey.as_mut_ptr(), pubkey.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);
        assert_ne!(privkey, [0u8; 32]);
        assert_ne!(pubkey, [0u8; 32]);

        let message = b"hello ThistleOS";
        let mut signature = [0u8; 64];
        let ret = unsafe {
            thistle_crypto_ed25519_sign(privkey.as_ptr(), message.as_ptr(), message.len(), signature.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);

        // Verify valid signature
        let ret = unsafe {
            thistle_crypto_ed25519_verify(pubkey.as_ptr(), message.as_ptr(), message.len(), signature.as_ptr())
        };
        assert_eq!(ret, 0, "valid signature must verify");

        // Tamper with signature
        let mut bad_sig = signature;
        bad_sig[0] ^= 0xFF;
        let ret = unsafe {
            thistle_crypto_ed25519_verify(pubkey.as_ptr(), message.as_ptr(), message.len(), bad_sig.as_ptr())
        };
        assert_eq!(ret, 1, "tampered signature must fail");

        // Wrong message
        let wrong_msg = b"wrong message!!";
        let ret = unsafe {
            thistle_crypto_ed25519_verify(pubkey.as_ptr(), wrong_msg.as_ptr(), wrong_msg.len(), signature.as_ptr())
        };
        assert_eq!(ret, 1, "wrong message must fail");
    }

    #[test]
    fn test_ed25519_derive_public() {
        let mut privkey = [0u8; 32];
        let mut pubkey = [0u8; 32];
        let ret = unsafe { thistle_crypto_ed25519_keygen(privkey.as_mut_ptr(), pubkey.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);

        let mut derived = [0u8; 32];
        let ret = unsafe { thistle_crypto_ed25519_derive_public(privkey.as_ptr(), derived.as_mut_ptr()) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(pubkey, derived, "derived public key must match keygen output");
    }

    #[test]
    fn test_ed25519_null_args() {
        let mut out = [0u8; 64];
        let dummy = [0u8; 32];

        let r = unsafe { thistle_crypto_ed25519_keygen(std::ptr::null_mut(), out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        let r = unsafe { thistle_crypto_ed25519_sign(std::ptr::null(), dummy.as_ptr(), 4, out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        let r = unsafe { thistle_crypto_ed25519_verify(std::ptr::null(), dummy.as_ptr(), 4, dummy.as_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);

        let r = unsafe { thistle_crypto_ed25519_derive_public(std::ptr::null(), out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_x25519_key_exchange() {
        // Generate two keypairs
        let mut prv_a = [0u8; 32];
        let mut pub_a = [0u8; 32];
        let mut prv_b = [0u8; 32];
        let mut pub_b = [0u8; 32];
        unsafe {
            thistle_crypto_ed25519_keygen(prv_a.as_mut_ptr(), pub_a.as_mut_ptr());
            thistle_crypto_ed25519_keygen(prv_b.as_mut_ptr(), pub_b.as_mut_ptr());
        }

        // Both sides compute shared secret
        let mut secret_ab = [0u8; 32];
        let mut secret_ba = [0u8; 32];
        let ret1 = unsafe {
            thistle_crypto_x25519_key_exchange(prv_a.as_ptr(), pub_b.as_ptr(), secret_ab.as_mut_ptr())
        };
        let ret2 = unsafe {
            thistle_crypto_x25519_key_exchange(prv_b.as_ptr(), pub_a.as_ptr(), secret_ba.as_mut_ptr())
        };
        assert_eq!(ret1, ESP_OK);
        assert_eq!(ret2, ESP_OK);
        assert_eq!(secret_ab, secret_ba, "both sides must derive the same shared secret");
        assert_ne!(secret_ab, [0u8; 32], "shared secret must not be zero");
    }

    #[test]
    fn test_x25519_null_args() {
        let dummy = [0u8; 32];
        let mut out = [0u8; 32];
        let r = unsafe { thistle_crypto_x25519_key_exchange(std::ptr::null(), dummy.as_ptr(), out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
        let r = unsafe { thistle_crypto_x25519_key_exchange(dummy.as_ptr(), std::ptr::null(), out.as_mut_ptr()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
        let r = unsafe { thistle_crypto_x25519_key_exchange(dummy.as_ptr(), dummy.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(r, ESP_ERR_INVALID_ARG);
    }
}
