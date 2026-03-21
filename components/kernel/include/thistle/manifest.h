// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/* Manifest types — apps, drivers, and firmware share the same schema */
typedef enum {
    MANIFEST_TYPE_APP,
    MANIFEST_TYPE_DRIVER,
    MANIFEST_TYPE_FIRMWARE,
} manifest_type_t;

/* Unified manifest for apps, drivers, and firmware.
 *
 * On SD card: manifest.json sits alongside the ELF/binary.
 * In app store: catalog entries map directly to this struct.
 * Built-in apps: use the lightweight app_manifest_t (pointer-based).
 *
 * Naming: <basename>.manifest.json alongside <basename>.app.elf
 *   e.g., messenger.manifest.json + messenger.app.elf
 */
typedef struct {
    manifest_type_t type;

    /* Identity */
    char id[64];               /* Reverse-domain: "com.thistle.messenger" */
    char name[32];             /* Display name */
    char version[16];          /* Semver: "1.2.0" */
    char author[32];           /* Author/publisher */
    char description[128];     /* Short description */

    /* Compatibility */
    char min_os[16];           /* Minimum ThistleOS version: "0.1.0" */
    char arch[16];             /* Target architecture: "esp32s3", "esp32c3", etc. */

    /* Files */
    char entry[64];            /* Entry filename: "messenger.app.elf" */
    char icon[64];             /* Icon filename (apps only, optional) */

    /* App-specific */
    uint32_t permissions;      /* Permission bitmask (PERM_RADIO | PERM_GPS | ...) */
    bool background;           /* Can run in background */
    uint32_t min_memory_kb;    /* Minimum PSRAM required */

    /* Driver-specific */
    char hal_interface[16];    /* HAL interface: "display", "radio", "input", etc. */

    /* Firmware-specific */
    char changelog[256];       /* What changed in this version */
} thistle_manifest_t;

/* Parse a manifest.json file from the given path.
 * Returns ESP_OK on success, ESP_ERR_NOT_FOUND if file missing,
 * ESP_ERR_INVALID_ARG on parse error. */
esp_err_t manifest_parse_file(const char *json_path, thistle_manifest_t *out);

/* Parse a manifest from a JSON string buffer.
 * Returns ESP_OK on success. */
esp_err_t manifest_parse_json(const char *json, size_t json_len, thistle_manifest_t *out);

/* Derive the manifest.json path from an ELF path.
 * e.g., "/sdcard/apps/messenger.app.elf" → "/sdcard/apps/messenger.manifest.json"
 * Writes result to out_path (must be at least 280 bytes). */
void manifest_path_from_elf(const char *elf_path, char *out_path, size_t out_size);

/* Check if the manifest's min_os version is compatible with the running kernel.
 * Returns true if compatible (min_os <= current version). */
bool manifest_is_compatible(const thistle_manifest_t *manifest);

/* Convert manifest type enum to string ("app", "driver", "firmware") */
const char *manifest_type_str(manifest_type_t type);
