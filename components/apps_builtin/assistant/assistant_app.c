/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — AI Assistant app lifecycle
 */
#include "assistant/assistant_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "assistant";

/* ------------------------------------------------------------------ */
/* Forward declarations (assistant_ui.c provides these)                */
/* ------------------------------------------------------------------ */

extern esp_err_t assistant_ui_save_conversation(void);

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int assistant_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    assistant_ui_create(ui_manager_get_app_area());
    return 0;
}

static void assistant_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Assistant");
    assistant_ui_show();
}

static void assistant_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    /* Persist conversation before hiding */
    assistant_ui_save_conversation();
    assistant_ui_hide();
}

static void assistant_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Assistant");
    assistant_ui_show();
}

static void assistant_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    assistant_ui_save_conversation();
    assistant_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t assistant_manifest = {
    .id               = "com.thistle.assistant",
    .name             = "Assistant",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t assistant_entry = {
    .manifest   = &assistant_manifest,
    .on_create  = assistant_on_create,
    .on_start   = assistant_on_start,
    .on_pause   = assistant_on_pause,
    .on_resume  = assistant_on_resume,
    .on_destroy = assistant_on_destroy,
};

esp_err_t assistant_app_register(void)
{
    return app_manager_register(&assistant_entry);
}
