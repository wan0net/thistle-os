/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Reader app lifecycle
 */
#include "reader/reader_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "reader";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int reader_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    reader_ui_create(ui_manager_get_app_area());
    return 0;
}

static void reader_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Reader");
    reader_ui_show();
}

static void reader_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    reader_ui_hide();
}

static void reader_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Reader");
    reader_ui_show();
}

static void reader_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    /* reader_ui_cleanup() is called implicitly via LVGL object deletion
     * when the parent app area is cleaned up by the app manager. */
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t reader_manifest = {
    .id               = "com.thistle.reader",
    .name             = "Reader",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t reader_entry = {
    .manifest   = &reader_manifest,
    .on_create  = reader_on_create,
    .on_start   = reader_on_start,
    .on_pause   = reader_on_pause,
    .on_resume  = reader_on_resume,
    .on_destroy = reader_on_destroy,
};

esp_err_t reader_app_register(void)
{
    return app_manager_register(&reader_entry);
}
