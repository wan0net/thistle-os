/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — WiFi Scanner app lifecycle
 *
 * App ID  : com.thistle.wifiscanner
 * Name    : WiFi Scanner
 */
#include "wifiscanner/wifiscanner_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "wifiscanner";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int wifiscanner_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    wifiscanner_ui_create(ui_manager_get_app_area());
    return 0;
}

static void wifiscanner_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("WiFi Scanner");
    wifiscanner_ui_show();
}

static void wifiscanner_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    wifiscanner_ui_hide();
}

static void wifiscanner_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("WiFi Scanner");
    wifiscanner_ui_show();
}

static void wifiscanner_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    /* UI objects are cleaned up by LVGL when the app area parent is destroyed */
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t wifiscanner_manifest = {
    .id               = "com.thistle.wifiscanner",
    .name             = "WiFi Scanner",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t wifiscanner_entry = {
    .manifest   = &wifiscanner_manifest,
    .on_create  = wifiscanner_on_create,
    .on_start   = wifiscanner_on_start,
    .on_pause   = wifiscanner_on_pause,
    .on_resume  = wifiscanner_on_resume,
    .on_destroy = wifiscanner_on_destroy,
};

esp_err_t wifiscanner_app_register(void)
{
    return app_manager_register(&wifiscanner_entry);
}
