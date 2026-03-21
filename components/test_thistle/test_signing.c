/*
 * test_signing.c — Unit tests for the ThistleOS signing subsystem
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Hardware-independent tests covering signing_init(), public key retrieval,
 * and the file-based API surface against non-existent paths. Tests that
 * require an actual filesystem with valid ELF + .sig files are not included
 * here as they are hardware-dependent.
 */

#include "unity.h"
#include "thistle/signing.h"
#include <string.h>
#include <stdint.h>

/* A deterministic 32-byte test key — all bytes set to their index value */
static const uint8_t s_test_key[THISTLE_SIGN_KEY_SIZE] = {
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
    0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
};

/* --------------------------------------------------------------------------
 * test_signing_init
 * -------------------------------------------------------------------------- */

TEST_CASE("test_signing_init: returns ESP_OK for valid key", "[signing]")
{
    esp_err_t ret = signing_init(s_test_key);
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

/* --------------------------------------------------------------------------
 * test_signing_get_public_key_hex
 * -------------------------------------------------------------------------- */

TEST_CASE("test_signing_get_public_key_hex: returns non-empty hex string after init", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    const char *hex = signing_get_public_key_hex();
    TEST_ASSERT_NOT_NULL(hex);

    /* Must be non-empty and at least as long as 2 hex digits per byte */
    size_t len = strlen(hex);
    TEST_ASSERT_GREATER_OR_EQUAL(THISTLE_SIGN_KEY_SIZE * 2, len);

    /*
     * Verify the first byte's hex encoding. Key[0] = 0x00 → "00",
     * Key[1] = 0x01 → "01", etc. Check the first four bytes.
     */
    TEST_ASSERT_EQUAL_CHAR('0', hex[0]);
    TEST_ASSERT_EQUAL_CHAR('0', hex[1]); /* 0x00 */
    TEST_ASSERT_EQUAL_CHAR('0', hex[2]);
    TEST_ASSERT_EQUAL_CHAR('1', hex[3]); /* 0x01 */
}

/* --------------------------------------------------------------------------
 * test_signing_has_signature_false
 * -------------------------------------------------------------------------- */

TEST_CASE("test_signing_has_signature_false: non-existent file reports unsigned", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    /*
     * There is no file at this path on the test host, so no .sig file exists.
     * The function must return false without crashing.
     */
    bool has_sig = signing_has_signature("/nonexistent/path/app.elf");
    TEST_ASSERT_FALSE(has_sig);
}

/* --------------------------------------------------------------------------
 * test_signing_verify_file_not_found
 * -------------------------------------------------------------------------- */

TEST_CASE("test_signing_verify_file_not_found: missing file returns ESP_ERR_NOT_FOUND", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    /*
     * There is no .sig file alongside a non-existent ELF path. The
     * implementation opens <elf_path>.sig first; if that fopen fails it
     * returns ESP_ERR_NOT_FOUND (see signing.c line ~106).
     */
    esp_err_t ret = signing_verify_file("/nonexistent/path/app.elf");
    TEST_ASSERT_EQUAL(ESP_ERR_NOT_FOUND, ret);
}

/* --------------------------------------------------------------------------
 * Additional edge-case tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_signing_verify_null_data: signing_verify with NULL data returns error", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    static const uint8_t dummy_sig[THISTLE_SIGN_SIG_SIZE] = {0};
    esp_err_t ret = signing_verify(NULL, 32, dummy_sig);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_signing_verify_null_signature: signing_verify with NULL signature returns error", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    static const uint8_t data[4] = {0x01, 0x02, 0x03, 0x04};
    esp_err_t ret = signing_verify(data, sizeof(data), NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_signing_init_null_key: signing_init(NULL) returns ESP_ERR_INVALID_ARG", "[signing]")
{
    esp_err_t ret = signing_init(NULL);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, ret);
}

TEST_CASE("test_signing_verify_zero_length: signing_verify with data_len=0 returns error", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    static const uint8_t dummy_sig[THISTLE_SIGN_SIG_SIZE] = {0};
    static const uint8_t data[1] = {0};
    esp_err_t ret = signing_verify(data, 0, dummy_sig);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_signing_verify_bad_signature: all-zero signature fails with ESP_ERR_INVALID_CRC", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    static const uint8_t data[16] = {
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    };
    static const uint8_t bad_sig[THISTLE_SIGN_SIG_SIZE] = {0};

    esp_err_t ret = signing_verify(data, sizeof(data), bad_sig);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_CRC, ret);
}

TEST_CASE("test_signing_public_key_hex_deterministic: same key produces same hex string", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));
    const char *hex1 = signing_get_public_key_hex();
    TEST_ASSERT_NOT_NULL(hex1);
    size_t len1 = strlen(hex1);

    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));
    const char *hex2 = signing_get_public_key_hex();
    TEST_ASSERT_NOT_NULL(hex2);

    TEST_ASSERT_EQUAL_size_t(len1, strlen(hex2));
    TEST_ASSERT_EQUAL_STRING(hex1, hex2);
}

TEST_CASE("test_signing_verify_file_null_path: NULL path returns error", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    esp_err_t ret = signing_verify_file(NULL);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_signing_has_signature_null_path: NULL path returns false without crash", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    bool result = signing_has_signature(NULL);
    TEST_ASSERT_FALSE(result);
}
