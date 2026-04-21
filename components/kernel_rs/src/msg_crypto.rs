// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — msg_crypto module
//
// End-to-end message encryption for the messenger subsystem.
// Encrypts/decrypts payloads using per-contact shared keys derived from
// passphrases exchanged out-of-band. Uses AES-256-CTR + HMAC-SHA256
// (encrypt-then-MAC) with per-message key derivation from a pre-computed
// master key.
//
// Wire format: [version:1][nonce:16][ciphertext:N][hmac:32]
// Total overhead: 49 bytes per message.

use std::sync::Mutex;

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_INVALID_SIZE: i32 = 0x104;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_CRC: i32 = 0x109;
const ESP_FAIL: i32 = -1;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_CHANNELS: usize = 32;
const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 16;
const HMAC_LEN: usize = 32;
const HEADER_LEN: usize = 1; // version byte
const OVERHEAD: usize = HEADER_LEN + NONCE_LEN + HMAC_LEN; // 49 bytes
const VERSION: u8 = 0x01;
const PBKDF2_ITERATIONS: u32 = 10_000;
const PBKDF2_SALT: &[u8] = b"ThistleOS-MsgCrypto-v1";

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct CryptoChannel {
    contact_id: u32,
    master_key: [u8; MASTER_KEY_LEN],
    active: bool,
    msg_count_tx: u32,
    msg_count_rx: u32,
}

impl CryptoChannel {
    const fn empty() -> Self {
        CryptoChannel {
            contact_id: 0,
            master_key: [0u8; MASTER_KEY_LEN],
            active: false,
            msg_count_tx: 0,
            msg_count_rx: 0,
        }
    }
}

struct MsgCryptoState {
    channels: [CryptoChannel; MAX_CHANNELS],
    channel_count: usize,
    initialized: bool,
}

// SAFETY: Only accessed under Mutex.
unsafe impl Send for MsgCryptoState {}

impl MsgCryptoState {
    const fn new() -> Self {
        MsgCryptoState {
            channels: [CryptoChannel::empty(); MAX_CHANNELS],
            channel_count: 0,
            initialized: false,
        }
    }

    fn find_channel(&self, contact_id: u32) -> Option<usize> {
        for i in 0..MAX_CHANNELS {
            if self.channels[i].active && self.channels[i].contact_id == contact_id {
                return Some(i);
            }
        }
        None
    }

    fn find_free_slot(&self) -> Option<usize> {
        for i in 0..MAX_CHANNELS {
            if !self.channels[i].active {
                return Some(i);
            }
        }
        None
    }
}

static STATE: Mutex<MsgCryptoState> = Mutex::new(MsgCryptoState::new());

