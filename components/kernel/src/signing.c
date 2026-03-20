// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * signing.c — ELF app signature verification for ThistleOS
 *
 * Current scheme: SHA-256 hash + HMAC-SHA256 tag (64-byte signature file).
 *   sig[0..31]  = SHA-256 of ELF binary
 *   sig[32..63] = HMAC-SHA256(key, sha256_of_elf)
 *
 * NOTE: This is a placeholder for a full Ed25519 implementation. The public
 * API is Ed25519-compatible (32-byte key, 64-byte signature). Upgrade the
 * signing_verify() internals to use mbedtls PSA Crypto Ed25519 for production.
 */

#include "thistle/signing.h"
#include "esp_log.h"
#include "mbedtls/sha256.h"
#include "mbedtls/md.h"
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

static const char *TAG = "signing";

static uint8_t s_public_key[THISTLE_SIGN_KEY_SIZE];
static char    s_public_key_hex[THISTLE_SIGN_KEY_SIZE * 2 + 1];
static bool    s_initialized = false;

/* --------------------------------------------------------------------------
 * signing_init
 * -------------------------------------------------------------------------- */
esp_err_t signing_init(const uint8_t public_key[THISTLE_SIGN_KEY_SIZE])
{
    if (!public_key) return ESP_ERR_INVALID_ARG;
    memcpy(s_public_key, public_key, THISTLE_SIGN_KEY_SIZE);

    /* Convert to hex string for display */
    for (int i = 0; i < THISTLE_SIGN_KEY_SIZE; i++) {
        sprintf(s_public_key_hex + i * 2, "%02x", public_key[i]);
    }

    s_initialized = true;
    ESP_LOGI(TAG, "Signing initialized, public key: %.16s...", s_public_key_hex);
    return ESP_OK;
}

/* --------------------------------------------------------------------------
 * signing_verify
 *
 * Verifies a 64-byte signature over an arbitrary data buffer.
 *   signature[0..31]  must equal SHA-256(data)
 *   signature[32..63] must equal HMAC-SHA256(public_key, SHA-256(data))
 * -------------------------------------------------------------------------- */
esp_err_t signing_verify(const uint8_t *data, size_t data_len,
                          const uint8_t signature[THISTLE_SIGN_SIG_SIZE])
{
    if (!s_initialized) return ESP_ERR_INVALID_STATE;
    if (!data || !signature) return ESP_ERR_INVALID_ARG;

    /* Step 1: Compute SHA-256 of the data */
    uint8_t hash[32];
    mbedtls_sha256(data, data_len, hash, 0 /* is224=0 => SHA-256 */);

    /* Step 2: First 32 bytes of signature must match the hash */
    if (memcmp(signature, hash, 32) != 0) {
        ESP_LOGW(TAG, "Signature hash mismatch");
        return ESP_ERR_INVALID_CRC;
    }

    /* Step 3: Verify HMAC-SHA256 tag (last 32 bytes of signature) */
    uint8_t expected_hmac[32];
    mbedtls_md_hmac(mbedtls_md_info_from_type(MBEDTLS_MD_SHA256),
                    s_public_key, THISTLE_SIGN_KEY_SIZE,
                    hash, 32,
                    expected_hmac);

    if (memcmp(signature + 32, expected_hmac, 32) != 0) {
        ESP_LOGW(TAG, "Signature HMAC mismatch");
        return ESP_ERR_INVALID_CRC;
    }

    return ESP_OK;
}

/* --------------------------------------------------------------------------
 * signing_verify_file
 *
 * Reads <elf_path>.sig and verifies it against the full ELF binary.
 * Returns:
 *   ESP_OK             — signature present and valid
 *   ESP_ERR_NOT_FOUND  — no .sig file (unsigned app)
 *   ESP_ERR_INVALID_CRC — .sig file present but verification failed
 *   ESP_ERR_INVALID_SIZE, ESP_ERR_NO_MEM — I/O / memory errors
 * -------------------------------------------------------------------------- */
esp_err_t signing_verify_file(const char *elf_path)
{
    if (!s_initialized || !elf_path) return ESP_ERR_INVALID_ARG;

    /* Build sig file path: <elf_path>.sig */
    char sig_path[280];
    snprintf(sig_path, sizeof(sig_path), "%s.sig", elf_path);

    /* Read signature file */
    FILE *sig_f = fopen(sig_path, "rb");
    if (!sig_f) {
        ESP_LOGD(TAG, "No signature file: %s", sig_path);
        return ESP_ERR_NOT_FOUND;
    }

    uint8_t signature[THISTLE_SIGN_SIG_SIZE];
    size_t  sig_read = fread(signature, 1, THISTLE_SIGN_SIG_SIZE, sig_f);
    fclose(sig_f);

    if (sig_read != THISTLE_SIGN_SIG_SIZE) {
        ESP_LOGW(TAG, "Invalid signature file size: %zu (expected %d)", sig_read, THISTLE_SIGN_SIG_SIZE);
        return ESP_ERR_INVALID_SIZE;
    }

    /* Read ELF file */
    FILE *elf_f = fopen(elf_path, "rb");
    if (!elf_f) {
        ESP_LOGE(TAG, "Cannot open ELF for verification: %s", elf_path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(elf_f, 0, SEEK_END);
    long elf_size = ftell(elf_f);
    fseek(elf_f, 0, SEEK_SET);

    if (elf_size <= 0 || elf_size > 1024 * 1024) {
        fclose(elf_f);
        ESP_LOGE(TAG, "ELF size out of range for verification: %ld", elf_size);
        return ESP_ERR_INVALID_SIZE;
    }

    uint8_t *elf_data = malloc((size_t)elf_size);
    if (!elf_data) {
        fclose(elf_f);
        return ESP_ERR_NO_MEM;
    }

    fread(elf_data, 1, (size_t)elf_size, elf_f);
    fclose(elf_f);

    esp_err_t ret = signing_verify(elf_data, (size_t)elf_size, signature);
    free(elf_data);

    if (ret == ESP_OK) {
        ESP_LOGI(TAG, "Signature valid: %s", elf_path);
    } else {
        ESP_LOGW(TAG, "Signature invalid: %s", elf_path);
    }

    return ret;
}

/* --------------------------------------------------------------------------
 * signing_has_signature
 * -------------------------------------------------------------------------- */
bool signing_has_signature(const char *elf_path)
{
    if (!elf_path) return false;
    char sig_path[280];
    snprintf(sig_path, sizeof(sig_path), "%s.sig", elf_path);
    FILE *f = fopen(sig_path, "rb");
    if (f) { fclose(f); return true; }
    return false;
}

/* --------------------------------------------------------------------------
 * signing_get_public_key_hex
 * -------------------------------------------------------------------------- */
const char *signing_get_public_key_hex(void)
{
    return s_initialized ? s_public_key_hex : "(not initialized)";
}
