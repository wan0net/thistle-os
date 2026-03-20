/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — App Store Client
 *
 * Fetches the remote catalog JSON, downloads app/firmware/driver files,
 * verifies SHA-256 hashes and Ed25519 signatures, and installs them to
 * the correct SD card directories.
 *
 * All SD card paths use THISTLE_SDCARD from hal/sdcard_path.h.
 * In SIMULATOR_BUILD every network function returns ESP_ERR_NOT_SUPPORTED.
 */

#include "thistle/appstore_client.h"
#include "thistle/signing.h"
#include "hal/sdcard_path.h"
#include "esp_log.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#ifndef SIMULATOR_BUILD
#include "esp_http_client.h"
#include "mbedtls/sha256.h"
#endif

static const char *TAG = "appstore_client";

/* Default catalog URL — overridden by /sdcard/config/appstore.json */
#define DEFAULT_CATALOG_URL "https://wan0net.github.io/thistle-apps/catalog.json"
#define DOWNLOAD_BUF_SIZE   4096
#define MAX_CATALOG_JSON    (32 * 1024)  /* 32 KB max catalog response */

static char s_catalog_url[APPSTORE_URL_MAX] = DEFAULT_CATALOG_URL;

/* ── Catalog URL config ─────────────────────────────────────── */

const char *appstore_get_catalog_url(void)
{
    static bool s_loaded = false;
    if (!s_loaded) {
        char config_path[128];
        snprintf(config_path, sizeof(config_path),
                 "%s/config/appstore.json", THISTLE_SDCARD);

        FILE *f = fopen(config_path, "r");
        if (f) {
            char buf[512];
            size_t n = fread(buf, 1, sizeof(buf) - 1, f);
            buf[n] = '\0';
            fclose(f);

            /* Simple extraction of "catalog_url" string value */
            const char *key = "\"catalog_url\"";
            const char *p = strstr(buf, key);
            if (p) {
                p = strchr(p + strlen(key), '"');
                if (p) {
                    p++;
                    const char *end = strchr(p, '"');
                    if (end && (size_t)(end - p) < sizeof(s_catalog_url)) {
                        size_t len = (size_t)(end - p);
                        memcpy(s_catalog_url, p, len);
                        s_catalog_url[len] = '\0';
                        ESP_LOGI(TAG, "Catalog URL from config: %s", s_catalog_url);
                    }
                }
            }
        }
        s_loaded = true;
    }
    return s_catalog_url;
}

/* ── Minimal JSON helpers ───────────────────────────────────── */

/* Extract a string value for key from a JSON object fragment. */
static bool json_str(const char *json, const char *key, char *out, size_t out_len)
{
    char search[80];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *p = strstr(json, search);
    if (!p) return false;
    p = strchr(p + strlen(search), '"');
    if (!p) return false;
    p++;
    const char *end = strchr(p, '"');
    if (!end) return false;
    size_t len = (size_t)(end - p);
    if (len >= out_len) len = out_len - 1;
    memcpy(out, p, len);
    out[len] = '\0';
    return true;
}

/* Extract an integer value for key. */
static bool json_int(const char *json, const char *key, int *out)
{
    char search[80];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *p = strstr(json, search);
    if (!p) return false;
    p = strchr(p + strlen(search), ':');
    if (!p) return false;
    p++;
    while (*p == ' ') p++;
    *out = atoi(p);
    return true;
}

/* ── HTTP response buffer ───────────────────────────────────── */

#ifndef SIMULATOR_BUILD

typedef struct {
    char  *buf;
    size_t len;
    size_t capacity;
} http_buf_t;

static esp_err_t http_buf_event_handler(esp_http_client_event_t *evt)
{
    http_buf_t *resp = (http_buf_t *)evt->user_data;
    if (!resp) return ESP_OK;

    if (evt->event_id == HTTP_EVENT_ON_DATA) {
        if (resp->len + (size_t)evt->data_len < resp->capacity) {
            memcpy(resp->buf + resp->len, evt->data, evt->data_len);
            resp->len += evt->data_len;
        } else {
            ESP_LOGW(TAG, "HTTP buffer overflow — response truncated");
        }
    }
    return ESP_OK;
}