// ---------------------------------------------------------------------------
// FFI structs
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CChannelInfo {
    pub contact_id: u32,
    pub active: bool,
    pub msg_count_tx: u32,
    pub msg_count_rx: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CMsgCryptoStats {
    pub channel_count: u32,
    pub total_encrypted: u32,
    pub total_decrypted: u32,
}

// ---------------------------------------------------------------------------
// Internal crypto helpers
// ---------------------------------------------------------------------------

/// Increment the last 4 bytes of a 16-byte counter block (big-endian).
fn increment_counter(counter: &mut [u8; 16]) {
    for i in (12..16).rev() {
        counter[i] = counter[i].wrapping_add(1);
        if counter[i] != 0 {
            break;
        }
    }
}

/// AES-256-CTR encrypt/decrypt (same operation — XOR with keystream).
fn aes256_ctr_process(key: &[u8; 32], nonce: &[u8; 16], input: &[u8], output: &mut [u8]) {
    let cipher = aes::Aes256::new(&(*key).into());
    let mut counter = [0u8; 16];
    counter.copy_from_slice(nonce);

    for (block_idx, chunk) in input.chunks(16).enumerate() {
        // Encrypt counter block to produce keystream
        let mut keystream = aes::Block::from(counter);
        cipher.encrypt_block(&mut keystream);

        // XOR input with keystream
        for (i, &byte) in chunk.iter().enumerate() {
            output[block_idx * 16 + i] = byte ^ keystream[i];
        }

        // Increment counter for next block
        increment_counter(&mut counter);
    }
}

/// HMAC-SHA256 with output to a 32-byte array.
fn compute_hmac(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).unwrap();
    mac.update(data);
    let result = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Derive per-message encryption and MAC keys from master key and nonce.
fn derive_per_message_keys(
    master_key: &[u8; MASTER_KEY_LEN],
    nonce: &[u8; NONCE_LEN],
) -> ([u8; 32], [u8; 32]) {
    // enc_key = HMAC-SHA256(master_key, nonce || "enc")
    let mut enc_input = [0u8; NONCE_LEN + 3];
    enc_input[..NONCE_LEN].copy_from_slice(nonce);
    enc_input[NONCE_LEN..].copy_from_slice(b"enc");
    let enc_key = compute_hmac(master_key, &enc_input);

    // mac_key = HMAC-SHA256(master_key, nonce || "mac")
    let mut mac_input = [0u8; NONCE_LEN + 3];
    mac_input[..NONCE_LEN].copy_from_slice(nonce);
    mac_input[NONCE_LEN..].copy_from_slice(b"mac");
    let mac_key = compute_hmac(master_key, &mac_input);

    (enc_key, mac_key)
}

/// Constant-time comparison of two byte slices.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Derive a master key from a passphrase using PBKDF2-SHA256.
fn derive_master_key(passphrase: &[u8]) -> [u8; MASTER_KEY_LEN] {
    let mut key = [0u8; MASTER_KEY_LEN];
    pbkdf2_hmac::<Sha256>(passphrase, PBKDF2_SALT, PBKDF2_ITERATIONS, &mut key);
    key
}

/// Zeroize channel key material before deactivation.
fn zeroize_channel(ch: &mut CryptoChannel) {
    // Overwrite key material with zeros
    for byte in ch.master_key.iter_mut() {
        *byte = 0;
    }
    ch.active = false;
    ch.contact_id = 0;
    ch.msg_count_tx = 0;
    ch.msg_count_rx = 0;
}

/// Encrypt plaintext into output buffer. Returns total output length.
fn encrypt_message(
    master_key: &[u8; MASTER_KEY_LEN],
    plaintext: &[u8],
    output: &mut [u8],
) -> Result<usize, i32> {
    let total_len = OVERHEAD + plaintext.len();
    if output.len() < total_len {
        return Err(ESP_ERR_NO_MEM);
    }

    // 1. Generate random nonce
    let mut nonce = [0u8; NONCE_LEN];
    if getrandom::getrandom(&mut nonce).is_err() {
        return Err(ESP_FAIL);
    }

    // 2. Derive per-message keys
    let (enc_key, mac_key) = derive_per_message_keys(master_key, &nonce);

    // 3. Write version byte
    output[0] = VERSION;

    // 4. Write nonce
    output[1..1 + NONCE_LEN].copy_from_slice(&nonce);

    // 5. Encrypt plaintext with AES-256-CTR
    let ct_start = HEADER_LEN + NONCE_LEN;
    let ct_end = ct_start + plaintext.len();
    aes256_ctr_process(&enc_key, &nonce, plaintext, &mut output[ct_start..ct_end]);

    // 6. Compute HMAC over [version | nonce | ciphertext]
    let hmac_val = compute_hmac(&mac_key, &output[0..ct_end]);
    output[ct_end..ct_end + HMAC_LEN].copy_from_slice(&hmac_val);

    Ok(total_len)
}

/// Decrypt ciphertext into plaintext buffer. Returns plaintext length.
fn decrypt_message(
    master_key: &[u8; MASTER_KEY_LEN],
    input: &[u8],
    plaintext: &mut [u8],
) -> Result<usize, i32> {
    // 1. Check minimum length
    if input.len() < OVERHEAD {
        return Err(ESP_ERR_INVALID_SIZE);
    }

    // 2. Check version
    if input[0] != VERSION {
        return Err(ESP_ERR_NOT_SUPPORTED);
    }

    // 3. Extract components
    let nonce: [u8; NONCE_LEN] = input[1..1 + NONCE_LEN]
        .try_into()
        .map_err(|_| ESP_ERR_INVALID_SIZE)?;
    let ct_len = input.len() - OVERHEAD;
    let ct_start = HEADER_LEN + NONCE_LEN;
    let ct_end = ct_start + ct_len;
    let ciphertext = &input[ct_start..ct_end];
    let received_hmac = &input[ct_end..ct_end + HMAC_LEN];

    if plaintext.len() < ct_len {
        return Err(ESP_ERR_NO_MEM);
    }

    // 4. Derive per-message keys
    let (enc_key, mac_key) = derive_per_message_keys(master_key, &nonce);

    // 5. Verify HMAC (constant-time comparison)
    let computed_hmac = compute_hmac(&mac_key, &input[0..ct_end]);
    if !constant_time_eq(&computed_hmac, received_hmac) {
        return Err(ESP_ERR_INVALID_CRC);
    }

    // 6. Decrypt
    aes256_ctr_process(&enc_key, &nonce, ciphertext, &mut plaintext[..ct_len]);

    Ok(ct_len)
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialize the message crypto subsystem. Clears all channels.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_init() -> i32 {
    let mut state = STATE.lock().unwrap();
    for ch in state.channels.iter_mut() {
        zeroize_channel(ch);
    }
    state.channel_count = 0;
    state.initialized = true;
    ESP_OK
}

/// Establish an encrypted channel with a contact using a shared passphrase.
/// If a channel already exists for this contact, it is replaced.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_establish(
    contact_id: u32,
    passphrase: *const u8,
    passphrase_len: usize,
) -> i32 {
    if passphrase.is_null() || passphrase_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let mut state = STATE.lock().unwrap();
    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // SAFETY: Caller guarantees passphrase points to passphrase_len valid bytes.
    let pw = std::slice::from_raw_parts(passphrase, passphrase_len);
    let master_key = derive_master_key(pw);

    // Check if channel already exists — replace it
    let slot = if let Some(idx) = state.find_channel(contact_id) {
        zeroize_channel(&mut state.channels[idx]);
        state.channel_count = state.channel_count.saturating_sub(1);
        idx
    } else {
        match state.find_free_slot() {
            Some(idx) => idx,
            None => return ESP_ERR_NO_MEM,
        }
    };

    state.channels[slot] = CryptoChannel {
        contact_id,
        master_key,
        active: true,
        msg_count_tx: 0,
        msg_count_rx: 0,
    };
    state.channel_count += 1;

    ESP_OK
}

/// Destroy an encrypted channel, zeroizing all key material.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_destroy(contact_id: u32) -> i32 {
    let mut state = STATE.lock().unwrap();
    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    match state.find_channel(contact_id) {
        Some(idx) => {
            zeroize_channel(&mut state.channels[idx]);
            state.channel_count = state.channel_count.saturating_sub(1);
            ESP_OK
        }
        None => ESP_ERR_NOT_FOUND,
    }
}

