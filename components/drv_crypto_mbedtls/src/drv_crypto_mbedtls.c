// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — mbedtls hardware crypto driver
//
// Wraps ESP-IDF's mbedtls (patched by Espressif for hardware AES/SHA
// acceleration on ESP32-S3 and other chips) as a HAL crypto driver.
// Uses PSA Crypto API (mbedtls 4.x / tf-psa-crypto).
#include "drv_crypto_mbedtls.h"
#include "hal/crypto.h"
#include "psa/crypto.h"
#include "esp_random.h"
#include "esp_err.h"
#include <string.h>

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

static psa_key_id_t import_aes_key(const uint8_t *key, size_t key_bytes,
                                    psa_algorithm_t alg, psa_key_usage_t usage)
{
    psa_key_attributes_t attr = PSA_KEY_ATTRIBUTES_INIT;
    psa_set_key_usage_flags(&attr, usage);
    psa_set_key_algorithm(&attr, alg);
    psa_set_key_type(&attr, PSA_KEY_TYPE_AES);
    psa_set_key_bits(&attr, (psa_key_bits_t)(key_bytes * 8));

    psa_key_id_t key_id = PSA_KEY_ID_NULL;
    psa_import_key(&attr, key, key_bytes, &key_id);
    return key_id;
}

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t hw_sha256(const uint8_t *data, size_t len, uint8_t *hash_out)
{
    if (!data || !hash_out) {
        return ESP_ERR_INVALID_ARG;
    }

    size_t olen;
    psa_status_t st = psa_hash_compute(PSA_ALG_SHA_256,
                                        data, len,
                                        hash_out, 32, &olen);
    return (st == PSA_SUCCESS) ? ESP_OK : ESP_FAIL;
}

static esp_err_t aes_cbc_op(const uint8_t *key, size_t key_bytes,
                              const uint8_t *iv,
                              const uint8_t *input, size_t len,
                              uint8_t *output, bool encrypt)
{
    if (!key || !iv || !input || !output) return ESP_ERR_INVALID_ARG;
    if (len == 0 || (len % 16) != 0) return ESP_ERR_INVALID_SIZE;

    psa_key_usage_t usage = encrypt ? PSA_KEY_USAGE_ENCRYPT : PSA_KEY_USAGE_DECRYPT;
    psa_key_id_t key_id = import_aes_key(key, key_bytes,
                                          PSA_ALG_CBC_NO_PADDING, usage);
    if (key_id == PSA_KEY_ID_NULL) return ESP_FAIL;

    psa_cipher_operation_t op = PSA_CIPHER_OPERATION_INIT;
    psa_status_t st;

    if (encrypt) {
        st = psa_cipher_encrypt_setup(&op, key_id, PSA_ALG_CBC_NO_PADDING);
    } else {
        st = psa_cipher_decrypt_setup(&op, key_id, PSA_ALG_CBC_NO_PADDING);
    }
    if (st != PSA_SUCCESS) goto done;

    st = psa_cipher_set_iv(&op, iv, 16);
    if (st != PSA_SUCCESS) goto abort_op;

    size_t out1 = 0, out2 = 0;
    st = psa_cipher_update(&op, input, len, output, len, &out1);
    if (st != PSA_SUCCESS) goto abort_op;

    st = psa_cipher_finish(&op, output + out1, len - out1, &out2);
    goto done;

abort_op:
    psa_cipher_abort(&op);
done:
    psa_destroy_key(key_id);
    return (st == PSA_SUCCESS) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_aes256_cbc_encrypt(const uint8_t *key, const uint8_t *iv,
                                        const uint8_t *plaintext, size_t len,
                                        uint8_t *ciphertext_out)
{
    return aes_cbc_op(key, 32, iv, plaintext, len, ciphertext_out, true);
}

static esp_err_t hw_aes256_cbc_decrypt(const uint8_t *key, const uint8_t *iv,
                                        const uint8_t *ciphertext, size_t len,
                                        uint8_t *plaintext_out)
{
    return aes_cbc_op(key, 32, iv, ciphertext, len, plaintext_out, false);
}

static esp_err_t hw_hmac_sha256(const uint8_t *key, size_t key_len,
                                 const uint8_t *data, size_t data_len,
                                 uint8_t *mac_out)
{
    if (!key || !data || !mac_out) return ESP_ERR_INVALID_ARG;
    if (key_len == 0) return ESP_ERR_INVALID_SIZE;

    psa_key_attributes_t attr = PSA_KEY_ATTRIBUTES_INIT;
    psa_set_key_usage_flags(&attr, PSA_KEY_USAGE_SIGN_MESSAGE);
    psa_set_key_algorithm(&attr, PSA_ALG_HMAC(PSA_ALG_SHA_256));
    psa_set_key_type(&attr, PSA_KEY_TYPE_HMAC);

    psa_key_id_t key_id = PSA_KEY_ID_NULL;
    psa_status_t st = psa_import_key(&attr, key, key_len, &key_id);
    if (st != PSA_SUCCESS) return ESP_FAIL;

    size_t mac_len;
    st = psa_mac_compute(key_id, PSA_ALG_HMAC(PSA_ALG_SHA_256),
                         data, data_len,
                         mac_out, 32, &mac_len);
    psa_destroy_key(key_id);
    return (st == PSA_SUCCESS) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_random(uint8_t *buf, size_t len)
{
    if (!buf) return ESP_ERR_INVALID_ARG;
    esp_fill_random(buf, len);
    return ESP_OK;
}

static esp_err_t aes_ecb_op(const uint8_t *key, size_t key_bytes,
                              const uint8_t *input, size_t len,
                              uint8_t *output, bool encrypt)
{
    if (!key || !input || !output) return ESP_ERR_INVALID_ARG;
    if (len == 0 || len % 16 != 0) return ESP_ERR_INVALID_SIZE;

    psa_key_usage_t usage = encrypt ? PSA_KEY_USAGE_ENCRYPT : PSA_KEY_USAGE_DECRYPT;
    psa_key_id_t key_id = import_aes_key(key, key_bytes,
                                          PSA_ALG_ECB_NO_PADDING, usage);
    if (key_id == PSA_KEY_ID_NULL) return ESP_FAIL;

    psa_cipher_operation_t op = PSA_CIPHER_OPERATION_INIT;
    psa_status_t st;

    if (encrypt) {
        st = psa_cipher_encrypt_setup(&op, key_id, PSA_ALG_ECB_NO_PADDING);
    } else {
        st = psa_cipher_decrypt_setup(&op, key_id, PSA_ALG_ECB_NO_PADDING);
    }
    if (st != PSA_SUCCESS) goto done;

    size_t out1 = 0, out2 = 0;
    st = psa_cipher_update(&op, input, len, output, len, &out1);
    if (st != PSA_SUCCESS) goto abort_op;

    st = psa_cipher_finish(&op, output + out1, len - out1, &out2);
    goto done;

abort_op:
    psa_cipher_abort(&op);
done:
    psa_destroy_key(key_id);
    return (st == PSA_SUCCESS) ? ESP_OK : ESP_FAIL;
}

static esp_err_t hw_aes128_ecb_encrypt(const uint8_t *key, const uint8_t *plaintext,
                                        size_t len, uint8_t *ciphertext_out)
{
    return aes_ecb_op(key, 16, plaintext, len, ciphertext_out, true);
}

static esp_err_t hw_aes128_ecb_decrypt(const uint8_t *key, const uint8_t *ciphertext,
                                        size_t len, uint8_t *plaintext_out)
{
    return aes_ecb_op(key, 16, ciphertext, len, plaintext_out, false);
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
