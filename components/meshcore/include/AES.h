// SPDX-License-Identifier: BSD-3-Clause
// AES.h — MeshCore AES-128-ECB via ThistleOS kernel crypto syscalls.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>

extern "C" {
    int thistle_crypto_aes128_ecb_encrypt(const uint8_t *key, const uint8_t *plaintext, size_t len, uint8_t *ciphertext_out);
    int thistle_crypto_aes128_ecb_decrypt(const uint8_t *key, const uint8_t *ciphertext, size_t len, uint8_t *plaintext_out);
}

class AES128 {
    uint8_t _key[16];
    bool _key_set;
public:
    AES128() : _key_set(false) {
        memset(_key, 0, sizeof(_key));
    }

    bool setKey(const uint8_t* key, size_t len) {
        if (len < 16) return false;
        memcpy(_key, key, 16);
        _key_set = true;
        return true;
    }

    void encryptBlock(uint8_t output[16], const uint8_t input[16]) {
        if (!_key_set) return;
        thistle_crypto_aes128_ecb_encrypt(_key, input, 16, output);
    }

    void decryptBlock(uint8_t output[16], const uint8_t input[16]) {
        if (!_key_set) return;
        thistle_crypto_aes128_ecb_decrypt(_key, input, 16, output);
    }
};
