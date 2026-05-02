#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_heap_caps.h"

#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/permissions.h"
#include "thistle/event.h"
#include "thistle/ota.h"
#include "thistle/display_server.h"
#include "thistle/elf_loader.h"
/* board_config.h not needed — WM selected by display capability */
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

extern void tk_wm_do_refresh(void);

static void tk_render_task(void *arg)
{
    (void)arg;
    ESP_LOGI(TAG, "tk_render_task: started");
    /* Wait a bit to let the system settle before first render */
    vTaskDelay(pdMS_TO_TICKS(500));
    /* Polling loop: tk_wm_render() and tk_wm_do_refresh() both internally
     * gate on dirty / REFRESH_NEEDED flags, so they're cheap no-ops when
     * nothing has changed. Hardware refresh only fires after a render
     * actually touched the framebuffer — keeping the e-paper static.
     *
     * Refresh is invoked at this shallow call depth (not from inside the
     * deep render chain) to avoid Xtensa CALL8 register-window overflow. */
    for (;;) {
        display_server_tick();
        tk_wm_do_refresh();
        vTaskDelay(pdMS_TO_TICKS(200));
    }
}

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

    /* Select WM variant based on display type.
     * E-paper: thistle-tk (pure Rust, embedded-graphics)
     * LCD: LVGL (existing C apps) */
    bool use_tk_wm = false;
    {
        const hal_registry_t *reg = hal_get_registry();
        if (reg && reg->display && reg->display->refresh) {
            ESP_LOGI(TAG, "E-paper display: using thistle-tk WM");
            ret = display_server_register_wm(thistle_tk_wm_get());
            use_tk_wm = true;
        } else {
            ret = display_server_register_wm(lvgl_lcd_wm_get());
        }
    }
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to register window manager: %s", esp_err_to_name(ret));
        return;
    }

    /* Subscribe to system events that warrant user-visible toasts */
    event_subscribe(EVENT_WIFI_CONNECTED,    system_event_toast, NULL);
    event_subscribe(EVENT_WIFI_DISCONNECTED, system_event_toast, NULL);
    event_subscribe(EVENT_SD_MOUNTED,        system_event_toast, NULL);
    event_subscribe(EVENT_SD_UNMOUNTED,      system_event_toast, NULL);
    event_subscribe(EVENT_BATTERY_LOW,       system_event_toast, NULL);

    if (!use_tk_wm) {
        /* Register LVGL-based built-in apps (they depend on LVGL and would
         * crash under the thistle-tk WM) */
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
    }

    /* Scan SPIFFS and SD card for standalone .app.elf files.
     * This function is #[no_mangle] in Rust (elf_loader.rs). */
    elf_app_scan_and_register();

    /* Grant full permissions to built-in apps */
    permissions_grant("com.thistle.tk_launcher", PERM_ALL);
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

    if (use_tk_wm) {
        /* Launch the thistle-tk native launcher */
        app_manager_launch("com.thistle.tk_launcher");

        /* Drive the display server for e-paper.
         * E-paper refresh is slow so we check every 100 ms; the WM
         * internally skips physical refresh when nothing has changed. */
        /* Use an internal DRAM stack — Xtensa register window underflow/overflow
         * during vTaskDelay context switches is unreliable with PSRAM-backed stacks
         * because the saved register windows may be read incorrectly on resume. */
        ESP_LOGI(TAG, "Free heap before render task: %lu bytes",
                 (unsigned long)esp_get_free_heap_size());
        BaseType_t task_ret = xTaskCreate(tk_render_task, "tk_render",
                                           16384, NULL, 5, NULL);
        if (task_ret != pdPASS) {
            ESP_LOGE(TAG, "tk_render: xTaskCreate failed (%d)", task_ret);
        }
    } else {
        app_manager_launch("com.thistle.launcher");

        /* Start LVGL render loop AFTER all UI objects are created.
         * This prevents race conditions with e-paper's slow flush. */
        ui_manager_start();
    }

    /* Check for SD card firmware update */
    if (ota_sd_update_available()) {
        ESP_LOGI(TAG, "Firmware update detected on SD card!");
        toast_show("Update available! Settings > About to install", TOAST_INFO, 5000);
    }

    ESP_LOGI(TAG, "ThistleOS ready");

    /* Enter the kernel main loop — does not return */
    kernel_run();
}
