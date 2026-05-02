/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — app permissions subsystem
 */
#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/* Permission flags — each bit represents a capability */
typedef enum {
    PERM_RADIO      = (1 << 0),   /* LoRa radio send/receive */
    PERM_GPS        = (1 << 1),   /* GPS location access */
    PERM_STORAGE    = (1 << 2),   /* SD card file access */
    PERM_NETWORK    = (1 << 3),   /* WiFi/BLE/4G network */
    PERM_AUDIO      = (1 << 4),   /* Audio playback/recording */
    PERM_SYSTEM     = (1 << 5),   /* System settings, reboot, OTA */
    PERM_IPC        = (1 << 6),   /* Inter-process communication */
    PERM_ALL        = 0x7F,       /* All permissions (built-in apps) */
} permission_t;

/* Permission set is a bitmask of permission_t values */
typedef uint32_t permission_set_t;

/* Returned by permissions_check() when the app lacks the requested permission.
 * ESP-IDF v6 added ESP_ERR_NOT_ALLOWED to esp_err.h; guard against redefinition. */
#ifndef ESP_ERR_NOT_ALLOWED
#define ESP_ERR_NOT_ALLOWED (ESP_ERR_INVALID_STATE + 0x100)
#endif

/* Initialize permissions subsystem */
esp_err_t permissions_init(void);

/* Grant permissions to an app (called during app registration/load) */
esp_err_t permissions_grant(const char *app_id, permission_set_t perms);

/* Revoke permissions from an app */
esp_err_t permissions_revoke(const char *app_id, permission_set_t perms);

/* Check if an app has a specific permission.
 * Returns ESP_OK if granted, ESP_ERR_NOT_ALLOWED if denied. */
esp_err_t permissions_check(const char *app_id, permission_t perm);

/* Get all permissions for an app */
permission_set_t permissions_get(const char *app_id);

/* Parse permission names from app manifest strings.
 * Accepts: "radio", "gps", "storage", "network", "audio", "system", "ipc"
 * Returns the corresponding permission_t flag, or 0 if unknown. */
permission_t permissions_parse(const char *name);

/* Convert permission set to human-readable string (comma-separated).
 * Writes to buf, returns buf. */
char *permissions_to_string(permission_set_t perms, char *buf, size_t buf_len);
