// SPDX-License-Identifier: BSD-3-Clause
// ed_25519.h — C API calling ThistleOS kernel Ed25519/X25519 syscalls.
// Rhys Weatherley Arduino Crypto library compatible interface.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

// Kernel crypto syscalls (linked at firmware build time)
int thistle_crypto_ed25519_keygen(uint8_t *private_key_out, uint8_t *public_key_out);
int thistle_crypto_ed25519_derive_public(const uint8_t *private_key, uint8_t *public_key_out);
int thistle_crypto_ed25519_sign(const uint8_t *private_key, const uint8_t *message, size_t msg_len, uint8_t *signature_out);
int thistle_crypto_ed25519_verify(const uint8_t *public_key, const uint8_t *message, size_t msg_len, const uint8_t *signature);
int thistle_crypto_x25519_key_exchange(const uint8_t *ed25519_private_key, const uint8_t *other_ed25519_public_key, uint8_t *shared_secret_out);

// Generate a keypair from a 32-byte seed.
// pub_key: 32 bytes output, prv_key: 64 bytes output (seed + pub), seed: 32 bytes input.
static inline void ed25519_create_keypair(uint8_t* pub_key, uint8_t* prv_key, const uint8_t* seed) {
    // Copy seed into first 32 bytes of private key
    memcpy(prv_key, seed, 32);
    // Derive public key from seed
    thistle_crypto_ed25519_derive_public(seed, pub_key);
    // Store public key in last 32 bytes of private key (Arduino Crypto convention)
    memcpy(prv_key + 32, pub_key, 32);
}

// Derive public key from private key.
static inline void ed25519_derive_pub(uint8_t* pub_key, const uint8_t* prv_key) {
    // Seed is first 32 bytes of the 64-byte private key
    thistle_crypto_ed25519_derive_public(prv_key, pub_key);
}

// Sign a message.
static inline void ed25519_sign(uint8_t* sig, const uint8_t* msg, size_t msg_len,
                                const uint8_t* pub_key, const uint8_t* prv_key) {
    (void)pub_key;
    // Seed is first 32 bytes of the 64-byte private key
    thistle_crypto_ed25519_sign(prv_key, msg, msg_len, sig);
}

// Verify a signature.
static inline int ed25519_verify(const uint8_t* sig, const uint8_t* msg, size_t msg_len,
                                  const uint8_t* pub_key) {
    // Returns 1 if valid (Arduino Crypto convention), 0 if invalid
    return thistle_crypto_ed25519_verify(pub_key, msg, msg_len, sig) == 0 ? 1 : 0;
}

// Compute shared secret via X25519 key exchange (Ed25519 → X25519 conversion).
static inline void ed25519_key_exchange(uint8_t* shared_secret,
                                         const uint8_t* other_pub_key,
                                         const uint8_t* prv_key) {
    // Seed is first 32 bytes of the 64-byte private key
    thistle_crypto_x25519_key_exchange(prv_key, other_pub_key, shared_secret);
}

#ifdef __cplusplus
}
#endif
