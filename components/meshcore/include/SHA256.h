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
};
