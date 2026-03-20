/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — App Store lifecycle controller
 *
 * App ID  : com.thistle.appstore
 * Name    : App Store
 */
#include "appstore/appstore_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "appstore";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int appstore_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    appstore_ui_create(ui_manager_get_app_area());
    return 0;
}

static void appstore_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("App Store");
    appstore_ui_show();
}

static void appstore_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    appstore_ui_hide();
}

static void appstore_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("App Store");
    appstore_ui_show();
}

static void appstore_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    /* UI root is a child of the app area — will be cleaned up when
     * the app area is rebuilt.  Nothing extra to free here. */
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t appstore_manifest = {
    .id               = "com.thistle.appstore",
    .name             = "App Store",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t appstore_entry = {
    .manifest   = &appstore_manifest,
    .on_create  = appstore_on_create,
    .on_start   = appstore_on_start,
    .on_pause   = appstore_on_pause,
    .on_resume  = appstore_on_resume,
    .on_destroy = appstore_on_destroy,
};

esp_err_t appstore_app_register(void)
{
    return app_manager_register(&appstore_entry);
}
