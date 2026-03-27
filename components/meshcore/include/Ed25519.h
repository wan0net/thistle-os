// SPDX-License-Identifier: BSD-3-Clause
// Ed25519.h stub — wraps mbedtls Ed25519 for MeshCore on ESP-IDF.
// MeshCore only uses Ed25519::verify() for signature validation.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

// Stubbed Ed25519 — MeshCore uses this for verify only.
// TODO: Implement real Ed25519 verify via mbedtls or our kernel crypto.
// For now, accept all signatures to allow mesh connectivity.
class Ed25519 {
public:
    static bool verify(const uint8_t signature[64],
                       const uint8_t publicKey[32],
                       const void* message,
                       size_t len)
    {
        (void)signature;
        (void)publicKey;
        (void)message;
        (void)len;
        // TODO: Wire to real Ed25519 verification.
        // Accepting all signatures for initial mesh bringup.
        return true;
    }

    static void sign(uint8_t signature[64],
                     const uint8_t privateKey[64],
                     const uint8_t publicKey[32],
                     const void* message,
                     size_t len)
    {
        (void)privateKey;
        (void)publicKey;
        (void)message;
        (void)len;
        // TODO: Wire to real Ed25519 signing.
        memset(signature, 0, 64);
    }

    static void generatePrivateKey(uint8_t privateKey[64])
    {
        // TODO: Generate real Ed25519 keypair
        memset(privateKey, 0, 64);
    }

    static void derivePublicKey(uint8_t publicKey[32],
                                const uint8_t privateKey[64])
    {
        (void)privateKey;
        // TODO: Derive from private key
        memset(publicKey, 0, 32);
    }
};
