// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * manifest.c — Unified manifest parser for apps, drivers, and firmware.
 *
 * Parses manifest.json files from SD card. Uses minimal hand-written JSON
 * scanning (no external JSON library) consistent with appstore_client.c.
 */

#include "thistle/manifest.h"
#include "thistle/kernel.h"
#include "thistle/permissions.h"
#include "esp_log.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *TAG = "manifest";

/* --------------------------------------------------------------------------
 * Helpers — extract JSON string values by key
 * -------------------------------------------------------------------------- */

/* Find "key": "value" in JSON and copy value into buf (max buf_size-1 chars).
 * Returns true if found. */
static bool json_get_string(const char *json, const char *key, char *buf, size_t buf_size)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);

    const char *p = strstr(json, pattern);
    if (!p) return false;

    p += strlen(pattern);
    /* Skip whitespace and colon */
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;

    if (*p != '"') return false;
    p++; /* skip opening quote */

    size_t i = 0;
    while (*p && *p != '"' && i < buf_size - 1) {
        buf[i++] = *p++;
    }
    buf[i] = '\0';
    return true;
}

/* Find "key": <number> in JSON and return the integer value.
 * Returns true if found. */
static bool json_get_int(const char *json, const char *key, int *out)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);

    const char *p = strstr(json, pattern);
    if (!p) return false;

    p += strlen(pattern);
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;

    char *end;
    long val = strtol(p, &end, 10);
    if (end == p) return false;

    *out = (int)val;
    return true;
}

/* Find "key": true/false in JSON.
 * Returns true if found. */
static bool json_get_bool(const char *json, const char *key, bool *out)
{
    char pattern[80];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);

    const char *p = strstr(json, pattern);
    if (!p) return false;

    p += strlen(pattern);
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;

    if (strncmp(p, "true", 4) == 0) {
        *out = true;
        return true;
    }
    if (strncmp(p, "false", 5) == 0) {
        *out = false;
        return true;
    }
    return false;
}

/* Parse a permissions array like ["radio", "gps", "storage"] into a bitmask.
 * Also supports comma-separated string "radio,gps,storage". */
static uint32_t parse_permissions_field(const char *json)
{
    uint32_t perms = 0;

    /* Find "permissions" field */
    const char *p = strstr(json, "\"permissions\"");
    if (!p) return 0;

    p += strlen("\"permissions\"");
    while (*p == ' ' || *p == '\t' || *p == ':' || *p == '\n' || *p == '\r') p++;

    /* Could be an array ["radio", "gps"] or a string "radio,gps" */
    const char *end;
    if (*p == '[') {
        end = strchr(p, ']');
    } else if (*p == '"') {
        p++;
        end = strchr(p, '"');
    } else {
        return 0;
    }

    if (!end) return 0;

    /* Scan for permission names */
    size_t span = (size_t)(end - p);
    char buf[256];
    if (span >= sizeof(buf)) span = sizeof(buf) - 1;
    memcpy(buf, p, span);
    buf[span] = '\0';

    if (strstr(buf, "radio"))   perms |= PERM_RADIO;
    if (strstr(buf, "gps"))     perms |= PERM_GPS;
    if (strstr(buf, "storage")) perms |= PERM_STORAGE;
    if (strstr(buf, "network")) perms |= PERM_NETWORK;
    if (strstr(buf, "audio"))   perms |= PERM_AUDIO;
    if (strstr(buf, "system"))  perms |= PERM_SYSTEM;
    if (strstr(buf, "ipc"))     perms |= PERM_IPC;

    return perms;
}

/* --------------------------------------------------------------------------
 * manifest_parse_json
 * -------------------------------------------------------------------------- */
esp_err_t manifest_parse_json(const char *json, size_t json_len, thistle_manifest_t *out)
{
    if (!json || !out) return ESP_ERR_INVALID_ARG;

    memset(out, 0, sizeof(*out));

    /* Type (required) */
    char type_str[16] = {0};
    if (json_get_string(json, "type", type_str, sizeof(type_str))) {
        if (strcmp(type_str, "app") == 0) {
            out->type = MANIFEST_TYPE_APP;
        } else if (strcmp(type_str, "driver") == 0) {
            out->type = MANIFEST_TYPE_DRIVER;
        } else if (strcmp(type_str, "firmware") == 0) {
            out->type = MANIFEST_TYPE_FIRMWARE;
        } else {
            ESP_LOGW(TAG, "Unknown manifest type: %s", type_str);
            return ESP_ERR_INVALID_ARG;
        }
    } else {
        ESP_LOGW(TAG, "Manifest missing 'type' field");
        return ESP_ERR_INVALID_ARG;
    }

    /* Identity fields */
    json_get_string(json, "id", out->id, sizeof(out->id));
    json_get_string(json, "name", out->name, sizeof(out->name));
    json_get_string(json, "version", out->version, sizeof(out->version));
    json_get_string(json, "author", out->author, sizeof(out->author));
    json_get_string(json, "description", out->description, sizeof(out->description));

    /* Compatibility */
    json_get_string(json, "min_os", out->min_os, sizeof(out->min_os));
    json_get_string(json, "arch", out->arch, sizeof(out->arch));

    /* Files */
    json_get_string(json, "entry", out->entry, sizeof(out->entry));
    json_get_string(json, "icon", out->icon, sizeof(out->icon));

    /* App-specific */
    out->permissions = parse_permissions_field(json);
    json_get_bool(json, "background", &out->background);
    int mem_kb = 0;
    if (json_get_int(json, "min_memory_kb", &mem_kb)) {
        out->min_memory_kb = (uint32_t)mem_kb;
    }

    /* Driver-specific */
    json_get_string(json, "hal_interface", out->hal_interface, sizeof(out->hal_interface));

    /* Firmware-specific */
    json_get_string(json, "changelog", out->changelog, sizeof(out->changelog));

    /* Validate required fields */
    if (out->id[0] == '\0') {
        ESP_LOGW(TAG, "Manifest missing 'id' field");
        return ESP_ERR_INVALID_ARG;
    }

    ESP_LOGD(TAG, "Parsed manifest: %s (%s) v%s [%s]",
             out->name, out->id, out->version, manifest_type_str(out->type));
    return ESP_OK;
}

