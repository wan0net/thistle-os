/*
 * ThistleOS WASM Simulator — browser entry point
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Emscripten compiles this to WASM. SDL2 is auto-shimmed to HTML5 Canvas.
 * The Rust kernel is linked as a static library (wasm32-unknown-emscripten).
 */

#include <stdio.h>
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

int main(void)
{
    printf("ThistleOS WASM Simulator starting...\n");

    /* Initialize Rust kernel */
    int ret = kernel_init();
    printf("kernel_init: %d\n", ret);

    /* Initialize display server */
    ret = display_server_init();
    printf("display_server_init: %d\n", ret);

    /* Register LVGL window manager */
    ret = display_server_register_wm(lvgl_wm_get());
    printf("display_server_register_wm: %d\n", ret);

    /* Register built-in apps */
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
