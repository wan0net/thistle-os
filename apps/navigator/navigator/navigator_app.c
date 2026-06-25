/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Navigator app lifecycle
 */
#include "navigator/navigator_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "navigator";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int navigator_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    navigator_ui_create(ui_manager_get_app_area());
    return 0;
}

static void navigator_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Navigator");
    navigator_ui_show();
}

static void navigator_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    navigator_ui_hide();
}

static void navigator_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Navigator");
    navigator_ui_show();
}

static void navigator_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    navigator_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t navigator_manifest = {
    .id               = "com.thistle.navigator",
    .name             = "Navigator",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t navigator_entry = {
    .manifest   = &navigator_manifest,
    .on_create  = navigator_on_create,
    .on_start   = navigator_on_start,
    .on_pause   = navigator_on_pause,
    .on_resume  = navigator_on_resume,
    .on_destroy = navigator_on_destroy,
};

esp_err_t navigator_app_register(void)
{
    return app_manager_register(&navigator_entry);
}
