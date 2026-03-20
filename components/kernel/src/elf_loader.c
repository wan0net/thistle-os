// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "thistle/elf_loader.h"
#include "thistle/syscall.h"
#include "thistle/signing.h"
#include "thistle/permissions.h"
#include "esp_elf.h"
#include "private/elf_symbol.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

static const char *TAG = "elf_loader";

#define ELF_APP_TASK_STACK   (8192)
#define ELF_APP_TASK_PRIO    (5)
#define MAX_LOADED_APPS      (4)
#define ELF_MAX_SIZE_BYTES   (1024 * 1024)   /* 1 MB hard cap */

/* Internal state for a loaded ELF app */
struct elf_app_handle {
    esp_elf_t    elf;
    char         path[128];
    TaskHandle_t task;
    bool         loaded;
    bool         running;
    app_manifest_t manifest;   /* Cached manifest from .thistle_app ELF section */
};

static struct elf_app_handle s_apps[MAX_LOADED_APPS];

/* --------------------------------------------------------------------------
 * Custom symbol resolver — bridges esp_elf's symbol lookup to our syscall table
 * -------------------------------------------------------------------------- */
static uintptr_t thistle_symbol_resolver(const char *sym_name)
{
    void *addr = syscall_resolve(sym_name);
    if (addr) {
        return (uintptr_t)addr;
    }
    ESP_LOGW(TAG, "Unresolved symbol: %s", sym_name);
    return 0;
}

/* --------------------------------------------------------------------------
 * Task wrapper — runs the ELF entry point in its own FreeRTOS task
 * -------------------------------------------------------------------------- */
static void elf_app_task(void *arg)
{
    struct elf_app_handle *app = (struct elf_app_handle *)arg;

    ESP_LOGI(TAG, "Starting ELF app: %s", app->path);

    /* Call the ELF's entry point via esp_elf_request(elf, opt, argc, argv).
     * opt=0 invokes the default entry point (main/app_main). */
    int err = esp_elf_request(&app->elf, 0, 0, NULL);
    if (err != 0) {
        ESP_LOGE(TAG, "ELF entry point error in '%s': ret=%d", app->path, err);
    } else {
        ESP_LOGI(TAG, "ELF app '%s' exited normally", app->path);
    }

    app->running = false;
    vTaskDelete(NULL);
}

/* --------------------------------------------------------------------------
 * Public API
 * -------------------------------------------------------------------------- */

esp_err_t elf_loader_init(void)
{
    memset(s_apps, 0, sizeof(s_apps));
    ESP_LOGI(TAG, "ELF loader initialised (max %d concurrent apps)", MAX_LOADED_APPS);
    return ESP_OK;
}

