#include <stdio.h>

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "thistle/kernel.h"
#include "ui/manager.h"

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

    ESP_LOGI(TAG, "ThistleOS ready");

    /* Enter the kernel main loop — does not return */
    kernel_run();
}
