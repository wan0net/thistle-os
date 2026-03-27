// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — mbedtls hardware crypto driver
//
// Wraps ESP-IDF's mbedtls (patched by Espressif for hardware AES/SHA
// acceleration on ESP32-S3 and other chips) as a HAL crypto driver.
#include "drv_crypto_mbedtls.h"
#include "hal/crypto.h"
#include "mbedtls/sha256.h"
#include "mbedtls/aes.h"
#include "mbedtls/md.h"
#include "esp_random.h"
#include "esp_err.h"
#include <string.h>

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t hw_sha256(const uint8_t *data, size_t len, uint8_t *hash_out)
{
    if (!data || !hash_out) {
        return ESP_ERR_INVALID_ARG;
    }

    // mbedtls_sha256(): last param 0 = SHA-256 (not SHA-224)
    int ret = mbedtls_sha256(data, len, hash_out, 0);
    return (ret == 0) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_aes256_cbc_encrypt(const uint8_t *key, const uint8_t *iv,
                                        const uint8_t *plaintext, size_t len,
                                        uint8_t *ciphertext_out)
{
    if (!key || !iv || !plaintext || !ciphertext_out) {
        return ESP_ERR_INVALID_ARG;
    }
    if (len == 0 || (len % 16) != 0) {
        return ESP_ERR_INVALID_SIZE;
    }

    // mbedtls_aes_crypt_cbc modifies IV in-place — use a local copy.
    uint8_t iv_copy[16];
    memcpy(iv_copy, iv, 16);

    mbedtls_aes_context ctx;
    mbedtls_aes_init(&ctx);

    int ret = mbedtls_aes_setkey_enc(&ctx, key, 256);
    if (ret != 0) {
        mbedtls_aes_free(&ctx);
        return ESP_FAIL;
    }

    ret = mbedtls_aes_crypt_cbc(&ctx, MBEDTLS_AES_ENCRYPT, len,
                                 iv_copy, plaintext, ciphertext_out);
    mbedtls_aes_free(&ctx);
    return (ret == 0) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_aes256_cbc_decrypt(const uint8_t *key, const uint8_t *iv,
                                        const uint8_t *ciphertext, size_t len,
                                        uint8_t *plaintext_out)
{
    if (!key || !iv || !ciphertext || !plaintext_out) {
        return ESP_ERR_INVALID_ARG;
    }
    if (len == 0 || (len % 16) != 0) {
        return ESP_ERR_INVALID_SIZE;
    }

    uint8_t iv_copy[16];
    memcpy(iv_copy, iv, 16);

    mbedtls_aes_context ctx;
    mbedtls_aes_init(&ctx);

    int ret = mbedtls_aes_setkey_dec(&ctx, key, 256);
    if (ret != 0) {
        mbedtls_aes_free(&ctx);
        return ESP_FAIL;
    }

    ret = mbedtls_aes_crypt_cbc(&ctx, MBEDTLS_AES_DECRYPT, len,
                                 iv_copy, ciphertext, plaintext_out);
    mbedtls_aes_free(&ctx);
    return (ret == 0) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_hmac_sha256(const uint8_t *key, size_t key_len,
                                 const uint8_t *data, size_t data_len,
                                 uint8_t *mac_out)
{
    if (!key || !data || !mac_out) {
        return ESP_ERR_INVALID_ARG;
    }
    if (key_len == 0) {
        return ESP_ERR_INVALID_SIZE;
    }

    const mbedtls_md_info_t *md_info = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
    if (!md_info) {
        return ESP_FAIL;
    }

    int ret = mbedtls_md_hmac(md_info, key, key_len, data, data_len, mac_out);
    return (ret == 0) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_random(uint8_t *buf, size_t len)
{
    if (!buf) {
        return ESP_ERR_INVALID_ARG;
    }
    if (len == 0) {
        return ESP_OK;
    }

    esp_fill_random(buf, len);
    return ESP_OK;
}

static esp_err_t hw_aes128_ecb_encrypt(const uint8_t *key, const uint8_t *plaintext,
                                        size_t len, uint8_t *ciphertext_out)
{
    if (!key || !plaintext || !ciphertext_out) return ESP_ERR_INVALID_ARG;
    if (len == 0 || len % 16 != 0) return ESP_ERR_INVALID_SIZE;

    mbedtls_aes_context ctx;
    mbedtls_aes_init(&ctx);
    if (mbedtls_aes_setkey_enc(&ctx, key, 128) != 0) {
        mbedtls_aes_free(&ctx);
        return ESP_FAIL;
    }

    for (size_t i = 0; i < len; i += 16) {
        mbedtls_aes_crypt_ecb(&ctx, MBEDTLS_AES_ENCRYPT, plaintext + i, ciphertext_out + i);
    }

    mbedtls_aes_free(&ctx);
    return ESP_OK;
}

static esp_err_t hw_aes128_ecb_decrypt(const uint8_t *key, const uint8_t *ciphertext,
                                        size_t len, uint8_t *plaintext_out)
{
    if (!key || !ciphertext || !plaintext_out) return ESP_ERR_INVALID_ARG;
    if (len == 0 || len % 16 != 0) return ESP_ERR_INVALID_SIZE;

    mbedtls_aes_context ctx;
    mbedtls_aes_init(&ctx);
    if (mbedtls_aes_setkey_dec(&ctx, key, 128) != 0) {
        mbedtls_aes_free(&ctx);
        return ESP_FAIL;
    }

    for (size_t i = 0; i < len; i += 16) {
        mbedtls_aes_crypt_ecb(&ctx, MBEDTLS_AES_DECRYPT, ciphertext + i, plaintext_out + i);
    }

    mbedtls_aes_free(&ctx);
    return ESP_OK;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_crypto_driver_t s_vtable = {
    .sha256             = hw_sha256,
    .aes256_cbc_encrypt = hw_aes256_cbc_encrypt,
    .aes256_cbc_decrypt = hw_aes256_cbc_decrypt,
    .hmac_sha256        = hw_hmac_sha256,
    .random             = hw_random,
    .aes128_ecb_encrypt = hw_aes128_ecb_encrypt,
    .aes128_ecb_decrypt = hw_aes128_ecb_decrypt,
    .name               = "mbedtls (HW-accelerated)",
};

const hal_crypto_driver_t *drv_crypto_mbedtls_get(void)
{
    return &s_vtable;
}
