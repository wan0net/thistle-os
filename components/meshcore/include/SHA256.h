// SPDX-License-Identifier: BSD-3-Clause
// SHA256.h — MeshCore SHA-256/HMAC-SHA256 via ThistleOS kernel crypto syscalls.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

extern "C" {
    int thistle_crypto_sha256(const uint8_t *data, size_t len, uint8_t *hash_out);
    int thistle_crypto_hmac_sha256(const uint8_t *key, size_t key_len, const uint8_t *data, size_t data_len, uint8_t *mac_out);
}

class SHA256 {
    // Buffer for accumulating update() data.
    // MeshCore packets are small — 1024 bytes covers all known usage.
    // If overflow occurs, _overflow is set and finalize produces a zero hash.
    uint8_t _buf[1024];
    size_t _buf_len;
    uint8_t _hash[32];
    bool _overflow;

    // HMAC state
    uint8_t _hmac_key[64];
    size_t _hmac_key_len;
    bool _hmac_mode;

public:
    static const size_t HASH_SIZE = 32;

    SHA256() : _buf_len(0), _overflow(false), _hmac_key_len(0), _hmac_mode(false) {
        memset(_hash, 0, sizeof(_hash));
    }

    void reset() {
        _buf_len = 0;
        _overflow = false;
        _hmac_mode = false;
        memset(_hash, 0, sizeof(_hash));
    }

    void update(const void* data, size_t len) {
        if (_overflow) return;
        const uint8_t* src = (const uint8_t*)data;
        if (_buf_len + len > sizeof(_buf)) {
            _overflow = true;
            return;
        }
        memcpy(_buf + _buf_len, src, len);
        _buf_len += len;
    }

    void finalize(void* hash, size_t hash_len) {
        if (_overflow) {
            // Buffer overflow — produce zero hash so verification fails
            memset(hash, 0, hash_len);
            return;
        }
        if (_hmac_mode) {
            thistle_crypto_hmac_sha256(_hmac_key, _hmac_key_len, _buf, _buf_len, _hash);
        } else {
            thistle_crypto_sha256(_buf, _buf_len, _hash);
        }
        size_t n = (hash_len < 32) ? hash_len : 32;
        memcpy(hash, _hash, n);
    }

    const uint8_t* hash() const { return _hash; }

    // HMAC-SHA256 support (Arduino Crypto API)
    void resetHMAC(const uint8_t* key, size_t key_len) {
        reset();
        _hmac_mode = true;
        _hmac_key_len = (key_len > 64) ? 64 : key_len;
        memcpy(_hmac_key, key, _hmac_key_len);
    }

    void finalizeHMAC(const uint8_t* key, size_t key_len, void* hmac_out, size_t hmac_len) {
        (void)key; (void)key_len;
        if (_overflow) {
            memset(hmac_out, 0, hmac_len);
            return;
        }
        // Use stored key from resetHMAC
        thistle_crypto_hmac_sha256(_hmac_key, _hmac_key_len, _buf, _buf_len, _hash);
        size_t n = (hmac_len < 32) ? hmac_len : 32;
        memcpy(hmac_out, _hash, n);
    }
};
