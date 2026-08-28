// SPDX-License-Identifier: BSD-3-Clause
// Host app loader for simulator-only TAP packages.

#include "sim_app_loader.h"

#include "sim_vfs.h"
#include "thistle/app_manager.h"
#include "thistle_app.h"

#include <dirent.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_HOST_APPS 16

typedef struct {
    void *module;
    char id[64];
    char name[64];
    char version[32];
    app_manifest_t manifest;
    app_entry_t entry;
} sim_host_app_t;

static sim_host_app_t s_apps[MAX_HOST_APPS];
static int s_app_count;

static int valid_app_id(const char *id)
{
    if (!id || !id[0]) return 0;
    for (const unsigned char *p = (const unsigned char *)id; *p; p++) {
        if ((*p >= 'a' && *p <= 'z') || (*p >= '0' && *p <= '9') ||
            *p == '.' || *p == '-' || *p == '_') continue;
        return 0;
    }
    return 1;
}

static int read_active_sequence(const char *app_dir, unsigned long *sequence)
{
    char path[768];
    snprintf(path, sizeof(path), "%s/active.json", app_dir);
    FILE *file = fopen(path, "rb");
    if (!file) return -1;
    char json[256] = {0};
    size_t length = fread(json, 1, sizeof(json) - 1, file);
    fclose(file);
    json[length] = '\0';
    const char *field = strstr(json, "\"release_sequence\"");
    if (!field || !(field = strchr(field, ':'))) return -1;
    char *end = NULL;
    unsigned long parsed = strtoul(field + 1, &end, 10);
    if (parsed == 0 || end == field + 1) return -1;
    *sequence = parsed;
    return 0;
}

static int register_bundle(const char *app_id, const char *bundle_path)
{
    if (s_app_count >= MAX_HOST_APPS) return -1;
    void *module = dlopen(bundle_path, RTLD_NOW | RTLD_LOCAL);
    if (!module) {
        fprintf(stderr, "[sim_app_loader] dlopen failed for %s: %s\n",
                bundle_path, dlerror());
        return -1;
    }
    const thistle_app_t **exported =
        (const thistle_app_t **)dlsym(module, "_thistle_app_entry");
    if (!exported || !*exported) {
        fprintf(stderr, "[sim_app_loader] missing _thistle_app_entry: %s\n",
                bundle_path);
        dlclose(module);
        return -1;
    }
    const thistle_app_t *app = *exported;
    if (!app->id || strcmp(app->id, app_id) != 0 || !valid_app_id(app->id)) {
        fprintf(stderr, "[sim_app_loader] package/export app ID mismatch: %s\n",
                bundle_path);
        dlclose(module);
        return -1;
    }

    sim_host_app_t *loaded = &s_apps[s_app_count];
    memset(loaded, 0, sizeof(*loaded));
    loaded->module = module;
    snprintf(loaded->id, sizeof(loaded->id), "%s", app->id);
    snprintf(loaded->name, sizeof(loaded->name), "%s", app->name ? app->name : app->id);
    snprintf(loaded->version, sizeof(loaded->version), "%s",
             app->version ? app->version : "0.0.0");
    loaded->manifest.id = loaded->id;
    loaded->manifest.name = loaded->name;
    loaded->manifest.version = loaded->version;
    loaded->manifest.allow_background = app->allow_background;
    loaded->entry.on_create = app->on_create;
    loaded->entry.on_start = app->on_start;
    loaded->entry.on_pause = app->on_pause;
    loaded->entry.on_resume = app->on_resume;
    loaded->entry.on_destroy = app->on_destroy;
    loaded->entry.manifest = &loaded->manifest;

    if (app_manager_register(&loaded->entry) != 0) {
        fprintf(stderr, "[sim_app_loader] registration failed: %s\n", app->id);
        dlclose(module);
        memset(loaded, 0, sizeof(*loaded));
        return -1;
    }
    s_app_count++;
    printf("TAP app registered: %s %s\n", loaded->id, loaded->version);
    return 0;
}

int sim_app_loader_scan_and_register(void)
{
    char apps_root[640];
    snprintf(apps_root, sizeof(apps_root), "%s/apps", sim_vfs_get_sdcard_path());
    DIR *apps = opendir(apps_root);
    if (!apps) return 0;

    int registered = 0;
    struct dirent *item;
    while ((item = readdir(apps)) != NULL) {
        if (item->d_name[0] == '.' || !valid_app_id(item->d_name)) continue;
        char app_dir[768];
        snprintf(app_dir, sizeof(app_dir), "%s/%s", apps_root, item->d_name);
        unsigned long sequence;
        if (read_active_sequence(app_dir, &sequence) != 0) continue;
        char bundle[896];
        snprintf(bundle, sizeof(bundle), "%s/generations/%lu/app.app.elf",
                 app_dir, sequence);
        if (register_bundle(item->d_name, bundle) == 0) registered++;
    }
    closedir(apps);
    return registered;
}
