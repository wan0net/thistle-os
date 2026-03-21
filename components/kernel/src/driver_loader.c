// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "thistle/driver_loader.h"
#include "thistle/syscall.h"
#include "thistle/signing.h"
#include "thistle/manifest.h"
#include "hal/sdcard_path.h"
#include "esp_log.h"
#include "esp_elf.h"
#include "private/elf_symbol.h"
#include "esp_heap_caps.h"
#include "freertos/FreeRTOS.h"
#include <stdio.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

static const char *TAG = "drv_loader";

#define DRIVERS_DIR     THISTLE_SDCARD "/drivers"
#define MAX_DRV_SIZE    (512 * 1024)   /* 512 KB max driver ELF */
#define MAX_LOADED_DRVS 8

/* Loaded driver state */
typedef struct {
    esp_elf_t          elf;
    char               path[128];
    bool               loaded;
    thistle_manifest_t manifest;   /* Parsed from manifest.json if available */
    bool               has_manifest;
} loaded_driver_t;

static loaded_driver_t s_drivers[MAX_LOADED_DRVS];
static int             s_driver_count = 0;

/* Config JSON passed to the currently-loading driver via board.json.
 * Set before calling driver_init(), read by driver via thistle_driver_get_config(). */
static const char *s_current_config = "{}";

/* --------------------------------------------------------------------------
 * Custom symbol resolver — bridges esp_elf to the kernel syscall table,
 * which already exports all HAL registration functions and ESP-IDF basics.
 * -------------------------------------------------------------------------- */
static uintptr_t driver_symbol_resolver(const char *sym_name)
{
    void *addr = syscall_resolve(sym_name);
    if (addr) {
        return (uintptr_t)addr;
    }
    ESP_LOGW(TAG, "Unresolved driver symbol: %s", sym_name);
    return 0;
}

/* --------------------------------------------------------------------------
 * Public API
 * -------------------------------------------------------------------------- */

esp_err_t driver_loader_init(void)
{
    memset(s_drivers, 0, sizeof(s_drivers));
    s_driver_count = 0;
    ESP_LOGI(TAG, "Driver loader initialized (max %d drivers)", MAX_LOADED_DRVS);
    return ESP_OK;
}

int driver_loader_get_count(void)
{
    return s_driver_count;
}

