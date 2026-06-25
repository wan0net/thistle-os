/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Flashlight/SOS app lifecycle
 *
 * App ID  : com.thistle.flashlight
 * Name    : Flashlight
 */
#include "flashlight/flashlight_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "flashlight";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int flashlight_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    flashlight_ui_create(ui_manager_get_app_area());
    return 0;
}

static void flashlight_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Flashlight");
    flashlight_ui_show();
}

static void flashlight_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    flashlight_ui_hide();
}

static void flashlight_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Flashlight");
    flashlight_ui_show();
}

static void flashlight_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    flashlight_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t flashlight_manifest = {
    .id               = "com.thistle.flashlight",
    .name             = "Flashlight",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t flashlight_entry = {
    .manifest   = &flashlight_manifest,
    .on_create  = flashlight_on_create,
    .on_start   = flashlight_on_start,
    .on_pause   = flashlight_on_pause,
    .on_resume  = flashlight_on_resume,
    .on_destroy = flashlight_on_destroy,
};

esp_err_t flashlight_app_register(void)
{
    return app_manager_register(&flashlight_entry);
}
