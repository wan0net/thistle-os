/*
 * ThistleOS WASM Simulator — browser entry point
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Emscripten compiles this to WASM. SDL2 is auto-shimmed to HTML5 Canvas.
 * The Rust kernel is linked as a static library (wasm32-unknown-emscripten).
 */

#include <stdio.h>
#include <string.h>
#include <emscripten.h>

#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/permissions.h"
#include "thistle/display_server.h"
#include "ui/lvgl_wm.h"

/* App registration headers */
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

#include "lvgl.h"
#include "hal/board.h"

/* Emscripten main loop callback — drives LVGL + input polling */
static void main_loop(void)
{
    /* Poll all registered HAL input drivers (mouse/keyboard events) */
    const hal_registry_t *reg = hal_get_registry();
    if (reg) {
        for (int i = 0; i < reg->input_count; i++) {
            if (reg->inputs[i] && reg->inputs[i]->poll) {
                reg->inputs[i]->poll();
            }
        }
    }

    /* LVGL tick + render */
    lv_tick_inc(16); /* ~60 FPS */
    lv_timer_handler();
}

/* Import from board_simulator.c */
extern void sim_board_set_device(const char *device);
extern bool sim_board_has_radio(void);
extern bool sim_board_has_gps(void);
extern bool sim_board_has_keyboard(void);

/* MeshChat Rust app */
extern int rs_meshchat_init(void);
extern int rs_meshchat_update(void);

/* Device selection — JS calls _wasm_set_device(idx) before main runs */
static const char *DEVICE_NAMES[] = {
    "tdeck","tdeck-pro","tdeck-plus","tdisplay","heltec-v3",
    "cardputer","cyd-s022","cyd-s028","t3-s3","c3-mini"
};
static const char *wasm_device_name = "tdeck";

EMSCRIPTEN_KEEPALIVE
void wasm_set_device(int idx) {
    if (idx >= 0 && idx < 10) {
        wasm_device_name = DEVICE_NAMES[idx];
    }
}

int main(void)
{
    /* Device defaults to tdeck. JS can call _wasm_set_device(idx)
     * before Module init to change it. */
    printf("ThistleOS WASM Simulator — %s\n", wasm_device_name);
    sim_board_set_device(wasm_device_name);

    /* Initialize Rust kernel */
    int ret = kernel_init();
    printf("kernel_init: %d\n", ret);

    /* Initialize display server */
    ret = display_server_init();
    printf("display_server_init: %d\n", ret);

    /* Register LVGL window manager */
    ret = display_server_register_wm(lvgl_lcd_wm_get());
    printf("display_server_register_wm: %d\n", ret);

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

    /* Conditional apps based on device capabilities */
    if (sim_board_has_radio()) {
        messenger_app_register();
        wifiscanner_app_register();
        printf("  + Messenger, WiFi Scanner (radio)\n");
    }
    if (sim_board_has_gps()) {
        navigator_app_register();
        printf("  + Navigator (GPS)\n");
    }
    assistant_app_register();  /* works on any device with network */
    weather_app_register();

    printf("%d apps registered for %s\n",
        9 + (sim_board_has_radio() ? 2 : 0) + (sim_board_has_gps() ? 1 : 0) + 2,
        device_buf);

    /* Grant permissions */
    permissions_grant("com.thistle.launcher",   0x7F);
    permissions_grant("com.thistle.settings",   0x7F);
    permissions_grant("com.thistle.filemgr",    0x7F);
    permissions_grant("com.thistle.reader",     0x7F);
    permissions_grant("com.thistle.messenger",  0x41);
    permissions_grant("com.thistle.navigator",  0x06);
    permissions_grant("com.thistle.notes",      0x04);
    permissions_grant("com.thistle.appstore",   0x0C);
    permissions_grant("com.thistle.assistant",  0x0C);
    permissions_grant("com.thistle.wifiscanner",0x08);
    permissions_grant("com.thistle.flashlight", 0x20);
    permissions_grant("com.thistle.weather",    0x02);
    permissions_grant("com.thistle.terminal",   0x7F);
    permissions_grant("com.thistle.vault",      0x24);

    /* Launch launcher */
    app_manager_launch("com.thistle.launcher");

    printf("ThistleOS WASM ready. Running main loop.\n");

    /* Emscripten main loop — renders at 60 FPS */
    emscripten_set_main_loop(main_loop, 0, 1);

    return 0;
}
