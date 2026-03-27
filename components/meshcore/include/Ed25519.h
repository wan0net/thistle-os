// SPDX-License-Identifier: BSD-3-Clause
// Ed25519.h — C++ wrapper calling ThistleOS kernel Ed25519 syscalls.
// MeshCore uses this class for signature verification and signing.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

// Kernel crypto syscalls (linked at firmware build time)
extern "C" {
    int thistle_crypto_ed25519_verify(const uint8_t *public_key, const uint8_t *message, size_t msg_len, const uint8_t *signature);
    int thistle_crypto_ed25519_sign(const uint8_t *private_key, const uint8_t *message, size_t msg_len, uint8_t *signature_out);
    int thistle_crypto_ed25519_keygen(uint8_t *private_key_out, uint8_t *public_key_out);
    int thistle_crypto_ed25519_derive_public(const uint8_t *private_key, uint8_t *public_key_out);
}

class Ed25519 {
public:
    static bool verify(const uint8_t signature[64],
                       const uint8_t publicKey[32],
                       const void* message,
                       size_t len)
    {
        return thistle_crypto_ed25519_verify(
            publicKey, (const uint8_t*)message, len, signature) == 0;
    }

    static void sign(uint8_t signature[64],
                     const uint8_t privateKey[64],
                     const uint8_t publicKey[32],
                     const void* message,
                     size_t len)
    {
        (void)publicKey;
        // MeshCore uses 64-byte private keys (first 32 = seed)
        thistle_crypto_ed25519_sign(
            privateKey, (const uint8_t*)message, len, signature);
    }

    static void generatePrivateKey(uint8_t privateKey[64])
    {
        uint8_t pub[32];
        // Generate seed into first 32 bytes, public key into last 32
        thistle_crypto_ed25519_keygen(privateKey, pub);
        memcpy(privateKey + 32, pub, 32);
    }

    static void derivePublicKey(uint8_t publicKey[32],
                                const uint8_t privateKey[64])
    {
        // Seed is first 32 bytes of the 64-byte private key
        thistle_crypto_ed25519_derive_public(privateKey, publicKey);
    }
};
