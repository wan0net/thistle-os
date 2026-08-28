// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

#define APPSTORE_URL_MAX      256
#define APPSTORE_HASH_HEX_LEN 64  /* SHA-256 hex string */

/* Entry types in the catalog */
typedef enum {
    CATALOG_TYPE_APP,        /* .tap — verified and activated as a generation */
    CATALOG_TYPE_FIRMWARE,   /* .bin — applied via OTA */
    CATALOG_TYPE_DRIVER,     /* .drv.elf — loaded as driver */
    CATALOG_TYPE_WM,         /* .wm.elf — loaded as a window manager */
} catalog_entry_type_t;

typedef struct {
    char id[64];
    char name[64];
    char version[16];
    char author[32];
    char description[256];
    char url[APPSTORE_URL_MAX];          /* HTTPS download URL */
    char sig_url[APPSTORE_URL_MAX];      /* Signature file URL */
    char sha256_hex[APPSTORE_HASH_HEX_LEN + 1]; /* Expected SHA-256 hash */
    char permissions[64];                /* Comma-separated */
    char min_os_version[16];
    uint32_t size_bytes;
    catalog_entry_type_t type;
    bool is_signed;
    char compatible_boards[128];
    char detection_bus[8];
    uint16_t detection_address;
    uint16_t detection_chip_id_reg;
    uint16_t detection_chip_id_value;
    char category[32];
    char icon_url[APPSTORE_URL_MAX];
    char screenshots[3][APPSTORE_URL_MAX];
    uint8_t screenshot_count;
    uint16_t rating_stars;
    uint32_t rating_count;
    uint32_t download_count;
    char updated_date[11];
    char changelog[512];
    char package_url[APPSTORE_URL_MAX];
    char package_sig_url[APPSTORE_URL_MAX];
    char package_sha256_hex[APPSTORE_HASH_HEX_LEN + 1];
    uint32_t package_size_bytes;
    uint32_t release_sequence;
    char publisher_key_id[64];
} catalog_entry_t;

#define CATALOG_MAX_ENTRIES 30

/* Fetch the remote catalog JSON from the configured URL.
 * Stores results in entries[], returns count via out_count.
 * Returns ESP_OK on success, ESP_ERR_NOT_SUPPORTED in simulator. */
esp_err_t appstore_fetch_catalog(const char *catalog_url,
                                  catalog_entry_t *entries, int max_entries,
                                  int *out_count);

/* Download a file from URL to a local path on SD card.
 * Optionally verifies SHA-256 hash if expected_sha256_hex is non-NULL.
 * Shows progress via callback (may be NULL). */
typedef void (*download_progress_cb_t)(uint32_t downloaded, uint32_t total, void *user_data);

esp_err_t appstore_download_file(const char *url, const char *dest_path,
                                  const char *expected_sha256_hex,
                                  download_progress_cb_t progress_cb,
                                  void *user_data);

/* Install a catalog entry:
 * - APP:      download signed .tap, verify both signatures and hashes, then
 *             atomically activate /sdcard/apps/<id>/generations/<sequence>/
 * - FIRMWARE: download .bin + .sig to /sdcard/update/, verify sig
 * - DRIVER:   download .drv.elf + .sig to /sdcard/drivers/, verify sig
 */
esp_err_t appstore_install_entry(const catalog_entry_t *entry,
                                  download_progress_cb_t progress_cb,
                                  void *user_data);

/* Get the configured catalog URL (from /sdcard/config/appstore.json or default) */
const char *appstore_get_catalog_url(void);
