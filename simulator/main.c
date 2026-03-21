/*
 * ThistleOS Simulator — SDL2 host application
 *
 * Runs the real ThistleOS UI in an SDL2 window for development/testing.
 * Display: 320x240 scaled 2x to 640x480 window.
 */
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/time.h>

#include "lvgl.h"
#include "hal/board.h"
#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/display_server.h"
#include "ui/manager.h"
#include "ui/lvgl_wm.h"
#include "sim_input.h"
#include "sim_vfs.h"
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

int main(int argc, char **argv)
{
    (void)argc;
    (void)argv;

    printf("ThistleOS Simulator starting...\n");
    fflush(stdout);

    /* Set up simulated SD card filesystem (symlink to simulator/sdcard/) */
    sim_vfs_init();

    /* Initialize kernel (board + drivers + event bus + IPC + syscalls + apps) */
    esp_err_t err = kernel_init();
    printf("kernel_init: %d\n", err);
    fflush(stdout);

    /* Initialize display server and register the LVGL window manager */
    err = display_server_init();
    printf("display_server_init: %d\n", err);
    fflush(stdout);

    err = display_server_register_wm(lvgl_wm_get());
    printf("display_server_register_wm: %d\n", err);
    fflush(stdout);

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
    app_manager_launch("com.thistle.launcher");
    printf("Launcher launched\n");
    fflush(stdout);

    printf("ThistleOS Simulator ready. Close window to exit.\n");
    fflush(stdout);

    /* Main loop — drive LVGL tick + timer handler + SDL event pump */
    uint32_t last_tick = 0;
    uint32_t start_ms = 0;
    bool splash_dismissed = false;
    while (1) {
        /* Update LVGL tick (esp_timer periodic is a no-op in sim) */
        struct timeval tv;
        gettimeofday(&tv, NULL);
        uint32_t now_ms = (uint32_t)(tv.tv_sec * 1000 + tv.tv_usec / 1000);
        if (last_tick == 0) last_tick = now_ms;
        uint32_t elapsed = now_ms - last_tick;
        if (start_ms == 0) start_ms = now_ms;
        if (elapsed > 0) {
            lv_tick_inc(elapsed);
            last_tick = now_ms;
        }

        /* Auto-dismiss splash after 2 seconds (esp_timer_start_once is a no-op) */
        if (!splash_dismissed && (now_ms - start_ms) > 2000) {
            /* Find and delete the splash overlay (topmost child of active screen) */
            lv_obj_t *scr = lv_display_get_screen_active(NULL);
            if (scr) {
                uint32_t cnt = lv_obj_get_child_count(scr);
                if (cnt > 0) {
                    lv_obj_t *top = lv_obj_get_child(scr, cnt - 1);
                    /* Splash is a full-screen white overlay — delete it */
                    if (top && lv_obj_get_width(top) >= 300) {
                        lv_obj_delete(top);
                        printf("Splash screen dismissed\n");
                        fflush(stdout);
                    }
                }
            }
            splash_dismissed = true;
        }

        /* Pump SDL events → HAL input events */
        sim_input_poll_sdl();

        /* Run LVGL timer handler (renders, processes animations) */
        lv_timer_handler();

        usleep(5000);  /* ~200 fps cap */
    }

    return 0;
}
