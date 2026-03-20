#include "settings/settings_app.h"

#include "thistle/app_manager.h"
#include "esp_log.h"

static const char *TAG = "settings";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int settings_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    settings_ui_create(NULL);
    return 0;
}

static void settings_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    settings_ui_show();
}

static void settings_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    settings_ui_hide();
}

static void settings_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    settings_ui_show();
}

static void settings_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t settings_manifest = {
    .id               = "com.thistle.settings",
    .name             = "Settings",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t settings_entry = {
    .manifest   = &settings_manifest,
    .on_create  = settings_on_create,
    .on_start   = settings_on_start,
    .on_pause   = settings_on_pause,
    .on_resume  = settings_on_resume,
    .on_destroy = settings_on_destroy,
};

esp_err_t settings_app_register(void)
{
    return app_manager_register(&settings_entry);
}
