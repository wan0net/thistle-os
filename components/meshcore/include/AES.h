// SPDX-License-Identifier: BSD-3-Clause
// AES.h stub — wraps mbedtls AES-128 for MeshCore on ESP-IDF.
// MeshCore uses AES128 class with setKey/encryptBlock/decryptBlock (ECB mode).
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "mbedtls/aes.h"

class AES128 {
    mbedtls_aes_context _enc_ctx;
    mbedtls_aes_context _dec_ctx;
    bool _key_set;
public:
    AES128() : _key_set(false) {
        mbedtls_aes_init(&_enc_ctx);
        mbedtls_aes_init(&_dec_ctx);
    }

    ~AES128() {
        mbedtls_aes_free(&_enc_ctx);
        mbedtls_aes_free(&_dec_ctx);
    }

    bool setKey(const uint8_t* key, size_t len) {
        if (len < 16) return false;
        mbedtls_aes_setkey_enc(&_enc_ctx, key, 128);
        mbedtls_aes_setkey_dec(&_dec_ctx, key, 128);
        _key_set = true;
        return true;
    }

    void encryptBlock(uint8_t output[16], const uint8_t input[16]) {
        if (!_key_set) return;
        mbedtls_aes_crypt_ecb(&_enc_ctx, MBEDTLS_AES_ENCRYPT, input, output);
    }

    void decryptBlock(uint8_t output[16], const uint8_t input[16]) {
        if (!_key_set) return;
        mbedtls_aes_crypt_ecb(&_dec_ctx, MBEDTLS_AES_DECRYPT, input, output);
    }
};
