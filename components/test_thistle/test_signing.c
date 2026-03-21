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

/* Real Ed25519 public key for test vector verification */
static const uint8_t s_test_key[THISTLE_SIGN_KEY_SIZE] = {
    0x25, 0xd3, 0xfc, 0xbc, 0x28, 0x2d, 0xb4, 0x6f,
    0xf4, 0x37, 0x78, 0x5c, 0x32, 0x90, 0xaf, 0x73,
    0x98, 0x17, 0xf2, 0x0d, 0xb4, 0x37, 0x88, 0x27,
    0xf9, 0x00, 0xc3, 0xf7, 0x7b, 0xe0, 0x27, 0xb7,
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
     * Verify the first byte's hex encoding. Key[0] = 0x25 → "25",
     * Key[1] = 0xd3 → "d3". Check the first four hex characters.
     */
    TEST_ASSERT_EQUAL_CHAR('2', hex[0]);
    TEST_ASSERT_EQUAL_CHAR('5', hex[1]); /* 0x25 */
    TEST_ASSERT_EQUAL_CHAR('d', hex[2]);
    TEST_ASSERT_EQUAL_CHAR('3', hex[3]); /* 0xd3 */
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

TEST_CASE("test_signing_verify_valid_ed25519: real Ed25519 signature verifies correctly", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    /* "ThistleOS test data" — 19 bytes, signed with matching private key */
    static const uint8_t data[] = {
        0x54, 0x68, 0x69, 0x73, 0x74, 0x6c, 0x65, 0x4f,
        0x53, 0x20, 0x74, 0x65, 0x73, 0x74, 0x20, 0x64,
        0x61, 0x74, 0x61,
    };

    static const uint8_t valid_sig[THISTLE_SIGN_SIG_SIZE] = {
        0xc0, 0x59, 0xf5, 0x99, 0x35, 0x6e, 0x14, 0x19,
        0x4c, 0xe2, 0x98, 0xdc, 0xed, 0x4c, 0x2c, 0xdb,
        0xae, 0x34, 0x81, 0x12, 0xa9, 0x23, 0x0d, 0x0b,
        0x30, 0x09, 0x44, 0x26, 0x8b, 0x3f, 0xca, 0x27,
        0x43, 0x45, 0x75, 0x21, 0x18, 0xa9, 0x6f, 0x32,
        0x46, 0xc4, 0x6f, 0x24, 0xa5, 0xcd, 0xb3, 0xb3,
        0xd4, 0x4f, 0xcb, 0x4b, 0x32, 0x0a, 0xc0, 0xdc,
        0x6e, 0x03, 0x01, 0xaf, 0x50, 0xa6, 0xc9, 0x0c,
    };

    esp_err_t ret = signing_verify(data, sizeof(data), valid_sig);
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("test_signing_verify_tampered_data: modified data fails Ed25519 check", "[signing]")
{
    TEST_ASSERT_EQUAL(ESP_OK, signing_init(s_test_key));

    /* Same data as valid test but with last byte changed */
    static const uint8_t tampered_data[] = {
        0x54, 0x68, 0x69, 0x73, 0x74, 0x6c, 0x65, 0x4f,
        0x53, 0x20, 0x74, 0x65, 0x73, 0x74, 0x20, 0x64,
        0x61, 0x74, 0xFF,
    };

    static const uint8_t valid_sig[THISTLE_SIGN_SIG_SIZE] = {
        0xc0, 0x59, 0xf5, 0x99, 0x35, 0x6e, 0x14, 0x19,
        0x4c, 0xe2, 0x98, 0xdc, 0xed, 0x4c, 0x2c, 0xdb,
        0xae, 0x34, 0x81, 0x12, 0xa9, 0x23, 0x0d, 0x0b,
        0x30, 0x09, 0x44, 0x26, 0x8b, 0x3f, 0xca, 0x27,
        0x43, 0x45, 0x75, 0x21, 0x18, 0xa9, 0x6f, 0x32,
        0x46, 0xc4, 0x6f, 0x24, 0xa5, 0xcd, 0xb3, 0xb3,
        0xd4, 0x4f, 0xcb, 0x4b, 0x32, 0x0a, 0xc0, 0xdc,
        0x6e, 0x03, 0x01, 0xaf, 0x50, 0xa6, 0xc9, 0x0c,
    };

    esp_err_t ret = signing_verify(tampered_data, sizeof(tampered_data), valid_sig);
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