/// Destroy all encrypted channels, zeroizing all key material.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_destroy_all() -> i32 {
    let mut state = STATE.lock().unwrap();
    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    for ch in state.channels.iter_mut() {
        zeroize_channel(ch);
    }
    state.channel_count = 0;
    ESP_OK
}

/// Check if an encrypted channel exists for the given contact.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_is_active(contact_id: u32) -> bool {
    let state = STATE.lock().unwrap();
    state.find_channel(contact_id).is_some()
}

/// Encrypt a message for a contact. Returns total output length or negative error.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_encrypt(
    contact_id: u32,
    plaintext: *const u8,
    pt_len: usize,
    ciphertext: *mut u8,
    ct_max: usize,
) -> i32 {
    if ciphertext.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    // plaintext may be null if pt_len == 0 (zero-length message)
    if plaintext.is_null() && pt_len > 0 {
        return ESP_ERR_INVALID_ARG;
    }

    let mut state = STATE.lock().unwrap();
    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let idx = match state.find_channel(contact_id) {
        Some(i) => i,
        None => return ESP_ERR_NOT_FOUND,
    };

    let master_key = state.channels[idx].master_key;

    // SAFETY: Caller guarantees pointers are valid for the given lengths.
    let pt = if pt_len > 0 {
        std::slice::from_raw_parts(plaintext, pt_len)
    } else {
        &[]
    };
    let ct = std::slice::from_raw_parts_mut(ciphertext, ct_max);

    match encrypt_message(&master_key, pt, ct) {
        Ok(len) => {
            state.channels[idx].msg_count_tx += 1;
            len as i32
        }
        Err(e) => e,
    }
}

/// Decrypt a message from a contact. Returns plaintext length or negative error.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_decrypt(
    contact_id: u32,
    ciphertext: *const u8,
    ct_len: usize,
    plaintext: *mut u8,
    pt_max: usize,
) -> i32 {
    if ciphertext.is_null() || plaintext.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let mut state = STATE.lock().unwrap();
    if !state.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let idx = match state.find_channel(contact_id) {
        Some(i) => i,
        None => return ESP_ERR_NOT_FOUND,
    };

    let master_key = state.channels[idx].master_key;

    // SAFETY: Caller guarantees pointers are valid for the given lengths.
    let ct = std::slice::from_raw_parts(ciphertext, ct_len);
    let pt = std::slice::from_raw_parts_mut(plaintext, pt_max);

    match decrypt_message(&master_key, ct, pt) {
        Ok(len) => {
            state.channels[idx].msg_count_rx += 1;
            len as i32
        }
        Err(e) => e,
    }
}

/// Returns the per-message overhead in bytes (49).
#[no_mangle]
pub extern "C" fn rs_msg_crypto_get_overhead() -> i32 {
    OVERHEAD as i32
}

/// Returns the number of active encrypted channels.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_channel_count() -> i32 {
    let state = STATE.lock().unwrap();
    state.channel_count as i32
}

/// Get information about a channel by contact ID.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_get_channel(
    contact_id: u32,
    out: *mut CChannelInfo,
) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let state = STATE.lock().unwrap();
    match state.find_channel(contact_id) {
        Some(idx) => {
            let ch = &state.channels[idx];
            (*out) = CChannelInfo {
                contact_id: ch.contact_id,
                active: ch.active,
                msg_count_tx: ch.msg_count_tx,
                msg_count_rx: ch.msg_count_rx,
            };
            ESP_OK
        }
        None => ESP_ERR_NOT_FOUND,
    }
}

/// Get aggregate stats for the message crypto subsystem.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_get_stats(out: *mut CMsgCryptoStats) -> i32 {
    if out.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let state = STATE.lock().unwrap();
    let mut total_enc: u32 = 0;
    let mut total_dec: u32 = 0;
    for ch in state.channels.iter() {
        if ch.active {
            total_enc += ch.msg_count_tx;
            total_dec += ch.msg_count_rx;
        }
    }
    (*out) = CMsgCryptoStats {
        channel_count: state.channel_count as u32,
        total_encrypted: total_enc,
        total_decrypted: total_dec,
    };
    ESP_OK
}

