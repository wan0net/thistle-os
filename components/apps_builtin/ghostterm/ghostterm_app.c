/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — GhostTerm serial terminal lifecycle
 *
 * App ID  : com.thistle.ghostterm
 * Name    : GhostTerm
 */
#include "ghostterm/ghostterm_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "ghostterm";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int ghostterm_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    ghostterm_ui_create(ui_manager_get_app_area());
    return 0;
}

static void ghostterm_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("GhostTerm");
    ghostterm_ui_show();
}

static void ghostterm_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    ghostterm_uart_stop();
    ghostterm_ui_hide();
}

static void ghostterm_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("GhostTerm");
    ghostterm_uart_start();
    ghostterm_ui_show();
}

static void ghostterm_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    ghostterm_uart_stop();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t ghostterm_manifest = {
    .id               = "com.thistle.ghostterm",
    .name             = "GhostTerm",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t ghostterm_entry = {
    .manifest   = &ghostterm_manifest,
    .on_create  = ghostterm_on_create,
    .on_start   = ghostterm_on_start,
    .on_pause   = ghostterm_on_pause,
    .on_resume  = ghostterm_on_resume,
    .on_destroy = ghostterm_on_destroy,
};

esp_err_t ghostterm_app_register(void)
{
    return app_manager_register(&ghostterm_entry);
}
