#include <stdio.h>

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/permissions.h"
#include "thistle/event.h"
#include "thistle/ota.h"
#include "thistle/display_server.h"
#include "thistle/elf_loader.h"
#include "hal/board.h"
#include "ui/manager.h"
#include "ui/lvgl_wm.h"
#include "ui/toast.h"
#include "launcher/launcher_app.h"
#include "settings/settings_app.h"
#include "file_manager/filemgr_app.h"
#include "reader/reader_app.h"
#include "messenger/messenger_app.h"
#include "navigator/navigator_app.h"
#include "notes/notes_app.h"
#include "appstore/appstore_app.h"
#include "assistant/assistant_app.h"
#include "wifiscanner/wifiscanner_app.h"
#include "flashlight/flashlight_app.h"
#include "weather/weather_app.h"
#include "terminal/terminal_app.h"
#include "vault/vault_app.h"

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

static void system_event_toast(const event_t *event, void *user_data)
{
    (void)user_data;
    switch (event->type) {
        case EVENT_WIFI_CONNECTED:
            toast_show("WiFi connected", TOAST_SUCCESS, 3000);
            break;
        case EVENT_WIFI_DISCONNECTED:
            toast_info("WiFi disconnected");
            break;
        case EVENT_SD_MOUNTED:
            toast_info("SD card mounted");
            break;
        case EVENT_SD_UNMOUNTED:
            toast_warn("SD card removed");
            break;
        case EVENT_BATTERY_LOW:
            toast_warn("Battery low!");
            break;
        default:
            break;
    }
}

void app_main(void)
{
#ifdef CONFIG_THISTLE_RUN_TESTS
    run_tests();
    return;
#endif

    ESP_LOGI(TAG, "ThistleOS v0.1.0 starting...");

    /* Start kernel services: board init, driver manager, app manager, event bus, IPC */
    esp_err_t ret = kernel_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "kernel_init failed: %s", esp_err_to_name(ret));
        return;
    }

    /* Initialize display server and register the LVGL window manager */
    ret = display_server_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "display_server_init failed: %s", esp_err_to_name(ret));
        return;
    }

    /* Select WM variant based on display capabilities:
     * E-paper displays have a refresh() function for deferred panel commit;
     * LCD displays do not. */
    {
        const hal_registry_t *reg = hal_get_registry();
        if (reg && reg->display && reg->display->refresh) {
            ret = display_server_register_wm(lvgl_epaper_wm_get());
        } else {
            ret = display_server_register_wm(lvgl_lcd_wm_get());
        }
    }
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to register LVGL window manager: %s", esp_err_to_name(ret));
        return;
    }

    /* Subscribe to system events that warrant user-visible toasts */
    event_subscribe(EVENT_WIFI_CONNECTED,    system_event_toast, NULL);
    event_subscribe(EVENT_WIFI_DISCONNECTED, system_event_toast, NULL);
    event_subscribe(EVENT_SD_MOUNTED,        system_event_toast, NULL);
    event_subscribe(EVENT_SD_UNMOUNTED,      system_event_toast, NULL);
    event_subscribe(EVENT_BATTERY_LOW,       system_event_toast, NULL);

    /* Register and launch built-in apps */
    launcher_app_register();
    settings_app_register();
    filemgr_app_register();
    reader_app_register();
    messenger_app_register();
    navigator_app_register();
    notes_app_register();
    appstore_app_register();
    assistant_app_register();
    wifiscanner_app_register();
    flashlight_app_register();
    weather_app_register();
    terminal_app_register();
    vault_app_register();

    /* Scan SPIFFS and SD card for standalone .app.elf files.
     * This function is #[no_mangle] in Rust (elf_loader.rs). */
    elf_app_scan_and_register();

    /* Grant full permissions to built-in apps */
    permissions_grant("com.thistle.launcher",   PERM_ALL);
    permissions_grant("com.thistle.settings",   PERM_ALL);
    permissions_grant("com.thistle.filemgr",    PERM_ALL);
    permissions_grant("com.thistle.reader",     PERM_ALL);
    permissions_grant("com.thistle.messenger",  PERM_RADIO | PERM_IPC);
    permissions_grant("com.thistle.navigator",  PERM_GPS | PERM_STORAGE);
    permissions_grant("com.thistle.notes",      PERM_STORAGE);
    permissions_grant("com.thistle.appstore",   PERM_STORAGE | PERM_NETWORK);
    permissions_grant("com.thistle.assistant",   PERM_NETWORK | PERM_STORAGE);
    permissions_grant("com.thistle.wifiscanner", PERM_NETWORK);
    permissions_grant("com.thistle.flashlight",  PERM_SYSTEM);
    permissions_grant("com.thistle.weather",     PERM_GPS);
    permissions_grant("com.thistle.terminal",    PERM_ALL);
    permissions_grant("com.thistle.vault",       PERM_STORAGE | PERM_SYSTEM);

    app_manager_launch("com.thistle.launcher");

    /* Start LVGL render loop AFTER all UI objects are created.
     * This prevents race conditions with e-paper's slow flush. */
    ui_manager_start();

    /* Check for SD card firmware update */
    if (ota_sd_update_available()) {
        ESP_LOGI(TAG, "Firmware update detected on SD card!");
        toast_show("Update available! Settings > About to install", TOAST_INFO, 5000);
    }

    ESP_LOGI(TAG, "ThistleOS ready");

    /* Enter the kernel main loop — does not return */
    kernel_run();
}
