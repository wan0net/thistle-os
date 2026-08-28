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
#include "sim_display.h"
#include "sim_vfs.h"
#include "sim_assert.h"
#include "sim_scenario.h"
#include "sim_app_loader.h"
#include "launcher/launcher_app.h"
#include "settings/settings_app.h"
#if THISTLE_HAVE_FILEMGR
#include "file_manager/filemgr_app.h"
#endif
#if THISTLE_HAVE_READER
#include "reader/reader_app.h"
#endif
#include "messenger/messenger_app.h"
#if THISTLE_HAVE_NAVIGATOR
#include "navigator/navigator_app.h"
#endif
#if THISTLE_HAVE_NOTES
#include "notes/notes_app.h"
#endif
#if THISTLE_HAVE_APPSTORE
#include "appstore/appstore_app.h"
#endif
#include "assistant/assistant_app.h"
#if THISTLE_HAVE_WIFISCANNER
#include "wifiscanner/wifiscanner_app.h"
#endif
#if THISTLE_HAVE_FLASHLIGHT
#include "flashlight/flashlight_app.h"
#endif
#if THISTLE_HAVE_WEATHER
#include "weather/weather_app.h"
#endif
#include "terminal/terminal_app.h"
#if THISTLE_HAVE_VAULT
#include "vault/vault_app.h"
#endif

static bool s_headless = false;
static int  s_timeout_ms = 0;
static const char *s_assert_file = NULL;
static const char *s_scenario_file = NULL;
static const char *s_screenshot_file = NULL;
static const char *s_launch_app = NULL;
static int s_tap_x = -1;
static int s_tap_y = -1;

bool sim_is_headless(void) { return s_headless; }

