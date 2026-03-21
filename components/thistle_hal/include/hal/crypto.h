// SPDX-License-Identifier: BSD-3-Clause
// HAL crypto driver interface — hardware-accelerated or software fallback

#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>

/* Crypto driver vtable — implement for hardware acceleration.
 * Any function left NULL falls back to the kernel's software implementation.
 * This allows partial hardware support (e.g., hardware SHA but software AES). */
typedef struct {
    esp_err_t (*sha256)(const uint8_t *data, size_t len, uint8_t *hash_out);

    esp_err_t (*aes256_cbc_encrypt)(const uint8_t *key, const uint8_t *iv,
                                     const uint8_t *plaintext, size_t len,
                                     uint8_t *ciphertext_out);

    esp_err_t (*aes256_cbc_decrypt)(const uint8_t *key, const uint8_t *iv,
                                     const uint8_t *ciphertext, size_t len,
                                     uint8_t *plaintext_out);

    esp_err_t (*hmac_sha256)(const uint8_t *key, size_t key_len,
                              const uint8_t *data, size_t data_len,
                              uint8_t *mac_out);

    esp_err_t (*random)(uint8_t *buf, size_t len);

    const char *name;  /* "ESP32-S3 Hardware", "Software (Rust)", etc. */
} hal_crypto_driver_t;
