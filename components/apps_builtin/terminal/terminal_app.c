/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Terminal app lifecycle
 *
 * App ID  : com.thistle.terminal
 * Name    : Terminal
 */
#include "terminal/terminal_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "terminal";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int terminal_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    terminal_ui_create(ui_manager_get_app_area());
    return 0;
}

static void terminal_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Terminal");
    terminal_ui_show();
}

static void terminal_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    terminal_uart_stop();
    terminal_ui_hide();
}

static void terminal_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Terminal");
    terminal_uart_start();
    terminal_ui_show();
}

static void terminal_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    terminal_uart_stop();
    terminal_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t terminal_manifest = {
    .id               = "com.thistle.terminal",
    .name             = "Terminal",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t terminal_entry = {
    .manifest   = &terminal_manifest,
    .on_create  = terminal_on_create,
    .on_start   = terminal_on_start,
    .on_pause   = terminal_on_pause,
    .on_resume  = terminal_on_resume,
    .on_destroy = terminal_on_destroy,
};

esp_err_t terminal_app_register(void)
{
    return app_manager_register(&terminal_entry);
}