static void sim_tk_input_cb(const hal_input_event_t *event, void *user_data)
{
    (void)user_data;
    const display_server_wm_t *wm = thistle_tk_wm_get();
    if (wm && wm->on_input) wm->on_input(event);
}

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
        } else if (strcmp(argv[i], "--screenshot") == 0 && i + 1 < argc) {
            s_screenshot_file = argv[++i];
        } else if (strncmp(argv[i], "--screenshot=", 13) == 0) {
            s_screenshot_file = argv[i] + 13;
        } else if (strcmp(argv[i], "--tap") == 0 && i + 1 < argc) {
            if (sscanf(argv[++i], "%d,%d", &s_tap_x, &s_tap_y) != 2) {
                fprintf(stderr, "Invalid --tap value; expected X,Y\n");
                return 2;
            }
        } else if (strcmp(argv[i], "--launch-app") == 0 && i + 1 < argc) {
            s_launch_app = argv[++i];
        } else if (strncmp(argv[i], "--launch-app=", 13) == 0) {
            s_launch_app = argv[i] + 13;
        } else if (strncmp(argv[i], "--tap=", 6) == 0) {
            if (sscanf(argv[i] + 6, "%d,%d", &s_tap_x, &s_tap_y) != 2) {
                fprintf(stderr, "Invalid --tap value; expected X,Y\n");
                return 2;
            }
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            printf("Usage: %s [OPTIONS]\n", argv[0]);
            printf("Options:\n");
            printf("  --device NAME       Simulate a specific board (default: tdeck)\n");
            printf("  --headless          Run without SDL window (framebuffer only)\n");
            printf("  --timeout MS        Exit after MS milliseconds (headless mode)\n");
            printf("  --assert FILE       Evaluate assertions from FILE on exit\n");
            printf("  --scenario FILE     Replay input scenario from FILE (future)\n");
            printf("  --screenshot FILE   Save native-resolution framebuffer as PPM on exit\n");
            printf("  --tap X,Y           Inject one native-coordinate tap after startup\n");
            printf("  --launch-app ID     Launch a registered app instead of the launcher\n");
            printf("  -h, --help          Show this help\n");
            printf("Devices: tdeck-pro, tdeck, tdeck-plus, tdisplay, heltec-v3,\n");
            printf("         cardputer, t3-s3, rak3312\n");
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

    extern bool sim_board_is_epaper(void);
    bool use_tk_wm = sim_board_is_epaper();
    err = display_server_register_wm(use_tk_wm ? thistle_tk_wm_get()
                                               : lvgl_lcd_wm_get());
    { char _msg[64]; snprintf(_msg, sizeof(_msg), "display_server_register_wm: %d", err);
      printf("%s\n", _msg); sim_assert_check_line(_msg); }
    fflush(stdout);

    if (use_tk_wm) {
        const hal_input_driver_t *input = sim_input_get();
        if (input && input->register_callback) {
            input->register_callback(sim_tk_input_cb, NULL);
        }
    }

    /* Signed host TAP generations are registered before legacy built-ins so
     * an installed app wins the idempotent duplicate-ID registration rule. */
    sim_app_loader_scan_and_register();

    /* Register built-in apps (always available) */
    launcher_app_register();
    settings_app_register();
#if THISTLE_HAVE_FILEMGR
    filemgr_app_register();
#endif
#if THISTLE_HAVE_READER
    reader_app_register();
#endif
#if THISTLE_HAVE_NOTES
    notes_app_register();
#endif
#if THISTLE_HAVE_FLASHLIGHT
    flashlight_app_register();
#endif
#if THISTLE_HAVE_VAULT
    vault_app_register();
#endif
#if THISTLE_HAVE_APPSTORE
    appstore_app_register();
#endif
    terminal_app_register();
    assistant_app_register();
#if THISTLE_HAVE_WEATHER
    weather_app_register();
#endif

    /* Conditional apps based on device capabilities */
    extern bool sim_board_has_radio(void);
    extern bool sim_board_has_gps(void);
    if (sim_board_has_radio()) {
        messenger_app_register();
#if THISTLE_HAVE_WIFISCANNER
        wifiscanner_app_register();
#endif
    }
#if THISTLE_HAVE_NAVIGATOR
    if (sim_board_has_gps()) {
        navigator_app_register();
    }
#endif
    const char *initial_app = s_launch_app
        ? s_launch_app
        : (use_tk_wm ? "com.thistle.tk_launcher" : "com.thistle.launcher");
    err = app_manager_launch(initial_app);
    if (err != ESP_OK) {
        fprintf(stderr, "Failed to launch app %s: %d\n", initial_app, err);
        return 1;
    }
    if (s_launch_app) {
        printf("App launched: %s\n", initial_app);
    } else {
        printf("Launcher launched\n");
        sim_assert_check_line("Launcher launched");
    }
    fflush(stdout);

    printf("ThistleOS Simulator ready. Close window to exit.\n");
    sim_assert_check_line("ThistleOS Simulator ready");
    fflush(stdout);

    /* Main loop — drive LVGL tick + timer handler + SDL event pump */
    uint32_t last_tick = 0;
    uint32_t start_ms = 0;
    int tap_phase = 0;
    while (1) {
        /* Update LVGL tick */
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

        /* Pump SDL events → HAL input events */
        sim_input_poll_sdl();

        if (use_tk_wm) {
            display_server_tick();
        } else {
            /* Run LVGL timer handler (renders, processes animations) */
            pthread_mutex_lock(&s_lvgl_mutex);
            lv_timer_handler();
            pthread_mutex_unlock(&s_lvgl_mutex);
        }

        uint32_t tap_down_ms = use_tk_wm ? 250u : 2250u;
        uint32_t tap_up_ms = tap_down_ms + 100u;
        if (tap_phase == 0 && s_tap_x >= 0 && s_tap_y >= 0 &&
            (now_ms - start_ms) > tap_down_ms &&
            sim_input_inject_touch((uint16_t)s_tap_x, (uint16_t)s_tap_y, true)) {
            tap_phase = 1;
        } else if (tap_phase == 1 && (now_ms - start_ms) > tap_up_ms &&
                   sim_input_inject_touch((uint16_t)s_tap_x,
                                          (uint16_t)s_tap_y, false)) {
            tap_phase = 2;
        }

        /* Headless timeout */
        if (s_headless && s_timeout_ms > 0 && (now_ms - start_ms) > (uint32_t)s_timeout_ms) {
            printf("Simulator timeout reached (%d ms)\n", s_timeout_ms);
            if (s_screenshot_file && !sim_display_save_ppm(s_screenshot_file)) {
                fprintf(stderr, "Failed to save simulator screenshot\n");
                exit(1);
            }
            int rc = s_assert_file ? sim_assert_evaluate() : 0;
            exit(rc);
        }

        usleep(5000);  /* ~200 fps cap */
    }

    return 0;
}
