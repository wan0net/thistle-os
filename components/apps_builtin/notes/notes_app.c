/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Notes app lifecycle
 */
#include "notes/notes_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "notes";

/* ------------------------------------------------------------------ */
/* Forward declarations (notes_ui.c provides these)                    */
/* ------------------------------------------------------------------ */

extern esp_err_t notes_ui_save_if_needed(void);

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int notes_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    notes_ui_create(ui_manager_get_app_area());
    return 0;
}

static void notes_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Notes");
    notes_ui_show();
}

static void notes_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    /* Auto-save any unsaved work before hiding */
    notes_ui_save_if_needed();
    notes_ui_hide();
}

static void notes_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Notes");
    notes_ui_show();
}

static void notes_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    notes_ui_save_if_needed();
    notes_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t notes_manifest = {
    .id               = "com.thistle.notes",
    .name             = "Notes",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t notes_entry = {
    .manifest   = &notes_manifest,
    .on_create  = notes_on_create,
    .on_start   = notes_on_start,
    .on_pause   = notes_on_pause,
    .on_resume  = notes_on_resume,
    .on_destroy = notes_on_destroy,
};

esp_err_t notes_app_register(void)
{
    return app_manager_register(&notes_entry);
}
