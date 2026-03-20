/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — app permissions subsystem
 *
 * Advisory permission enforcement for ThistleOS apps.
 * Full enforcement (blocking syscalls per-caller) requires a
 * FreeRTOS task → app_id mapping, which is tracked as future work.
 */
#include "thistle/permissions.h"
#include "esp_log.h"
#include <string.h>

static const char *TAG = "perms";

#define MAX_APPS 16

typedef struct {
    char app_id[64];
    permission_set_t granted;
    bool active;
} app_perms_t;

static app_perms_t s_apps[MAX_APPS];

esp_err_t permissions_init(void)
{
    memset(s_apps, 0, sizeof(s_apps));
    ESP_LOGI(TAG, "Permissions subsystem initialized");
    return ESP_OK;
}

static app_perms_t *find_app(const char *app_id)
{
    if (!app_id) return NULL;
    for (int i = 0; i < MAX_APPS; i++) {
        if (s_apps[i].active && strcmp(s_apps[i].app_id, app_id) == 0) {
            return &s_apps[i];
        }
    }
    return NULL;
}

static app_perms_t *find_or_create(const char *app_id)
{
    app_perms_t *existing = find_app(app_id);
    if (existing) return existing;

    for (int i = 0; i < MAX_APPS; i++) {
        if (!s_apps[i].active) {
            strncpy(s_apps[i].app_id, app_id, sizeof(s_apps[i].app_id) - 1);
            s_apps[i].app_id[sizeof(s_apps[i].app_id) - 1] = '\0';
            s_apps[i].active = true;
            s_apps[i].granted = 0;
            return &s_apps[i];
        }
    }
    return NULL;
}

esp_err_t permissions_grant(const char *app_id, permission_set_t perms)
{
    if (!app_id) return ESP_ERR_INVALID_ARG;

    app_perms_t *app = find_or_create(app_id);
    if (!app) {
        ESP_LOGE(TAG, "No free permission slots for app: %s", app_id);
        return ESP_ERR_NO_MEM;
    }

    app->granted |= perms;
    ESP_LOGI(TAG, "Granted permissions 0x%lx to %s (total: 0x%lx)",
             (unsigned long)perms, app_id, (unsigned long)app->granted);
    return ESP_OK;
}

esp_err_t permissions_revoke(const char *app_id, permission_set_t perms)
{
    app_perms_t *app = find_app(app_id);
    if (!app) return ESP_ERR_NOT_FOUND;

    app->granted &= ~perms;
    ESP_LOGI(TAG, "Revoked permissions 0x%lx from %s (remaining: 0x%lx)",
             (unsigned long)perms, app_id, (unsigned long)app->granted);
    return ESP_OK;
}

esp_err_t permissions_check(const char *app_id, permission_t perm)
{
    app_perms_t *app = find_app(app_id);
    if (!app) {
        ESP_LOGW(TAG, "Permission check for unknown app: %s", app_id ? app_id : "(null)");
        return ESP_ERR_NOT_FOUND;
    }

    if (app->granted & perm) {
        return ESP_OK;
    }

    ESP_LOGW(TAG, "Permission denied: %s lacks permission 0x%x", app_id, (unsigned)perm);
    return ESP_ERR_NOT_ALLOWED;
}

permission_set_t permissions_get(const char *app_id)
{
    app_perms_t *app = find_app(app_id);
    return app ? app->granted : 0;
}

permission_t permissions_parse(const char *name)
{
    if (!name) return 0;
    if (strcmp(name, "radio") == 0)   return PERM_RADIO;
    if (strcmp(name, "gps") == 0)     return PERM_GPS;
    if (strcmp(name, "storage") == 0) return PERM_STORAGE;
    if (strcmp(name, "network") == 0) return PERM_NETWORK;
    if (strcmp(name, "audio") == 0)   return PERM_AUDIO;
    if (strcmp(name, "system") == 0)  return PERM_SYSTEM;
    if (strcmp(name, "ipc") == 0)     return PERM_IPC;
    return 0;
}

char *permissions_to_string(permission_set_t perms, char *buf, size_t buf_len)
{
    if (!buf || buf_len == 0) return buf;
    buf[0] = '\0';

    static const struct { permission_t flag; const char *name; } map[] = {
        { PERM_RADIO,   "radio"   },
        { PERM_GPS,     "gps"     },
        { PERM_STORAGE, "storage" },
        { PERM_NETWORK, "network" },
        { PERM_AUDIO,   "audio"   },
        { PERM_SYSTEM,  "system"  },
        { PERM_IPC,     "ipc"     },
    };

    size_t pos = 0;
    for (int i = 0; i < 7; i++) {
        if (perms & map[i].flag) {
            if (pos > 0 && pos < buf_len - 1) {
                buf[pos++] = ',';
            }
            size_t nlen = strlen(map[i].name);
            if (pos + nlen < buf_len) {
                memcpy(buf + pos, map[i].name, nlen);
                pos += nlen;
            }
        }
    }
    buf[pos < buf_len ? pos : buf_len - 1] = '\0';
    return buf;
}