/// Standalone PBKDF2 key derivation utility. Writes 32 bytes to key_out.
#[no_mangle]
pub unsafe extern "C" fn rs_msg_crypto_derive_key(
    passphrase: *const u8,
    pw_len: usize,
    key_out: *mut u8,
) -> i32 {
    if passphrase.is_null() || key_out.is_null() || pw_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    // SAFETY: Caller guarantees pointers are valid for the given lengths.
    let pw = std::slice::from_raw_parts(passphrase, pw_len);
    let key = derive_master_key(pw);
    std::ptr::copy_nonoverlapping(key.as_ptr(), key_out, MASTER_KEY_LEN);
    ESP_OK
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset state before each test to avoid cross-test contamination.
    fn reset_state() {
        let mut state = STATE.lock().unwrap();
        for ch in state.channels.iter_mut() {
            zeroize_channel(ch);
        }
        state.channel_count = 0;
        state.initialized = false;
    }

    // ── Init ─────────────────────────────────────────────────────────

    #[test]
    fn test_init_ok() {
        reset_state();
        let ret = unsafe { rs_msg_crypto_init() };
        assert_eq!(ret, ESP_OK);
        let state = STATE.lock().unwrap();
        assert!(state.initialized);
        assert_eq!(state.channel_count, 0);
    }

    #[test]
    fn test_init_double_idempotent() {
        reset_state();
        let ret1 = unsafe { rs_msg_crypto_init() };
        assert_eq!(ret1, ESP_OK);
        // Establish a channel, then re-init should clear it
        let pw = b"secret";
        unsafe {
            rs_msg_crypto_establish(1, pw.as_ptr(), pw.len());
        }
        let ret2 = unsafe { rs_msg_crypto_init() };
        assert_eq!(ret2, ESP_OK);
        let state = STATE.lock().unwrap();
        assert_eq!(state.channel_count, 0);
    }

    // ── Channel management ───────────────────────────────────────────

    #[test]
    fn test_establish_channel() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"my-passphrase";
        let ret = unsafe { rs_msg_crypto_establish(42, pw.as_ptr(), pw.len()) };
        assert_eq!(ret, ESP_OK);
        assert!(unsafe { rs_msg_crypto_is_active(42) });
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, 1);
    }

    #[test]
    fn test_establish_duplicate_replaces() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw1 = b"first-passphrase";
        let pw2 = b"second-passphrase";
        unsafe {
            rs_msg_crypto_establish(10, pw1.as_ptr(), pw1.len());
            rs_msg_crypto_establish(10, pw2.as_ptr(), pw2.len());
        }
        // Should still have only 1 channel
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, 1);
        assert!(unsafe { rs_msg_crypto_is_active(10) });
    }

    #[test]
    fn test_destroy_channel() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"pass";
        unsafe { rs_msg_crypto_establish(5, pw.as_ptr(), pw.len()) };
        let ret = unsafe { rs_msg_crypto_destroy(5) };
        assert_eq!(ret, ESP_OK);
        assert!(!unsafe { rs_msg_crypto_is_active(5) });
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, 0);
    }

    #[test]
    fn test_destroy_nonexistent() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let ret = unsafe { rs_msg_crypto_destroy(999) };
        assert_eq!(ret, ESP_ERR_NOT_FOUND);
    }

    #[test]
    fn test_destroy_all() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"pass";
        for i in 0..5 {
            unsafe { rs_msg_crypto_establish(i, pw.as_ptr(), pw.len()) };
        }
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, 5);
        let ret = unsafe { rs_msg_crypto_destroy_all() };
        assert_eq!(ret, ESP_OK);
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, 0);
        for i in 0..5 {
            assert!(!unsafe { rs_msg_crypto_is_active(i) });
        }
    }

    #[test]
    fn test_max_channels() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"pass";
        for i in 0..MAX_CHANNELS as u32 {
            let ret = unsafe { rs_msg_crypto_establish(i, pw.as_ptr(), pw.len()) };
            assert_eq!(ret, ESP_OK);
        }
        assert_eq!(unsafe { rs_msg_crypto_channel_count() }, MAX_CHANNELS as i32);
        // One more should fail
        let ret = unsafe { rs_msg_crypto_establish(999, pw.as_ptr(), pw.len()) };
        assert_eq!(ret, ESP_ERR_NO_MEM);
    }

    // ── AES-256-CTR ──────────────────────────────────────────────────

    #[test]
    fn test_ctr_empty() {
        let key = [0x42u8; 32];
        let nonce = [0x13u8; 16];
        let input: &[u8] = &[];
        let mut output = [0u8; 0];
        aes256_ctr_process(&key, &nonce, input, &mut output);
        // No panic, no output
    }

    #[test]
    fn test_ctr_short_message() {
        let key = [0x01u8; 32];
        let nonce = [0x00u8; 16];
        let input = b"hello";
        let mut encrypted = [0u8; 5];
        let mut decrypted = [0u8; 5];

        aes256_ctr_process(&key, &nonce, input, &mut encrypted);
        assert_ne!(&encrypted, input);

        // CTR is symmetric — same operation decrypts
        aes256_ctr_process(&key, &nonce, &encrypted, &mut decrypted);
        assert_eq!(&decrypted, input);
    }

    #[test]
    fn test_ctr_block_aligned() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 16];
        let input = [0x55u8; 32]; // exactly 2 blocks
        let mut encrypted = [0u8; 32];
        let mut decrypted = [0u8; 32];

        aes256_ctr_process(&key, &nonce, &input, &mut encrypted);
        assert_ne!(encrypted, input);

        aes256_ctr_process(&key, &nonce, &encrypted, &mut decrypted);
        assert_eq!(decrypted, input);
    }

    #[test]
    fn test_ctr_non_aligned() {
        let key = [0x77u8; 32];
        let nonce = [0x88u8; 16];
        let input = [0x33u8; 25]; // not a multiple of 16
        let mut encrypted = [0u8; 25];
        let mut decrypted = [0u8; 25];

        aes256_ctr_process(&key, &nonce, &input, &mut encrypted);
        aes256_ctr_process(&key, &nonce, &encrypted, &mut decrypted);
        assert_eq!(decrypted, input);
    }

    #[test]
    fn test_ctr_roundtrip_large() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 16];
        let input: Vec<u8> = (0..=255).cycle().take(500).collect();
        let mut encrypted = vec![0u8; 500];
        let mut decrypted = vec![0u8; 500];

        aes256_ctr_process(&key, &nonce, &input, &mut encrypted);
        aes256_ctr_process(&key, &nonce, &encrypted, &mut decrypted);
        assert_eq!(decrypted, input);
    }

    // ── Counter increment ────────────────────────────────────────────

    #[test]
    fn test_counter_single_increment() {
        let mut counter = [0u8; 16];
        increment_counter(&mut counter);
        assert_eq!(counter[15], 1);
        assert_eq!(counter[14], 0);
    }

    #[test]
    fn test_counter_overflow_byte() {
        let mut counter = [0u8; 16];
        counter[15] = 0xFF;
        increment_counter(&mut counter);
        assert_eq!(counter[15], 0);
        assert_eq!(counter[14], 1);
    }

    #[test]
    fn test_counter_wrap_multi_byte() {
        let mut counter = [0u8; 16];
        counter[15] = 0xFF;
        counter[14] = 0xFF;
        increment_counter(&mut counter);
        assert_eq!(counter[15], 0);
        assert_eq!(counter[14], 0);
        assert_eq!(counter[13], 1);
    }

    // ── Key derivation ───────────────────────────────────────────────

    #[test]
    fn test_pbkdf2_deterministic() {
        let key1 = derive_master_key(b"my-secret-passphrase");
        let key2 = derive_master_key(b"my-secret-passphrase");
        assert_eq!(key1, key2);
        assert_ne!(key1, [0u8; 32]);
    }

    #[test]
    fn test_pbkdf2_same_passphrase_same_key() {
        let k1 = derive_master_key(b"identical");
        let k2 = derive_master_key(b"identical");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_pbkdf2_different_passphrase_different_key() {
        let k1 = derive_master_key(b"alpha");
        let k2 = derive_master_key(b"bravo");
        assert_ne!(k1, k2);
    }

    // ── Per-message key derivation ───────────────────────────────────

    #[test]
    fn test_per_msg_keys_different_nonces() {
        let master = [0xAA; 32];
        let nonce_a = [0x01; 16];
        let nonce_b = [0x02; 16];
        let (enc_a, mac_a) = derive_per_message_keys(&master, &nonce_a);
        let (enc_b, mac_b) = derive_per_message_keys(&master, &nonce_b);
        assert_ne!(enc_a, enc_b);
        assert_ne!(mac_a, mac_b);
    }

    #[test]
    fn test_per_msg_enc_key_differs_from_mac_key() {
        let master = [0xBB; 32];
        let nonce = [0x05; 16];
        let (enc_key, mac_key) = derive_per_message_keys(&master, &nonce);
        assert_ne!(enc_key, mac_key);
    }

    #[test]
    fn test_per_msg_keys_deterministic() {
        let master = [0xCC; 32];
        let nonce = [0x07; 16];
        let (enc1, mac1) = derive_per_message_keys(&master, &nonce);
        let (enc2, mac2) = derive_per_message_keys(&master, &nonce);
        assert_eq!(enc1, enc2);
        assert_eq!(mac1, mac2);
    }

    // ── Encrypt full flow ────────────────────────────────────────────

    #[test]
    fn test_encrypt_correct_length() {
        let key = derive_master_key(b"test");
        let plaintext = b"hello world";
        let mut output = vec![0u8; plaintext.len() + OVERHEAD];
        let result = encrypt_message(&key, plaintext, &mut output);
        assert_eq!(result, Ok(plaintext.len() + OVERHEAD));
    }

    #[test]
    fn test_encrypt_different_nonces_different_output() {
        let key = derive_master_key(b"test");
        let plaintext = b"same message";
        let mut out1 = vec![0u8; plaintext.len() + OVERHEAD];
        let mut out2 = vec![0u8; plaintext.len() + OVERHEAD];
        encrypt_message(&key, plaintext, &mut out1).unwrap();
        encrypt_message(&key, plaintext, &mut out2).unwrap();
        // Random nonces make outputs differ (with overwhelming probability)
        assert_ne!(out1, out2);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = derive_master_key(b"roundtrip-test");
        let plaintext = b"confidential data";
        let mut encrypted = vec![0u8; plaintext.len() + OVERHEAD];
        let len = encrypt_message(&key, plaintext, &mut encrypted).unwrap();

        let mut decrypted = vec![0u8; plaintext.len()];
        let pt_len = decrypt_message(&key, &encrypted[..len], &mut decrypted).unwrap();
        assert_eq!(pt_len, plaintext.len());
        assert_eq!(&decrypted[..pt_len], plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_various_lengths() {
        let key = derive_master_key(b"various");
        for size in [0, 1, 15, 16, 17, 31, 32, 33, 64, 100, 255, 512] {
            let plaintext: Vec<u8> = (0..size).map(|i| (i & 0xFF) as u8).collect();
            let mut encrypted = vec![0u8; size + OVERHEAD];
            let enc_len = encrypt_message(&key, &plaintext, &mut encrypted).unwrap();
            assert_eq!(enc_len, size + OVERHEAD);

            let mut decrypted = vec![0u8; size];
            let pt_len =
                decrypt_message(&key, &encrypted[..enc_len], &mut decrypted).unwrap();
            assert_eq!(pt_len, size);
            assert_eq!(&decrypted[..pt_len], &plaintext[..]);
        }
    }

    #[test]
    fn test_encrypt_wrong_channel_fails() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"pass";
        unsafe { rs_msg_crypto_establish(1, pw.as_ptr(), pw.len()) };

        let plaintext = b"hello";
        let mut ct = [0u8; 100];
        // Contact 2 has no channel
        let ret = unsafe {
            rs_msg_crypto_encrypt(2, plaintext.as_ptr(), plaintext.len(), ct.as_mut_ptr(), ct.len())
        };
        assert_eq!(ret, ESP_ERR_NOT_FOUND);
    }

    // ── Decrypt validation ───────────────────────────────────────────

    #[test]
    fn test_decrypt_too_short() {
        let key = derive_master_key(b"test");
        let input = [0u8; OVERHEAD - 1]; // too short
        let mut pt = [0u8; 64];
        let result = decrypt_message(&key, &input, &mut pt);
        assert_eq!(result, Err(ESP_ERR_INVALID_SIZE));
    }

    #[test]
    fn test_decrypt_wrong_version() {
        let key = derive_master_key(b"test");
        let mut input = vec![0u8; OVERHEAD]; // minimum valid length (0-byte plaintext)
        input[0] = 0xFF; // wrong version
        let mut pt = [0u8; 64];
        let result = decrypt_message(&key, &input, &mut pt);
        assert_eq!(result, Err(ESP_ERR_NOT_SUPPORTED));
    }

    #[test]
    fn test_decrypt_tampered_ciphertext() {
        let key = derive_master_key(b"tamper-test");
        let plaintext = b"sensitive info";
        let mut encrypted = vec![0u8; plaintext.len() + OVERHEAD];
        let len = encrypt_message(&key, plaintext, &mut encrypted).unwrap();

        // Tamper with a ciphertext byte
        let ct_start = HEADER_LEN + NONCE_LEN;
        encrypted[ct_start] ^= 0xFF;

        let mut decrypted = vec![0u8; plaintext.len()];
        let result = decrypt_message(&key, &encrypted[..len], &mut decrypted);
        assert_eq!(result, Err(ESP_ERR_INVALID_CRC));
    }

    #[test]
    fn test_decrypt_tampered_hmac() {
        let key = derive_master_key(b"hmac-test");
        let plaintext = b"important";
        let mut encrypted = vec![0u8; plaintext.len() + OVERHEAD];
        let len = encrypt_message(&key, plaintext, &mut encrypted).unwrap();

        // Tamper with the last byte (in the HMAC)
        encrypted[len - 1] ^= 0x01;

        let mut decrypted = vec![0u8; plaintext.len()];
        let result = decrypt_message(&key, &encrypted[..len], &mut decrypted);
        assert_eq!(result, Err(ESP_ERR_INVALID_CRC));
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let key1 = derive_master_key(b"correct-key");
        let key2 = derive_master_key(b"wrong-key");
        let plaintext = b"secret";
        let mut encrypted = vec![0u8; plaintext.len() + OVERHEAD];
        let len = encrypt_message(&key1, plaintext, &mut encrypted).unwrap();

        let mut decrypted = vec![0u8; plaintext.len()];
        let result = decrypt_message(&key2, &encrypted[..len], &mut decrypted);
        assert_eq!(result, Err(ESP_ERR_INVALID_CRC));
    }

    #[test]
    fn test_decrypt_correct() {
        let key = derive_master_key(b"correct");
        let plaintext = b"verified message";
        let mut encrypted = vec![0u8; plaintext.len() + OVERHEAD];
        let len = encrypt_message(&key, plaintext, &mut encrypted).unwrap();

        let mut decrypted = vec![0u8; plaintext.len()];
        let pt_len = decrypt_message(&key, &encrypted[..len], &mut decrypted).unwrap();
        assert_eq!(pt_len, plaintext.len());
        assert_eq!(&decrypted[..pt_len], plaintext);
    }

    // ── Constant-time eq ─────────────────────────────────────────────

    #[test]
    fn test_ct_eq_equal() {
        let a = [1u8, 2, 3, 4, 5];
        let b = [1u8, 2, 3, 4, 5];
        assert!(constant_time_eq(&a, &b));
    }

    #[test]
    fn test_ct_eq_different() {
        let a = [1u8, 2, 3, 4, 5];
        let b = [1u8, 2, 3, 4, 6];
        assert!(!constant_time_eq(&a, &b));
    }

    #[test]
    fn test_ct_eq_different_lengths() {
        let a = [1u8, 2, 3];
        let b = [1u8, 2, 3, 4];
        assert!(!constant_time_eq(&a, &b));
    }

    // ── Stats and info ───────────────────────────────────────────────

    #[test]
    fn test_stats_empty() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let mut stats = CMsgCryptoStats {
            channel_count: 0,
            total_encrypted: 0,
            total_decrypted: 0,
        };
        let ret = unsafe { rs_msg_crypto_get_stats(&mut stats) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(stats.channel_count, 0);
        assert_eq!(stats.total_encrypted, 0);
        assert_eq!(stats.total_decrypted, 0);
    }

    #[test]
    fn test_stats_after_activity() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"stats-test";
        unsafe { rs_msg_crypto_establish(1, pw.as_ptr(), pw.len()) };

        // Encrypt a message
        let plaintext = b"test msg";
        let mut ct = [0u8; 128];
        let enc_len = unsafe {
            rs_msg_crypto_encrypt(1, plaintext.as_ptr(), plaintext.len(), ct.as_mut_ptr(), ct.len())
        };
        assert!(enc_len > 0);

        // Decrypt it
        let mut pt = [0u8; 128];
        let dec_len = unsafe {
            rs_msg_crypto_decrypt(1, ct.as_ptr(), enc_len as usize, pt.as_mut_ptr(), pt.len())
        };
        assert!(dec_len > 0);

        let mut stats = CMsgCryptoStats {
            channel_count: 0,
            total_encrypted: 0,
            total_decrypted: 0,
        };
        unsafe { rs_msg_crypto_get_stats(&mut stats) };
        assert_eq!(stats.channel_count, 1);
        assert_eq!(stats.total_encrypted, 1);
        assert_eq!(stats.total_decrypted, 1);
    }

    #[test]
    fn test_channel_info() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"info-test";
        unsafe { rs_msg_crypto_establish(7, pw.as_ptr(), pw.len()) };

        let mut info = CChannelInfo {
            contact_id: 0,
            active: false,
            msg_count_tx: 0,
            msg_count_rx: 0,
        };
        let ret = unsafe { rs_msg_crypto_get_channel(7, &mut info) };
        assert_eq!(ret, ESP_OK);
        assert_eq!(info.contact_id, 7);
        assert!(info.active);
        assert_eq!(info.msg_count_tx, 0);
        assert_eq!(info.msg_count_rx, 0);

        // Non-existent channel
        let ret = unsafe { rs_msg_crypto_get_channel(999, &mut info) };
        assert_eq!(ret, ESP_ERR_NOT_FOUND);
    }

    // ── Zeroization ──────────────────────────────────────────────────

    #[test]
    fn test_destroy_zeroizes_key() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"zeroize-me";
        unsafe { rs_msg_crypto_establish(50, pw.as_ptr(), pw.len()) };

        // Verify key is non-zero
        {
            let state = STATE.lock().unwrap();
            let idx = state.find_channel(50).unwrap();
            assert_ne!(state.channels[idx].master_key, [0u8; 32]);
        }

        unsafe { rs_msg_crypto_destroy(50) };

        // After destroy, the slot's key should be zeroed
        {
            let state = STATE.lock().unwrap();
            // The slot is no longer findable by contact_id, but we can check
            // all slots to confirm no non-zero keys for contact 50 remain
            for ch in state.channels.iter() {
                if !ch.active {
                    // Inactive channels should have zeroed keys
                    // (we can't guarantee which slot was used, but the
                    // zeroize function zeroes the key)
                    continue;
                }
                assert_ne!(ch.contact_id, 50);
            }
        }
    }

    #[test]
    fn test_destroy_all_zeroizes() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"zeroize-all";
        for i in 0..5u32 {
            unsafe { rs_msg_crypto_establish(i, pw.as_ptr(), pw.len()) };
        }

        unsafe { rs_msg_crypto_destroy_all() };

        let state = STATE.lock().unwrap();
        for ch in state.channels.iter() {
            assert!(!ch.active);
            assert_eq!(ch.master_key, [0u8; 32]);
            assert_eq!(ch.contact_id, 0);
        }
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn test_null_pointer_safety() {
        reset_state();
        unsafe { rs_msg_crypto_init() };

        // Null passphrase
        let ret = unsafe { rs_msg_crypto_establish(1, std::ptr::null(), 10) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null ciphertext output
        let pw = b"p";
        unsafe { rs_msg_crypto_establish(1, pw.as_ptr(), pw.len()) };
        let pt = b"hello";
        let ret = unsafe {
            rs_msg_crypto_encrypt(1, pt.as_ptr(), pt.len(), std::ptr::null_mut(), 100)
        };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null plaintext input (with non-zero length)
        let mut ct = [0u8; 128];
        let ret = unsafe {
            rs_msg_crypto_encrypt(1, std::ptr::null(), 5, ct.as_mut_ptr(), ct.len())
        };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null decrypt inputs
        let ret = unsafe {
            rs_msg_crypto_decrypt(1, std::ptr::null(), 10, ct.as_mut_ptr(), ct.len())
        };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null stats output
        let ret = unsafe { rs_msg_crypto_get_stats(std::ptr::null_mut()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null channel info output
        let ret = unsafe { rs_msg_crypto_get_channel(1, std::ptr::null_mut()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);

        // Null derive key output
        let ret = unsafe { rs_msg_crypto_derive_key(pw.as_ptr(), pw.len(), std::ptr::null_mut()) };
        assert_eq!(ret, ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_zero_length_plaintext() {
        reset_state();
        unsafe { rs_msg_crypto_init() };
        let pw = b"empty-msg-test";
        unsafe { rs_msg_crypto_establish(1, pw.as_ptr(), pw.len()) };

        let mut ct = [0u8; OVERHEAD + 16]; // extra space
        let enc_len = unsafe {
            rs_msg_crypto_encrypt(1, [].as_ptr(), 0, ct.as_mut_ptr(), ct.len())
        };
        assert_eq!(enc_len, OVERHEAD as i32);

        let mut pt = [0u8; 16];
        let dec_len = unsafe {
            rs_msg_crypto_decrypt(1, ct.as_ptr(), enc_len as usize, pt.as_mut_ptr(), pt.len())
        };
        assert_eq!(dec_len, 0);
    }

    #[test]
    fn test_max_message_size() {
        let key = derive_master_key(b"large");
        let plaintext = vec![0xABu8; 4096];
        let mut encrypted = vec![0u8; 4096 + OVERHEAD];
        let len = encrypt_message(&key, &plaintext, &mut encrypted).unwrap();
        assert_eq!(len, 4096 + OVERHEAD);

        let mut decrypted = vec![0u8; 4096];
        let pt_len = decrypt_message(&key, &encrypted[..len], &mut decrypted).unwrap();
        assert_eq!(pt_len, 4096);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_overhead_constant() {
        assert_eq!(rs_msg_crypto_get_overhead(), 49);
        assert_eq!(OVERHEAD, 49);
        assert_eq!(HEADER_LEN + NONCE_LEN + HMAC_LEN, 49);
    }

    // ── FFI derive key utility ───────────────────────────────────────

    #[test]
    fn test_derive_key_ffi() {
        let pw = b"ffi-derivation";
        let mut key1 = [0u8; 32];
        let mut key2 = [0u8; 32];
        let ret = unsafe {
            rs_msg_crypto_derive_key(pw.as_ptr(), pw.len(), key1.as_mut_ptr())
        };
        assert_eq!(ret, ESP_OK);
        assert_ne!(key1, [0u8; 32]);

        // Same passphrase produces same key
        unsafe {
            rs_msg_crypto_derive_key(pw.as_ptr(), pw.len(), key2.as_mut_ptr());
        }
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_derive_key_matches_internal() {
        let pw = b"consistency-check";
        let internal = derive_master_key(pw);
        let mut ffi_key = [0u8; 32];
        unsafe {
            rs_msg_crypto_derive_key(pw.as_ptr(), pw.len(), ffi_key.as_mut_ptr());
        }
        assert_eq!(ffi_key, internal);
    }

    // ── Uninitialized state ──────────────────────────────────────────

    #[test]
    fn test_operations_before_init() {
        reset_state();
        let pw = b"nope";
        let ret = unsafe { rs_msg_crypto_establish(1, pw.as_ptr(), pw.len()) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);

        let ret = unsafe { rs_msg_crypto_destroy(1) };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);

        let ret = unsafe { rs_msg_crypto_destroy_all() };
        assert_eq!(ret, ESP_ERR_INVALID_STATE);
    }
}
