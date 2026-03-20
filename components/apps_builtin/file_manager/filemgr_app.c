#include "file_manager/filemgr_app.h"

#include "thistle/app_manager.h"
#include "esp_log.h"

static const char *TAG = "filemgr";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int filemgr_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    filemgr_ui_create(ui_manager_get_app_area());
    return 0;
}

static void filemgr_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    filemgr_ui_show();
}

static void filemgr_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    filemgr_ui_hide();
}

static void filemgr_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    filemgr_ui_show();
}

static void filemgr_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t filemgr_manifest = {
    .id               = "com.thistle.filemgr",
    .name             = "Files",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t filemgr_entry = {
    .manifest   = &filemgr_manifest,
    .on_create  = filemgr_on_create,
    .on_start   = filemgr_on_start,
    .on_pause   = filemgr_on_pause,
    .on_resume  = filemgr_on_resume,
    .on_destroy = filemgr_on_destroy,
};

esp_err_t filemgr_app_register(void)
{
    return app_manager_register(&filemgr_entry);
}
