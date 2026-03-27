// SPDX-License-Identifier: BSD-3-Clause
// ed_25519.h stub — C functions for MeshCore's Ed25519 operations.
// These are the Rhys Weatherley Arduino Crypto library C API.
// TODO: Wire to real Ed25519 via mbedtls or our kernel's ed25519-dalek.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

// Generate a keypair from a 32-byte seed.
// pub_key: 32 bytes output, prv_key: 64 bytes output, seed: 32 bytes input.
static inline void ed25519_create_keypair(uint8_t* pub_key, uint8_t* prv_key, const uint8_t* seed) {
    // TODO: Real keypair generation
    // For bringup: copy seed into private key, derive a dummy public key
    memcpy(prv_key, seed, 32);
    memset(prv_key + 32, 0, 32);
    // Simple (insecure) pub derivation for mesh connectivity testing
    for (int i = 0; i < 32; i++) {
        pub_key[i] = seed[i] ^ 0xFF;
    }
}

// Derive public key from private key.
static inline void ed25519_derive_pub(uint8_t* pub_key, const uint8_t* prv_key) {
    // TODO: Real derivation
    for (int i = 0; i < 32; i++) {
        pub_key[i] = prv_key[i] ^ 0xFF;
    }
}

// Sign a message.
static inline void ed25519_sign(uint8_t* sig, const uint8_t* msg, size_t msg_len,
                                const uint8_t* pub_key, const uint8_t* prv_key) {
    (void)msg; (void)msg_len; (void)pub_key; (void)prv_key;
    // TODO: Real Ed25519 signing
    memset(sig, 0, 64);
}

// Verify a signature.
static inline int ed25519_verify(const uint8_t* sig, const uint8_t* msg, size_t msg_len,
                                  const uint8_t* pub_key) {
    (void)sig; (void)msg; (void)msg_len; (void)pub_key;
    // TODO: Real Ed25519 verification
    return 1; // Accept all for bringup
}

// Compute shared secret via ECDH key exchange.
static inline void ed25519_key_exchange(uint8_t* shared_secret,
                                         const uint8_t* other_pub_key,
                                         const uint8_t* prv_key) {
    // TODO: Real X25519 key exchange
    // For bringup: XOR the keys together as a dummy shared secret
    for (int i = 0; i < 32; i++) {
        shared_secret[i] = other_pub_key[i] ^ prv_key[i];
    }
}

#ifdef __cplusplus
}
#endif
