// SPDX-License-Identifier: BSD-3-Clause
// Flashlight — first app using the widget API (no LVGL dependency)
//
// Two modes: Flashlight (white screen) and SOS (Morse code flash).
// Uses only thistle_ui_* and thistle_* syscalls.

#include "thistle_app.h"

#define TAG "flashlight"

// State
static thistle_widget_t s_root = THISTLE_WIDGET_NONE;
static thistle_widget_t s_flash_btn = THISTLE_WIDGET_NONE;
static thistle_widget_t s_sos_btn = THISTLE_WIDGET_NONE;
static thistle_widget_t s_hint = THISTLE_WIDGET_NONE;
static int s_mode = 0;  // 0=off, 1=on, 2=sos

static void set_mode(int mode);

// ── Event callbacks ─────────────────────────────────────────────────

static void flash_clicked(thistle_widget_t w, int event, void *ud)
{
    (void)w; (void)event; (void)ud;
    if (s_mode == 1) set_mode(0);
    else set_mode(1);
}

static void sos_clicked(thistle_widget_t w, int event, void *ud)
{
    (void)w; (void)event; (void)ud;
    if (s_mode == 2) set_mode(0);
    else set_mode(2);
}

// ── Mode management ─────────────────────────────────────────────────

static void set_mode(int mode)
{
    s_mode = mode;

    uint32_t bg = thistle_ui_theme_bg();
    uint32_t text = thistle_ui_theme_text();

    switch (mode) {
        case 1: // Flashlight ON — white screen
            thistle_ui_set_bg_color(s_root, 0xFFFFFF);
            thistle_ui_set_text(s_flash_btn, "OFF");
            thistle_ui_set_text(s_sos_btn, "SOS");
            thistle_ui_set_text_color(s_hint, 0x000000);
            thistle_ui_set_text(s_hint, "Tap OFF to turn off");
            break;

        case 2: // SOS — TODO: implement morse timer via kernel
            thistle_ui_set_bg_color(s_root, bg);
            thistle_ui_set_text(s_flash_btn, "FLASHLIGHT");
            thistle_ui_set_text(s_sos_btn, "STOP SOS");
            thistle_ui_set_text_color(s_hint, text);
            thistle_ui_set_text(s_hint, "SOS mode active");
            thistle_log(TAG, "SOS mode started");
            break;

        case 0: // Off
        default:
            thistle_ui_set_bg_color(s_root, bg);
            thistle_ui_set_text(s_flash_btn, "FLASHLIGHT");
            thistle_ui_set_text(s_sos_btn, "SOS");
            thistle_ui_set_text_color(s_hint, thistle_ui_theme_text_secondary());
            thistle_ui_set_text(s_hint, "Screen turns white when active");
            break;
    }
}

// ── App lifecycle ───────────────────────────────────────────────────

static int flashlight_on_create(void)
{
    thistle_log(TAG, "on_create");

    s_root = thistle_ui_get_app_root();

    // Make root a centered column layout
    thistle_widget_t container = thistle_ui_create_container(s_root);
    thistle_ui_set_size(container, -1, -1); // THISTLE_SIZE_FULL
    thistle_ui_set_layout(container, THISTLE_LAYOUT_FLEX_COLUMN);
    thistle_ui_set_align(container, 1, 1); // center, center
    thistle_ui_set_gap(container, 20);
    thistle_ui_set_bg_color(container, thistle_ui_theme_bg());

    // Save root reference for color changes
    s_root = container;

    // FLASHLIGHT button
    s_flash_btn = thistle_ui_create_button(container, "FLASHLIGHT");
    thistle_ui_set_size(s_flash_btn, 200, 52);
    thistle_ui_set_bg_color(s_flash_btn, thistle_ui_theme_primary());
    thistle_ui_set_text_color(s_flash_btn, 0xFFFFFF);
    thistle_ui_set_font_size(s_flash_btn, 18);
    thistle_ui_set_radius(s_flash_btn, 8);
    thistle_ui_on_event(s_flash_btn, THISTLE_EVENT_CLICK, flash_clicked, 0);

    // SOS button
    s_sos_btn = thistle_ui_create_button(container, "SOS");
    thistle_ui_set_size(s_sos_btn, 200, 44);
    thistle_ui_set_bg_color(s_sos_btn, thistle_ui_theme_surface());
    thistle_ui_set_text_color(s_sos_btn, thistle_ui_theme_text());
    thistle_ui_set_font_size(s_sos_btn, 18);
    thistle_ui_set_radius(s_sos_btn, 8);
    thistle_ui_set_border_width(s_sos_btn, 1);
    thistle_ui_on_event(s_sos_btn, THISTLE_EVENT_CLICK, sos_clicked, 0);

    // Hint label
    s_hint = thistle_ui_create_label(container, "Screen turns white when active");
    thistle_ui_set_text_color(s_hint, thistle_ui_theme_text_secondary());
    thistle_ui_set_font_size(s_hint, 14);

    return 0;
}

static void flashlight_on_start(void)
{
    thistle_log(TAG, "on_start");
}

static void flashlight_on_pause(void)
{
    thistle_log(TAG, "on_pause");
    if (s_mode != 0) set_mode(0);
}

static void flashlight_on_resume(void)
{
    thistle_log(TAG, "on_resume");
}

static void flashlight_on_destroy(void)
{
    thistle_log(TAG, "on_destroy");
}

static const thistle_app_t flashlight_app = {
    .id               = "com.thistle.flashlight",
    .name             = "Flashlight",
    .version          = "2.0.0",
    .allow_background = false,
    .on_create        = flashlight_on_create,
    .on_start         = flashlight_on_start,
    .on_pause         = flashlight_on_pause,
    .on_resume        = flashlight_on_resume,
    .on_destroy       = flashlight_on_destroy,
};

THISTLE_APP(flashlight_app);
