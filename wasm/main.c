/*
 * ThistleOS WASM Simulator — browser entry point
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Mirrors simulator/main.c but uses Emscripten's main loop instead of
 * a blocking while(1). SDL2 is auto-shimmed to HTML5 Canvas.
 */

#include <stdio.h>
#include <string.h>
#include <emscripten.h>

#include "lvgl.h"
#include "hal/board.h"
#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/display_server.h"
#include "ui/lvgl_wm.h"
#include "sim_input.h"
#include "sim_vfs.h"

/* App registration headers — must match simulator/main.c */
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

/* Defined in board_simulator.c (shared with SDL sim) */
extern void sim_board_set_device(const char *device);
extern bool sim_board_has_radio(void);
extern bool sim_board_has_gps(void);

/* ------------------------------------------------------------------ */
/* Device selection — JS calls _wasm_set_device(idx) before main      */
/* ------------------------------------------------------------------ */

static const char *DEVICE_NAMES[] = {
    "tdeck", "tdeck-pro", "tdeck-plus", "tdisplay",
    "heltec-v3", "cardputer", "t3-s3", "rak3312"
};
#define NUM_DEVICES 8
static const char *wasm_device_name = "tdeck";

EMSCRIPTEN_KEEPALIVE
void wasm_set_device(int idx) {
    if (idx >= 0 && idx < NUM_DEVICES) {
        wasm_device_name = DEVICE_NAMES[idx];
    }
}

/* ------------------------------------------------------------------ */
/* Main loop callback — drives LVGL at ~60 FPS                        */
/* ------------------------------------------------------------------ */

static void main_loop(void)
{
    /* Poll HAL input drivers (SDL mouse/keyboard → HAL events) */
    sim_input_poll_sdl();

    /* LVGL tick + render */
    lv_tick_inc(16);  /* ~60 FPS */
    lv_timer_handler();
}

/* ------------------------------------------------------------------ */
/* Entry point                                                         */
/* ------------------------------------------------------------------ */

int main(void)
{
    printf("ThistleOS WASM Simulator — %s\n", wasm_device_name);
    sim_board_set_device(wasm_device_name);

    /* Set up simulated SD card filesystem */
    sim_vfs_init();

    /* Initialize kernel (board + drivers + event bus + IPC + syscalls) */
    int ret = kernel_init();
    printf("kernel_init: %d\n", ret);

    /* Initialize display server and register LVGL window manager */
    ret = display_server_init();
    printf("display_server_init: %d\n", ret);

    ret = display_server_register_wm(lvgl_lcd_wm_get());
    printf("display_server_register_wm: %d\n", ret);

    /* Register built-in apps — same order as simulator/main.c */
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
    if (sim_board_has_radio()) {
        messenger_app_register();
        wifiscanner_app_register();
    }
    if (sim_board_has_gps()) {
        navigator_app_register();
    }

    /* Launch launcher */
    app_manager_launch("com.thistle.launcher");

    printf("ThistleOS WASM ready. Running main loop.\n");

    /* Emscripten main loop — renders at 60 FPS via requestAnimationFrame */
    emscripten_set_main_loop(main_loop, 0, 1);

    return 0;
}
