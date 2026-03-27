// SPDX-License-Identifier: BSD-3-Clause
// SHA256.h stub — wraps mbedtls SHA-256 for MeshCore on ESP-IDF
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "mbedtls/sha256.h"

class SHA256 {
    mbedtls_sha256_context _ctx;
    uint8_t _hash[32];
public:
    static const size_t HASH_SIZE = 32;

    SHA256() { mbedtls_sha256_init(&_ctx); }
    ~SHA256() { mbedtls_sha256_free(&_ctx); }

    void reset() {
        mbedtls_sha256_free(&_ctx);
        mbedtls_sha256_init(&_ctx);
        mbedtls_sha256_starts(&_ctx, 0); // 0 = SHA-256 (not SHA-224)
    }

    void update(const void* data, size_t len) {
        mbedtls_sha256_update(&_ctx, (const unsigned char*)data, len);
    }

    void finalize(uint8_t* hash, size_t hash_len) {
        mbedtls_sha256_finish(&_ctx, _hash);
        size_t n = (hash_len < 32) ? hash_len : 32;
        memcpy(hash, _hash, n);
    }

    const uint8_t* hash() const { return _hash; }

    // HMAC-SHA256 support (Arduino Crypto API)
    void resetHMAC(const uint8_t* key, size_t key_len) {
        // HMAC = H((K ^ opad) || H((K ^ ipad) || message))
        // For simplicity, use mbedtls md for HMAC
        reset();
        // Store key for finalize — simplified: just reset with key XOR ipad
        uint8_t k_ipad[64];
        memset(k_ipad, 0x36, 64);
        size_t kl = (key_len > 64) ? 64 : key_len;
        for (size_t i = 0; i < kl; i++) k_ipad[i] ^= key[i];
        update(k_ipad, 64);
        // Store key for opad in _opad_key
        memset(_opad_key, 0x5c, 64);
        for (size_t i = 0; i < kl; i++) _opad_key[i] ^= key[i];
    }

    void finalizeHMAC(const uint8_t* key, size_t key_len, uint8_t* hmac_out, size_t hmac_len) {
        (void)key; (void)key_len;
        // Finish inner hash
        uint8_t inner[32];
        mbedtls_sha256_finish(&_ctx, inner);

        // Outer hash: H(K ^ opad || inner_hash)
        mbedtls_sha256_free(&_ctx);
        mbedtls_sha256_init(&_ctx);
        mbedtls_sha256_starts(&_ctx, 0);
        mbedtls_sha256_update(&_ctx, _opad_key, 64);
        mbedtls_sha256_update(&_ctx, inner, 32);
        mbedtls_sha256_finish(&_ctx, _hash);

        size_t n = (hmac_len < 32) ? hmac_len : 32;
        memcpy(hmac_out, _hash, n);
    }

private:
    uint8_t _opad_key[64];
};
