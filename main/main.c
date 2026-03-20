#include <stdio.h>

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "ui/manager.h"
#include "launcher/launcher_app.h"
#include "settings/settings_app.h"
#include "file_manager/filemgr_app.h"

#ifdef CONFIG_THISTLE_RUN_TESTS
#include "unity.h"
static void run_tests(void)
{
    UNITY_BEGIN();
    unity_run_all_tests();
    UNITY_END();
}
#endif

static const char *TAG = "thistle";

void app_main(void)
{
#ifdef CONFIG_THISTLE_RUN_TESTS
    run_tests();
    return;
#endif

    ESP_LOGI(TAG, "ThistleOS v0.1.0 starting...");

    /* Start kernel services: board init, driver manager, app manager, event bus, IPC */
    ESP_ERROR_CHECK(kernel_init());

    /* Start LVGL and the ThistleOS window manager / UI */
    ESP_ERROR_CHECK(ui_manager_init());

    /* Register and launch built-in apps */
    launcher_app_register();
    settings_app_register();
    filemgr_app_register();
    app_manager_launch("com.thistle.launcher");

    ESP_LOGI(TAG, "ThistleOS ready");

    /* Enter the kernel main loop — does not return */
    kernel_run();
}