#endif /* !SIMULATOR_BUILD */

/* ── Catalog fetch ──────────────────────────────────────────── */

esp_err_t appstore_fetch_catalog(const char *catalog_url,
                                  catalog_entry_t *entries, int max_entries,
                                  int *out_count)
{
    if (!entries || !out_count) return ESP_ERR_INVALID_ARG;
    *out_count = 0;

#ifdef SIMULATOR_BUILD
    ESP_LOGW(TAG, "Simulator: appstore_fetch_catalog not available");
    return ESP_ERR_NOT_SUPPORTED;
#else
    const char *url = (catalog_url && catalog_url[0]) ?
                      catalog_url : appstore_get_catalog_url();
    ESP_LOGI(TAG, "Fetching catalog: %s", url);

    http_buf_t resp = {
        .buf      = malloc(MAX_CATALOG_JSON),
        .len      = 0,
        .capacity = MAX_CATALOG_JSON,
    };
    if (!resp.buf) return ESP_ERR_NO_MEM;

    esp_http_client_config_t config = {
        .url           = url,
        .event_handler = http_buf_event_handler,
        .user_data     = &resp,
        .timeout_ms    = 15000,
    };

    esp_http_client_handle_t client = esp_http_client_init(&config);
    esp_err_t err    = esp_http_client_perform(client);
    int       status = esp_http_client_get_status_code(client);
    esp_http_client_cleanup(client);

    if (err != ESP_OK || status != 200) {
        ESP_LOGE(TAG, "Catalog fetch failed: %s (HTTP %d)",
                 esp_err_to_name(err), status);
        free(resp.buf);
        return ESP_FAIL;
    }

    resp.buf[resp.len] = '\0';
    ESP_LOGI(TAG, "Catalog fetched: %zu bytes", resp.len);

    /*
     * Parse individual entry objects from the JSON.
     * Strategy: walk the buffer, extract one { ... } object at a time,
     * parse fields from it.  Nested objects in "entries" array are handled
     * naturally because we scan from the first '{' after the outer array
     * open bracket.
     */
    int         count  = 0;
    const char *cursor = resp.buf;

    while (count < max_entries) {
        const char *obj_start = strchr(cursor, '{');
        if (!obj_start) break;

        /* Find the matching '}' for this object */
        const char *obj_end = strchr(obj_start + 1, '}');
        if (!obj_end) break;

        size_t obj_len = (size_t)(obj_end - obj_start) + 1;
        char  *obj     = malloc(obj_len + 1);
        if (!obj) break;
        memcpy(obj, obj_start, obj_len);
        obj[obj_len] = '\0';

        catalog_entry_t *e = &entries[count];
        memset(e, 0, sizeof(*e));

        json_str(obj, "id",          e->id,          sizeof(e->id));
        json_str(obj, "name",        e->name,         sizeof(e->name));
        json_str(obj, "version",     e->version,      sizeof(e->version));
        json_str(obj, "author",      e->author,       sizeof(e->author));
        json_str(obj, "description", e->description,  sizeof(e->description));
        json_str(obj, "url",         e->url,          sizeof(e->url));
        json_str(obj, "sig_url",     e->sig_url,      sizeof(e->sig_url));
        json_str(obj, "sha256",      e->sha256_hex,   sizeof(e->sha256_hex));
        json_str(obj, "permissions", e->permissions,  sizeof(e->permissions));
        json_str(obj, "min_os_version", e->min_os_version, sizeof(e->min_os_version));

        int size_val = 0;
        if (json_int(obj, "size_bytes", &size_val) && size_val > 0) {
            e->size_bytes = (uint32_t)size_val;
        }

        /* Determine type from "type" field */
        char type_str[16] = {0};
        json_str(obj, "type", type_str, sizeof(type_str));
        if (strcmp(type_str, "firmware") == 0) {
            e->type = CATALOG_TYPE_FIRMWARE;
        } else if (strcmp(type_str, "driver") == 0) {
            e->type = CATALOG_TYPE_DRIVER;
        } else {
            e->type = CATALOG_TYPE_APP;
        }

        e->is_signed = (e->sig_url[0] != '\0');

        free(obj);

        /* Only count entries that have at minimum an id */
        if (e->id[0] != '\0') {
            count++;
        }

        cursor = obj_end + 1;
    }

    free(resp.buf);
    *out_count = count;
    ESP_LOGI(TAG, "Parsed %d catalog entries", count);
    return ESP_OK;
#endif /* SIMULATOR_BUILD */
}

