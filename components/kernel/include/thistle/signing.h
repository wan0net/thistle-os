// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define THISTLE_SIGN_KEY_SIZE    32   /* Ed25519 public key */
#define THISTLE_SIGN_SIG_SIZE    64   /* Ed25519 signature */

/* Initialize the signing subsystem with the trusted public key.
 * The key is typically embedded in firmware at compile time. */
esp_err_t signing_init(const uint8_t public_key[THISTLE_SIGN_KEY_SIZE]);

/* Verify an Ed25519 signature over a data buffer.
 * Returns ESP_OK if signature is valid, ESP_ERR_INVALID_CRC if invalid. */
esp_err_t signing_verify(const uint8_t *data, size_t data_len,
                          const uint8_t signature[THISTLE_SIGN_SIG_SIZE]);

/* Verify a .app.elf file's signature.
 * Expects a .sig file alongside the ELF (e.g., hello.app.elf.sig).
 * Returns ESP_OK if signed and valid, ESP_ERR_NOT_FOUND if no sig file,
 * ESP_ERR_INVALID_CRC if sig is invalid. */
esp_err_t signing_verify_file(const char *elf_path);

/* Check if an app is signed (does the .sig file exist?) */
bool signing_has_signature(const char *elf_path);

/* Get the trusted public key (hex string) */
const char *signing_get_public_key_hex(void);
