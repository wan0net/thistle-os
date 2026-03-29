/*
 * ThistleOS Simulator — SDL2 host application
 *
 * Runs the real ThistleOS UI in an SDL2 window for development/testing.
 * Pass --device <name> to simulate a specific hardware target.
 * Default device: tdeck (320x240, 2x scale).
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/time.h>
#include <pthread.h>

static pthread_mutex_t s_lvgl_mutex = PTHREAD_MUTEX_INITIALIZER;

/* Public accessor for drivers/tasks that need to touch LVGL */
void sim_lvgl_lock(void)   { pthread_mutex_lock(&s_lvgl_mutex); }
void sim_lvgl_unlock(void) { pthread_mutex_unlock(&s_lvgl_mutex); }

#include "lvgl.h"
#include "hal/board.h"
#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/display_server.h"
#include "ui/manager.h"
#include "ui/lvgl_wm.h"
#include "sim_input.h"
#include "sim_vfs.h"
#include "sim_assert.h"
#include "sim_scenario.h"
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

static bool s_headless = false;
static int  s_timeout_ms = 0;
static const char *s_assert_file = NULL;
static const char *s_scenario_file = NULL;

bool sim_is_headless(void) { return s_headless; }

/* Defined in board_simulator.c */
extern void sim_board_set_device(const char *device);

int main(int argc, char **argv)
{
    const char *device = "tdeck";  /* default device */

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--device") == 0 && i + 1 < argc) {
            device = argv[++i];
        } else if (strncmp(argv[i], "--device=", 9) == 0) {
            device = argv[i] + 9;
        } else if (strcmp(argv[i], "--headless") == 0) {
            s_headless = true;
        } else if (strcmp(argv[i], "--timeout") == 0 && i + 1 < argc) {
            s_timeout_ms = atoi(argv[++i]);
        } else if (strncmp(argv[i], "--timeout=", 10) == 0) {
            s_timeout_ms = atoi(argv[i] + 10);
        } else if (strcmp(argv[i], "--assert") == 0 && i + 1 < argc) {
            s_assert_file = argv[++i];
        } else if (strncmp(argv[i], "--assert=", 9) == 0) {
            s_assert_file = argv[i] + 9;
        } else if (strcmp(argv[i], "--scenario") == 0 && i + 1 < argc) {
            s_scenario_file = argv[++i];
        } else if (strncmp(argv[i], "--scenario=", 11) == 0) {
            s_scenario_file = argv[i] + 11;
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            printf("Usage: %s [OPTIONS]\n", argv[0]);
            printf("Options:\n");
            printf("  --device NAME       Simulate a specific board (default: tdeck)\n");
            printf("  --headless          Run without SDL window (framebuffer only)\n");
            printf("  --timeout MS        Exit after MS milliseconds (headless mode)\n");
            printf("  --assert FILE       Evaluate assertions from FILE on exit\n");
            printf("  --scenario FILE     Replay input scenario from FILE (future)\n");
            printf("  -h, --help          Show this help\n");
            printf("Devices: tdeck-pro, tdeck, tdeck-plus, tdisplay, heltec-v3,\n");
            printf("         cardputer, cyd-s022, cyd-s028, t3-s3, c3-mini\n");
            return 0;
        }
    }

    sim_board_set_device(device);
    printf("ThistleOS Simulator — %s\n", device);
    fflush(stdout);

    /* Set up simulated SD card filesystem (symlink to simulator/sdcard/) */
    sim_vfs_init();

    if (s_assert_file) {
        sim_assert_init(s_assert_file);
    }

    if (s_scenario_file) {
        sim_scenario_load(s_scenario_file);
    }

    /* Initialize kernel (board + drivers + event bus + IPC + syscalls + apps) */
    esp_err_t err = kernel_init();
    { char _msg[64]; snprintf(_msg, sizeof(_msg), "kernel_init: %d", err);
      printf("%s\n", _msg); sim_assert_check_line(_msg); }
    fflush(stdout);

    /* Initialize display server and register the LVGL window manager */
    err = display_server_init();
    { char _msg[64]; snprintf(_msg, sizeof(_msg), "display_server_init: %d", err);
      printf("%s\n", _msg); sim_assert_check_line(_msg); }
    fflush(stdout);

    err = display_server_register_wm(lvgl_lcd_wm_get());
    { char _msg[64]; snprintf(_msg, sizeof(_msg), "display_server_register_wm: %d", err);
      printf("%s\n", _msg); sim_assert_check_line(_msg); }
    fflush(stdout);

    /* Register built-in apps (always available) */
    launcher_app_register();
    settings_app_register();
    filemgr_app_register();
    reader_app_register();
    notes_app_register();
    flashlight_app_register();
    vault_app_register();
    appstore_app_register();
    terminal_app_register();
    assistant_app_register();
    weather_app_register();

    /* Conditional apps based on device capabilities */
    extern bool sim_board_has_radio(void);
    extern bool sim_board_has_gps(void);
    if (sim_board_has_radio()) {
        messenger_app_register();
        wifiscanner_app_register();
    }
    if (sim_board_has_gps()) {
        navigator_app_register();
    }
    app_manager_launch("com.thistle.launcher");
    printf("Launcher launched\n");
    sim_assert_check_line("Launcher launched");
    fflush(stdout);

    printf("ThistleOS Simulator ready. Close window to exit.\n");
    sim_assert_check_line("ThistleOS Simulator ready");
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
        if (!s_headless && !splash_dismissed && (now_ms - start_ms) > 2000) {
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
        pthread_mutex_lock(&s_lvgl_mutex);
        lv_timer_handler();
        pthread_mutex_unlock(&s_lvgl_mutex);

        /* Headless timeout */
        if (s_headless && s_timeout_ms > 0 && (now_ms - start_ms) > (uint32_t)s_timeout_ms) {
            printf("Simulator timeout reached (%d ms)\n", s_timeout_ms);
            int rc = s_assert_file ? sim_assert_evaluate() : 0;
            exit(rc);
        }

        usleep(5000);  /* ~200 fps cap */
    }

    return 0;
}