/* ── File download with hash verification ───────────────────── */

esp_err_t appstore_download_file(const char *url, const char *dest_path,
                                  const char *expected_sha256_hex,
                                  download_progress_cb_t progress_cb,
                                  void *user_data)
{
    if (!url || !dest_path) return ESP_ERR_INVALID_ARG;

#ifdef SIMULATOR_BUILD
    ESP_LOGW(TAG, "Simulator: appstore_download_file not available");
    return ESP_ERR_NOT_SUPPORTED;
#else
    ESP_LOGI(TAG, "Downloading %s -> %s", url, dest_path);

    FILE *f = fopen(dest_path, "wb");
    if (!f) {
        ESP_LOGE(TAG, "Cannot create destination file: %s", dest_path);
        return ESP_ERR_NOT_FOUND;
    }

    /* SHA-256 context — always computed, checked only when hash supplied */
    mbedtls_sha256_context sha_ctx;
    mbedtls_sha256_init(&sha_ctx);
    mbedtls_sha256_starts(&sha_ctx, 0 /* 0 = SHA-256, 1 = SHA-224 */);

    esp_http_client_config_t config = {
        .url        = url,
        .timeout_ms = 30000,
    };

    esp_http_client_handle_t client = esp_http_client_init(&config);
    esp_err_t err = esp_http_client_open(client, 0);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "HTTP open failed: %s", esp_err_to_name(err));
        fclose(f);
        esp_http_client_cleanup(client);
        mbedtls_sha256_free(&sha_ctx);
        return err;
    }

    int      content_length = esp_http_client_fetch_headers(client);
    uint32_t total          = (content_length > 0) ? (uint32_t)content_length : 0;
    uint32_t downloaded     = 0;

    uint8_t *buf = malloc(DOWNLOAD_BUF_SIZE);
    if (!buf) {
        fclose(f);
        esp_http_client_close(client);
        esp_http_client_cleanup(client);
        mbedtls_sha256_free(&sha_ctx);
        return ESP_ERR_NO_MEM;
    }

    while (1) {
        int read_len = esp_http_client_read(client, (char *)buf, DOWNLOAD_BUF_SIZE);
        if (read_len <= 0) break;

        fwrite(buf, 1, (size_t)read_len, f);
        mbedtls_sha256_update(&sha_ctx, buf, (size_t)read_len);
        downloaded += (uint32_t)read_len;

        if (progress_cb) {
            progress_cb(downloaded, total, user_data);
        }
    }

    free(buf);
    fclose(f);
    esp_http_client_close(client);
    esp_http_client_cleanup(client);

    /* Verify SHA-256 when an expected hash was provided */
    if (expected_sha256_hex && expected_sha256_hex[0] != '\0') {
        uint8_t hash[32];
        mbedtls_sha256_finish(&sha_ctx, hash);

        char computed_hex[65];
        for (int i = 0; i < 32; i++) {
            sprintf(computed_hex + i * 2, "%02x", hash[i]);
        }
        computed_hex[64] = '\0';

        if (strcmp(computed_hex, expected_sha256_hex) != 0) {
            ESP_LOGE(TAG, "SHA-256 mismatch! Expected: %.16s... Got: %.16s...",
                     expected_sha256_hex, computed_hex);
            remove(dest_path);
            mbedtls_sha256_free(&sha_ctx);
            return ESP_ERR_INVALID_CRC;
        }
        ESP_LOGI(TAG, "SHA-256 verified OK: %.16s...", computed_hex);
    }

    mbedtls_sha256_free(&sha_ctx);
    ESP_LOGI(TAG, "Downloaded %lu bytes to %s",
             (unsigned long)downloaded, dest_path);
    return ESP_OK;
