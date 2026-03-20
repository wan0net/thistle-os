#include "thistle/app_manager.h"
#include "thistle/kernel.h"
#include "thistle/event.h"

#include "esp_log.h"
#include "esp_heap_caps.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>
#include <limits.h>

static const char *TAG = "app_mgr";

#define APP_SLOTS_MAX 8
#define APP_MEMORY_THRESHOLD_BYTES (50 * 1024)  /* 50 KB minimum free heap */

typedef struct {
    const app_entry_t *entry;
    app_state_t        state;
    app_handle_t       handle;
    TaskHandle_t       task;         /* Reserved for future per-app task support */
    uint32_t           last_used_ms; /* kernel_uptime_ms() when app was last foreground */
} app_slot_t;

static app_slot_t s_slots[APP_SLOTS_MAX];
static int s_slot_count = 0;

/* --------------------------------------------------------------------------
 * Internal helpers
 * -------------------------------------------------------------------------- */

static app_slot_t *slot_by_handle(app_handle_t handle)
{
    if (handle < 0 || handle >= APP_SLOTS_MAX) {
        return NULL;
    }
    if (s_slots[handle].entry == NULL) {
        return NULL;
    }
    return &s_slots[handle];
}

static app_slot_t *slot_by_id(const char *app_id)
{
    for (int i = 0; i < APP_SLOTS_MAX; i++) {
        if (s_slots[i].entry == NULL) {
            continue;
        }
        if (s_slots[i].entry->manifest != NULL &&
            strcmp(s_slots[i].entry->manifest->id, app_id) == 0) {
            return &s_slots[i];
        }
    }
    return NULL;
}

static app_slot_t *foreground_slot(void)
{
    for (int i = 0; i < APP_SLOTS_MAX; i++) {
        if (s_slots[i].state == APP_STATE_RUNNING) {
            return &s_slots[i];
        }
    }
    return NULL;
}

static void pause_foreground(void)
{
    app_slot_t *fg = foreground_slot();
    if (fg == NULL) {
        return;
    }
    ESP_LOGI(TAG, "Pausing foreground app '%s'", fg->entry->manifest->id);
    if (fg->entry->on_pause) {
        fg->entry->on_pause();
    }
    fg->state = APP_STATE_BACKGROUNDED;
}

static void evict_lru_app(void)
{
    int      oldest_idx  = -1;
    uint32_t oldest_time = UINT32_MAX;

    for (int i = 0; i < APP_SLOTS_MAX; i++) {
        if (s_slots[i].entry == NULL) continue;
        if (s_slots[i].state == APP_STATE_UNLOADED) continue;
        if (s_slots[i].state == APP_STATE_RUNNING)  continue; /* never evict foreground */

        /* Never evict the launcher */
        if (s_slots[i].entry->manifest != NULL &&
            strcmp(s_slots[i].entry->manifest->id, "com.thistle.launcher") == 0) continue;

        if (s_slots[i].last_used_ms < oldest_time) {
            oldest_time = s_slots[i].last_used_ms;
            oldest_idx  = i;
        }
    }

    if (oldest_idx >= 0) {
        const char *name = (s_slots[oldest_idx].entry->manifest != NULL)
                           ? s_slots[oldest_idx].entry->manifest->name
                           : "?";
        ESP_LOGI(TAG, "Evicting LRU app: %s (last used %lu ms ago)",
                 name,
                 (unsigned long)(kernel_uptime_ms() - oldest_time));
        app_manager_kill((app_handle_t)oldest_idx);
    } else {
        ESP_LOGW(TAG, "evict_lru_app: no evictable app found");
    }
}

/* --------------------------------------------------------------------------
 * Public API
 * -------------------------------------------------------------------------- */

esp_err_t app_manager_init(void)
{
    memset(s_slots, 0, sizeof(s_slots));
    s_slot_count = 0;
    ESP_LOGI(TAG, "App manager initialized (%d slots)", APP_SLOTS_MAX);
    return ESP_OK;
}

