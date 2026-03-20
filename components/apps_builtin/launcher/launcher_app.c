#include "launcher/launcher_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "launcher";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int launcher_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    /* Launcher runs on the root display object; parent is provided
     * by the kernel when it calls on_create.  For now the UI is
     * initialised with a NULL parent — launcher_ui_create handles it. */
    /* Use the UI manager's app content area as parent */
    extern lv_obj_t *ui_manager_get_app_area(void);
    launcher_ui_create(ui_manager_get_app_area());
    return 0;
}

static void launcher_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Launcher");
    launcher_ui_show();
}

static void launcher_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    launcher_ui_hide();
}

static void launcher_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Launcher");
    launcher_ui_show();
}

static void launcher_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t launcher_manifest = {
    .id               = "com.thistle.launcher",
    .name             = "Launcher",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t launcher_entry = {
    .manifest   = &launcher_manifest,
    .on_create  = launcher_on_create,
    .on_start   = launcher_on_start,
    .on_pause   = launcher_on_pause,
    .on_resume  = launcher_on_resume,
    .on_destroy = launcher_on_destroy,
};

esp_err_t launcher_app_register(void)
{
    return app_manager_register(&launcher_entry);
}