/* --------------------------------------------------------------------------
 * manifest_parse_file
 * -------------------------------------------------------------------------- */
esp_err_t manifest_parse_file(const char *json_path, thistle_manifest_t *out)
{
    if (!json_path || !out) return ESP_ERR_INVALID_ARG;

    FILE *f = fopen(json_path, "r");
    if (!f) {
        ESP_LOGD(TAG, "No manifest file: %s", json_path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0 || size > 4096) {
        fclose(f);
        ESP_LOGW(TAG, "Manifest file too large or empty: %s (%ld bytes)", json_path, size);
        return ESP_ERR_INVALID_SIZE;
    }

    char *buf = malloc((size_t)size + 1);
    if (!buf) {
        fclose(f);
        return ESP_ERR_NO_MEM;
    }

    size_t nread = fread(buf, 1, (size_t)size, f);
    fclose(f);
    buf[nread] = '\0';

    esp_err_t ret = manifest_parse_json(buf, nread, out);
    free(buf);
    return ret;
}

/* --------------------------------------------------------------------------
 * manifest_path_from_elf
 *
 * Derives manifest path from ELF path:
 *   "/sdcard/apps/messenger.app.elf" → "/sdcard/apps/messenger.manifest.json"
 *   "/sdcard/drivers/sx1262.drv.elf" → "/sdcard/drivers/sx1262.manifest.json"
 * -------------------------------------------------------------------------- */
void manifest_path_from_elf(const char *elf_path, char *out_path, size_t out_size)
{
    /* Find the last '.' before ".app.elf" or ".drv.elf" */
    const char *app_ext = strstr(elf_path, ".app.elf");
    const char *drv_ext = strstr(elf_path, ".drv.elf");
    const char *ext = app_ext ? app_ext : drv_ext;

    if (ext) {
        size_t prefix_len = (size_t)(ext - elf_path);
        snprintf(out_path, out_size, "%.*s.manifest.json", (int)prefix_len, elf_path);
    } else {
        /* Fallback: just append .manifest.json */
        snprintf(out_path, out_size, "%s.manifest.json", elf_path);
    }
}

/* --------------------------------------------------------------------------
 * manifest_is_compatible
 * -------------------------------------------------------------------------- */
bool manifest_is_compatible(const thistle_manifest_t *manifest)
{
    if (!manifest) return true;

    /* Check architecture if specified */
    if (manifest->arch[0] != '\0') {
        const char *current_arch = CONFIG_IDF_TARGET;  /* "esp32s3", "esp32c3", etc. */
        if (strcmp(manifest->arch, current_arch) != 0) {
            ESP_LOGW(TAG, "Architecture mismatch: manifest requires '%s', running on '%s'",
                     manifest->arch, current_arch);
            return false;
        }
    }

    /* Check min_os version if specified */
    if (manifest->min_os[0] == '\0') {
        return true;
    }

    /* Simple semver comparison: parse major.minor.patch */
    int req_major = 0, req_minor = 0, req_patch = 0;
    sscanf(manifest->min_os, "%d.%d.%d", &req_major, &req_minor, &req_patch);

    if (THISTLE_VERSION_MAJOR > req_major) return true;
    if (THISTLE_VERSION_MAJOR < req_major) return false;
    if (THISTLE_VERSION_MINOR > req_minor) return true;
    if (THISTLE_VERSION_MINOR < req_minor) return false;
    return THISTLE_VERSION_PATCH >= req_patch;
}

/* --------------------------------------------------------------------------
 * manifest_type_str
 * -------------------------------------------------------------------------- */
const char *manifest_type_str(manifest_type_t type)
{
    switch (type) {
        case MANIFEST_TYPE_APP:      return "app";
        case MANIFEST_TYPE_DRIVER:   return "driver";
        case MANIFEST_TYPE_FIRMWARE: return "firmware";
        default:                     return "unknown";
    }
}