esp_err_t elf_app_load(const char *path, elf_app_handle_t *handle)
{
    if (!path || !handle) {
        return ESP_ERR_INVALID_ARG;
    }

    /* ------------------------------------------------------------------ */
    /* 1. Find a free slot                                                  */
    /* ------------------------------------------------------------------ */
    struct elf_app_handle *app = NULL;
    for (int i = 0; i < MAX_LOADED_APPS; i++) {
        if (!s_apps[i].loaded) {
            app = &s_apps[i];
            break;
        }
    }
    if (!app) {
        ESP_LOGE(TAG, "No free ELF slots (max %d)", MAX_LOADED_APPS);
        return ESP_ERR_NO_MEM;
    }

    /* ------------------------------------------------------------------ */
    /* 2. Open and read the ELF file into PSRAM                            */
    /* ------------------------------------------------------------------ */
    FILE *f = fopen(path, "rb");
    if (!f) {
        ESP_LOGE(TAG, "Cannot open ELF: %s", path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0 || size > ELF_MAX_SIZE_BYTES) {
        ESP_LOGE(TAG, "Rejecting ELF '%s': size %ld out of range", path, size);
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    uint8_t *buf = heap_caps_malloc((size_t)size, MALLOC_CAP_SPIRAM);
    if (!buf) {
        ESP_LOGE(TAG, "PSRAM alloc failed for %ld bytes (ELF: %s)", size, path);
        fclose(f);
        return ESP_ERR_NO_MEM;
    }

    size_t nread = fread(buf, 1, (size_t)size, f);
    fclose(f);

    if ((long)nread != size) {
        ESP_LOGE(TAG, "Short read: expected %ld, got %zu (ELF: %s)", size, nread, path);
        free(buf);
        return ESP_ERR_INVALID_SIZE;
    }

    /* ------------------------------------------------------------------ */
    /* 3. Verify ELF signature before allowing execution                   */
    /* ------------------------------------------------------------------ */
    esp_err_t sig_ret = signing_verify_file(path);
    if (sig_ret == ESP_OK) {
        ESP_LOGI(TAG, "ELF signature verified: %s", path);
        permissions_grant(app->manifest.id, PERM_ALL);
    } else if (sig_ret == ESP_ERR_NOT_FOUND) {
        ESP_LOGW(TAG, "ELF unsigned: %s (running in restricted mode)", path);
        permissions_grant(app->manifest.id, PERM_IPC);  /* minimal — no radio/gps/storage/network */
    } else {
        ESP_LOGE(TAG, "ELF signature INVALID: %s (refusing to load)", path);
        free(buf);
        return ESP_ERR_INVALID_CRC;
    }

    /* ------------------------------------------------------------------ */
    /* 4. Initialise the esp_elf context                                   */
    /* ------------------------------------------------------------------ */
    esp_err_t ret = esp_elf_init(&app->elf);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_elf_init failed: %s", esp_err_to_name(ret));
        free(buf);
        return ret;
    }

    /* ------------------------------------------------------------------ */
    /* 5. Register our syscall table as the symbol resolver, then relocate */
    /*                                                                     */
    /* The elf_loader resolves symbols via elf_find_sym() which calls the  */
    /* registered resolver. We set a custom one that delegates to our      */
    /* syscall_resolve() function.                                         */
    /* ------------------------------------------------------------------ */

    ESP_LOGI(TAG, "Relocating '%s' (%ld bytes, %zu exported symbols)",
             path, size, syscall_table_count());

    /* Set our custom symbol resolver before relocating */
    elf_set_symbol_resolver(thistle_symbol_resolver);

    ret = esp_elf_relocate(&app->elf, buf);

    /* The ELF loader has copied/mapped what it needs; free the file buffer */
    free(buf);

    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_elf_relocate failed for '%s': %s", path, esp_err_to_name(ret));
        esp_elf_deinit(&app->elf);
        return ret;
    }

    /* ------------------------------------------------------------------ */
    /* 6. Populate handle metadata                                         */
    /* ------------------------------------------------------------------ */
    strncpy(app->path, path, sizeof(app->path) - 1);
    app->path[sizeof(app->path) - 1] = '\0';
    app->loaded  = true;
    app->running = false;
    app->task    = NULL;

    /* Default manifest — overridden once .thistle_app section parsing is
     * implemented.  The filename (minus extension) is used as a fallback id. */
    const char *basename = strrchr(path, '/');
    basename = basename ? basename + 1 : path;
    app->manifest = (app_manifest_t){
        .id               = app->path,   /* points into stable storage */
        .name             = basename,
        .version          = "0.0.0",
        .allow_background = false,
        .min_memory_kb    = 0,
    };

    *handle = app;
    ESP_LOGI(TAG, "ELF loaded: %s", path);
    return ESP_OK;
}

esp_err_t elf_app_start(elf_app_handle_t handle)
{
    if (!handle || !handle->loaded) {
        return ESP_ERR_INVALID_STATE;
    }
    if (handle->running) {
        ESP_LOGW(TAG, "ELF app already running: %s", handle->path);
        return ESP_ERR_INVALID_STATE;
    }

    BaseType_t rc = xTaskCreate(
        elf_app_task,
        "elf_app",
        ELF_APP_TASK_STACK,
        handle,
        ELF_APP_TASK_PRIO,
        &handle->task
    );

    if (rc != pdPASS) {
        ESP_LOGE(TAG, "xTaskCreate failed for ELF app: %s", handle->path);
        return ESP_ERR_NO_MEM;
    }

    handle->running = true;
    ESP_LOGI(TAG, "ELF app task started: %s", handle->path);
    return ESP_OK;
}

esp_err_t elf_app_unload(elf_app_handle_t handle)
{
    if (!handle) {
        return ESP_ERR_INVALID_ARG;
    }

    /* Forcibly kill the task if still alive */
    if (handle->running && handle->task) {
        vTaskDelete(handle->task);
        handle->task    = NULL;
        handle->running = false;
    }

    /* Release ELF-allocated memory (text, data, bss sections in PSRAM) */
    if (handle->loaded) {
        esp_elf_deinit(&handle->elf);
        handle->loaded = false;
    }

    ESP_LOGI(TAG, "ELF app unloaded: %s", handle->path);
    memset(handle, 0, sizeof(*handle));
    return ESP_OK;
}

const app_manifest_t *elf_app_get_manifest(elf_app_handle_t handle)
{
    if (!handle || !handle->loaded) {
        return NULL;
    }
    return &handle->manifest;
}