esp_err_t app_manager_register(const app_entry_t *app)
{
    if (app == NULL || app->manifest == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    for (int i = 0; i < APP_SLOTS_MAX; i++) {
        if (s_slots[i].entry == NULL) {
            s_slots[i].entry  = app;
            s_slots[i].state  = APP_STATE_UNLOADED;
            s_slots[i].handle = (app_handle_t)i;
            s_slots[i].task   = NULL;
            s_slot_count++;
            ESP_LOGI(TAG, "Registered app '%s' as handle %d", app->manifest->id, i);
            return ESP_OK;
        }
    }

    ESP_LOGE(TAG, "No free app slots (max %d)", APP_SLOTS_MAX);
    return ESP_ERR_NO_MEM;
}

esp_err_t app_manager_launch(const char *app_id)
{
    if (app_id == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    app_slot_t *target = slot_by_id(app_id);
    if (target == NULL) {
        ESP_LOGE(TAG, "app_manager_launch: unknown app '%s'", app_id);
        return ESP_ERR_NOT_FOUND;
    }

    /* Check available heap before loading; evict LRU app if memory is low */
    size_t free_heap = heap_caps_get_free_size(MALLOC_CAP_DEFAULT);
    if (free_heap < APP_MEMORY_THRESHOLD_BYTES) {
        ESP_LOGW(TAG, "Low memory (%zu bytes free), evicting LRU app", free_heap);
        evict_lru_app();
    }

    /* Remember the current foreground so we can restore it if launch fails. */
    app_slot_t *prev_fg = foreground_slot();

    /* Track whether this is the very first launch (on_create will run) */
    bool fresh_launch = (target->state == APP_STATE_UNLOADED);

    /* Pause whatever is currently in the foreground */
    pause_foreground();

    /* If the app hasn't been created yet, call on_create */
    if (fresh_launch) {
        target->state = APP_STATE_LOADING;
        ESP_LOGI(TAG, "Creating app '%s'", app_id);
        if (target->entry->on_create) {
            esp_err_t ret = target->entry->on_create();
            if (ret != ESP_OK) {
                ESP_LOGE(TAG, "on_create failed for '%s': %s", app_id, esp_err_to_name(ret));
                target->state = APP_STATE_UNLOADED;
                /* Restore the previous foreground app so it isn't left stranded
                 * in BACKGROUNDED state with no way to resume. */
                if (prev_fg != NULL) {
                    ESP_LOGI(TAG, "Restoring foreground app '%s' after launch failure",
                             prev_fg->entry->manifest->id);
                    prev_fg->state = APP_STATE_RUNNING;
                    if (prev_fg->entry->on_resume) {
                        prev_fg->entry->on_resume();
                    }
                }
                return ret;
            }
        }
    }

    /* Bring to foreground.
     * First time: call on_start.  Subsequent times (app was backgrounded):
     * call on_resume so the app can show its UI without re-initialising. */
    target->state         = APP_STATE_RUNNING;
    target->last_used_ms  = kernel_uptime_ms();
    if (fresh_launch) {
        ESP_LOGI(TAG, "Starting app '%s' (handle %d)", app_id, target->handle);
        if (target->entry->on_start) {
            target->entry->on_start();
        }
    } else {
        ESP_LOGI(TAG, "Resuming app '%s' (handle %d)", app_id, target->handle);
        if (target->entry->on_resume) {
            target->entry->on_resume();
        }
    }

    /* Publish event */
    event_t ev = {
        .type      = EVENT_APP_LAUNCHED,
        .timestamp = 0,   /* caller may fill with kernel_uptime_ms() */
        .data      = (void *)target->entry->manifest->id,
        .data_len  = 0,
    };
    event_publish(&ev);

    return ESP_OK;
}

esp_err_t app_manager_switch_to(app_handle_t handle)
{
    app_slot_t *target = slot_by_handle(handle);
    if (target == NULL) {
        ESP_LOGE(TAG, "app_manager_switch_to: invalid handle %d", handle);
        return ESP_ERR_INVALID_ARG;
    }

    if (target->state == APP_STATE_RUNNING) {
        /* Already foreground */
        return ESP_OK;
    }

    if (target->state == APP_STATE_UNLOADED) {
        ESP_LOGE(TAG, "app_manager_switch_to: app %d is unloaded, use launch", handle);
        return ESP_ERR_INVALID_STATE;
    }

    /* Pause current foreground */
    pause_foreground();

    /* Resume target */
    ESP_LOGI(TAG, "Resuming app '%s' (handle %d)", target->entry->manifest->id, handle);
    target->state        = APP_STATE_RUNNING;
    target->last_used_ms = kernel_uptime_ms();
    if (target->entry->on_resume) {
        target->entry->on_resume();
    }

    event_t ev = {
        .type     = EVENT_APP_SWITCHED,
        .data     = (void *)(uintptr_t)handle,
        .data_len = 0,
    };
    event_publish(&ev);

    return ESP_OK;
}

app_handle_t app_manager_get_foreground(void)
{
    app_slot_t *fg = foreground_slot();
    return (fg != NULL) ? fg->handle : APP_HANDLE_INVALID;
}

app_state_t app_manager_get_state(app_handle_t handle)
{
    app_slot_t *slot = slot_by_handle(handle);
    if (slot == NULL) {
        return APP_STATE_UNLOADED;
    }
    return slot->state;
}

esp_err_t app_manager_suspend(app_handle_t handle)
{
    app_slot_t *slot = slot_by_handle(handle);
    if (slot == NULL) {
        ESP_LOGE(TAG, "app_manager_suspend: invalid handle %d", handle);
        return ESP_ERR_INVALID_ARG;
    }
    if (slot->state == APP_STATE_UNLOADED || slot->state == APP_STATE_SUSPENDED) {
        return ESP_OK;
    }

    ESP_LOGI(TAG, "Suspending app '%s'", slot->entry->manifest->id);
    if (slot->entry->on_pause) {
        slot->entry->on_pause();
    }
    slot->state = APP_STATE_SUSPENDED;
    return ESP_OK;
}

esp_err_t app_manager_kill(app_handle_t handle)
{
    app_slot_t *slot = slot_by_handle(handle);
    if (slot == NULL) {
        ESP_LOGE(TAG, "app_manager_kill: invalid handle %d", handle);
        return ESP_ERR_INVALID_ARG;
    }
    if (slot->state == APP_STATE_UNLOADED) {
        return ESP_OK;
    }

    ESP_LOGI(TAG, "Killing app '%s'", slot->entry->manifest->id);
    if (slot->entry->on_destroy) {
        slot->entry->on_destroy();
    }
    slot->state = APP_STATE_UNLOADED;

    event_t ev = {
        .type     = EVENT_APP_STOPPED,
        .data     = (void *)slot->entry->manifest->id,
        .data_len = 0,
    };
    event_publish(&ev);

    return ESP_OK;
}

size_t app_manager_get_free_memory(void)
{
    return heap_caps_get_free_size(MALLOC_CAP_DEFAULT);
}

esp_err_t app_manager_evict_lru(void)
{
    evict_lru_app();
    return ESP_OK;
}