esp_err_t driver_loader_load(const char *path)
{
    if (!path) {
        return ESP_ERR_INVALID_ARG;
    }

    if (s_driver_count >= MAX_LOADED_DRVS) {
        ESP_LOGE(TAG, "No free driver slots (max %d)", MAX_LOADED_DRVS);
        return ESP_ERR_NO_MEM;
    }

    /* ------------------------------------------------------------------ */
    /* 1. Verify signature before loading                                  */
    /* ------------------------------------------------------------------ */
    esp_err_t sig_ret = signing_verify_file(path);
    if (sig_ret == ESP_ERR_INVALID_CRC) {
        ESP_LOGE(TAG, "Driver signature INVALID — refusing to load: %s", path);
        return ESP_ERR_INVALID_CRC;
    }
    if (sig_ret == ESP_ERR_NOT_FOUND) {
        ESP_LOGW(TAG, "Driver unsigned (dev mode): %s", path);
    } else if (sig_ret == ESP_OK) {
        ESP_LOGI(TAG, "Driver signature verified: %s", path);
    }

    /* ------------------------------------------------------------------ */
    /* 2. Try to load manifest.json                                        */
    /* ------------------------------------------------------------------ */
    loaded_driver_t *drv_slot = &s_drivers[s_driver_count];
    char manifest_path[280];
    manifest_path_from_elf(path, manifest_path, sizeof(manifest_path));

    if (manifest_parse_file(manifest_path, &drv_slot->manifest) == ESP_OK) {
        drv_slot->has_manifest = true;
        ESP_LOGI(TAG, "Driver manifest: %s v%s (HAL: %s)",
                 drv_slot->manifest.name, drv_slot->manifest.version,
                 drv_slot->manifest.hal_interface);

        if (!manifest_is_compatible(&drv_slot->manifest)) {
            ESP_LOGE(TAG, "Driver '%s' incompatible (requires OS %s, arch %s)",
                     drv_slot->manifest.id, drv_slot->manifest.min_os, drv_slot->manifest.arch);
            return ESP_ERR_NOT_SUPPORTED;
        }
    } else {
        drv_slot->has_manifest = false;
        ESP_LOGD(TAG, "No manifest for driver: %s", path);
    }

    /* ------------------------------------------------------------------ */
    /* 3. Read ELF file into PSRAM                                         */
    /* ------------------------------------------------------------------ */
    FILE *f = fopen(path, "rb");
    if (!f) {
        ESP_LOGE(TAG, "Cannot open driver ELF: %s", path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0 || size > MAX_DRV_SIZE) {
        ESP_LOGE(TAG, "Rejecting driver '%s': size %ld out of range", path, size);
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    uint8_t *buf = heap_caps_malloc((size_t)size, MALLOC_CAP_SPIRAM);
    if (!buf) {
        ESP_LOGE(TAG, "PSRAM alloc failed for driver (%ld bytes): %s", size, path);
        fclose(f);
        return ESP_ERR_NO_MEM;
    }

    size_t nread = fread(buf, 1, (size_t)size, f);
    fclose(f);

    if ((long)nread != size) {
        ESP_LOGE(TAG, "Short read: expected %ld, got %zu (%s)", size, nread, path);
        free(buf);
        return ESP_ERR_INVALID_SIZE;
    }

    /* ------------------------------------------------------------------ */
    /* 4. Initialise the esp_elf context                                   */
    /* ------------------------------------------------------------------ */
    loaded_driver_t *drv = drv_slot;

    esp_err_t ret = esp_elf_init(&drv->elf);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_elf_init failed for '%s': %s", path, esp_err_to_name(ret));
        free(buf);
        return ret;
    }

    /* ------------------------------------------------------------------ */
    /* 5. Set symbol resolver and relocate                                 */
    /* ------------------------------------------------------------------ */
    elf_set_symbol_resolver(driver_symbol_resolver);

    ESP_LOGI(TAG, "Loading driver: %s (%ld bytes)", path, size);

    ret = esp_elf_relocate(&drv->elf, buf);
    free(buf);  /* esp_elf has taken ownership of what it needs */

    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_elf_relocate failed for '%s': %s", path, esp_err_to_name(ret));
        esp_elf_deinit(&drv->elf);
        return ret;
    }

    /* ------------------------------------------------------------------ */
    /* 6. Call the driver entry point (index 0 = driver_init)             */
    /*    The driver calls hal_*_register() to wire itself into the HAL.  */
    /* ------------------------------------------------------------------ */
    ESP_LOGI(TAG, "Calling driver_init() for: %s", path);
    int init_ret = esp_elf_request(&drv->elf, 0, 0, NULL);
    if (init_ret != 0) {
        ESP_LOGE(TAG, "driver_init() failed for '%s': ret=%d", path, init_ret);
        esp_elf_deinit(&drv->elf);
        return ESP_FAIL;
    }

    /* ------------------------------------------------------------------ */
    /* 7. Record the loaded driver                                         */
    /* ------------------------------------------------------------------ */
    strncpy(drv->path, path, sizeof(drv->path) - 1);
    drv->path[sizeof(drv->path) - 1] = '\0';
    drv->loaded = true;
    s_driver_count++;

    ESP_LOGI(TAG, "Driver loaded successfully: %s", path);
    return ESP_OK;
}

int driver_loader_scan_and_load(void)
{
    DIR *dir = opendir(DRIVERS_DIR);
    if (!dir) {
        ESP_LOGD(TAG, "No drivers directory found at: %s", DRIVERS_DIR);
        return 0;
    }

    int loaded = 0;
    struct dirent *ent;

    while ((ent = readdir(dir)) != NULL) {
        /* Only process .drv.elf files */
        const char *name = ent->d_name;
        size_t len = strlen(name);

        /* Minimum length: "x.drv.elf" = 9 chars */
        if (len < 9) continue;
        if (strcmp(name + len - 8, ".drv.elf") != 0) continue;

        char full_path[512];
        snprintf(full_path, sizeof(full_path), "%s/%s", DRIVERS_DIR, name);

        esp_err_t ret = driver_loader_load(full_path);
        if (ret == ESP_OK) {
            loaded++;
        } else {
            ESP_LOGW(TAG, "Failed to load driver '%s': %s", name, esp_err_to_name(ret));
        }
    }

    closedir(dir);
    ESP_LOGI(TAG, "Scanned %s: %d driver(s) loaded", DRIVERS_DIR, loaded);
    return loaded;
}

esp_err_t driver_loader_load_with_config(const char *path, const char *config_json)
{
    /* Store config so the driver can retrieve it via thistle_driver_get_config() */
    s_current_config = config_json ? config_json : "{}";
    esp_err_t ret = driver_loader_load(path);
    s_current_config = "{}";  /* Reset after load */
    return ret;
}

const char *driver_loader_get_config(void)
{
    return s_current_config;
}
