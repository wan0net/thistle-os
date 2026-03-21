// SPDX-License-Identifier: BSD-3-Clause
// Hello World — first ThistleOS standalone .app.elf
//
// Minimal app that proves the ELF loading pipeline.
// Uses only syscall table functions — no LVGL dependency.

#include "thistle_app.h"

#define TAG "hello"

static int hello_on_create(void)
{
    thistle_log(TAG, "Hello from a loadable app!");

    uint16_t w = thistle_display_get_width();
    uint16_t h = thistle_display_get_height();
    thistle_log(TAG, "Display: %dx%d");

    uint16_t batt = thistle_power_get_battery_mv();
    thistle_log(TAG, "Battery: %d mV");

    uint32_t uptime = thistle_millis();
    thistle_log(TAG, "Uptime: %d ms");

    return 0;
}

static void hello_on_start(void)
{
    thistle_log(TAG, "on_start — app is now foreground");
}

static void hello_on_pause(void)
{
    thistle_log(TAG, "on_pause — app going to background");
}

static void hello_on_resume(void)
{
    thistle_log(TAG, "on_resume — app back to foreground");
}

static void hello_on_destroy(void)
{
    thistle_log(TAG, "on_destroy — goodbye!");
}

static const thistle_app_t hello_app = {
    .id               = "com.example.hello",
    .name             = "Hello World",
    .version          = "1.0.0",
    .allow_background = false,
    .on_create        = hello_on_create,
    .on_start         = hello_on_start,
    .on_pause         = hello_on_pause,
    .on_resume        = hello_on_resume,
    .on_destroy       = hello_on_destroy,
};

THISTLE_APP(hello_app);