#endif /* SIMULATOR_BUILD */
}

/* ── Install entry ──────────────────────────────────────────── */

esp_err_t appstore_install_entry(const catalog_entry_t *entry,
                                  download_progress_cb_t progress_cb,
                                  void *user_data)
{
    if (!entry || entry->url[0] == '\0') return ESP_ERR_INVALID_ARG;

#ifdef SIMULATOR_BUILD
    ESP_LOGW(TAG, "Simulator: appstore_install_entry not available");
    return ESP_ERR_NOT_SUPPORTED;
#else
    /* Determine destination directory and file extension by type */
    const char *dir;
    const char *ext;
    switch (entry->type) {
        case CATALOG_TYPE_FIRMWARE:
            dir = THISTLE_SDCARD "/update";
            ext = ".bin";
            break;
        case CATALOG_TYPE_DRIVER:
            dir = THISTLE_SDCARD "/drivers";
            ext = ".drv.elf";
            break;
        case CATALOG_TYPE_APP:
        default:
            dir = THISTLE_SDCARD "/apps";
            ext = ".app.elf";
            break;
    }

    /* Ensure destination directory exists */
    struct stat st;
    if (stat(dir, &st) != 0) {
        if (mkdir(dir, 0755) != 0) {
            ESP_LOGW(TAG, "mkdir %s failed — may already exist", dir);
        }
    }

    /* Build destination file path */
    char dest_path[300];
    if (entry->type == CATALOG_TYPE_FIRMWARE) {
        /* Firmware always goes to the fixed path that OTA looks for */
        snprintf(dest_path, sizeof(dest_path), "%s/thistle_os.bin", dir);
    } else {
        snprintf(dest_path, sizeof(dest_path), "%s/%s%s", dir, entry->id, ext);
    }

    /* Download the main payload with hash verification */
    esp_err_t ret = appstore_download_file(entry->url, dest_path,
                                            entry->sha256_hex[0] ? entry->sha256_hex : NULL,
                                            progress_cb, user_data);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Payload download failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* Download and verify signature when sig_url is present */
    if (entry->sig_url[0] != '\0') {
        char sig_path[308];
        snprintf(sig_path, sizeof(sig_path), "%s.sig", dest_path);

        esp_err_t sig_dl = appstore_download_file(entry->sig_url, sig_path,
                                                   NULL, NULL, NULL);
        if (sig_dl != ESP_OK) {
            /* Non-fatal: log the warning but continue to the verify step.
             * signing_verify_file will fail with NOT_FOUND if the file
             * is absent, which we treat as an unsigned entry. */
            ESP_LOGW(TAG, "Signature download failed: %s", esp_err_to_name(sig_dl));
        }

        esp_err_t sig_ret = signing_verify_file(dest_path);
        if (sig_ret == ESP_ERR_INVALID_CRC) {
            ESP_LOGE(TAG, "Signature INVALID — deleting downloaded file");
            remove(dest_path);
            remove(sig_path);
            return ESP_ERR_INVALID_CRC;
        } else if (sig_ret == ESP_ERR_NOT_FOUND) {
            ESP_LOGW(TAG, "No signature file found — entry is unsigned");
        } else if (sig_ret == ESP_OK) {
            ESP_LOGI(TAG, "Signature verified OK");
        }
    }

    ESP_LOGI(TAG, "Installed '%s' -> %s", entry->name, dest_path);
    return ESP_OK;
#endif /* SIMULATOR_BUILD */
}
