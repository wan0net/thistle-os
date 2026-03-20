/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Flashlight/SOS UI
 *
 * Two modes:
 *   Flashlight — entire app area set to white, backlight raised to 100% on LCD.
 *   SOS        — cycles ... --- ... in Morse via LVGL timer, flashing screen white/dark.
 *
 * Layout (320x216 app area):
 *   ┌────────────────────────────────┐
 *   │                                │
 *   │        [FLASHLIGHT]            │  big primary button
 *   │                                │
 *   │           [SOS]                │  toggle button
 *   │                                │
 *   │     (screen turns white        │
 *   │      when active)              │
 *   │                                │
 *   └────────────────────────────────┘
 */
#include "flashlight/flashlight_app.h"

#include "ui/theme.h"
#include "thistle/app_manager.h"
#include "hal/board.h"

#include "lvgl.h"
#include "esp_log.h"

#include <string.h>
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "flashlight_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W  320
#define APP_AREA_H  216

/* ------------------------------------------------------------------ */
/* SOS Morse pattern                                                    */
/* Encoded as durations (ms on, ms off) pairs. A zero on-time           */
/* entry marks end of sequence; the timer restarts from the beginning.  */
/* ------------------------------------------------------------------ */

/* S = ...  O = ---  S = ...  then word gap */
#define SHORT_ON   200
#define SHORT_OFF  200
#define LONG_ON    600
#define LONG_OFF   200
#define LETTER_GAP 600
#define WORD_GAP   1400

typedef struct {
    uint32_t on_ms;   /* 0 = sequence end (pause before restart) */
    uint32_t off_ms;
} morse_step_t;

static const morse_step_t k_sos_pattern[] = {
    /* S */
    { SHORT_ON, SHORT_OFF },
    { SHORT_ON, SHORT_OFF },
    { SHORT_ON, LETTER_GAP },
    /* O */
    { LONG_ON, LONG_OFF },
    { LONG_ON, LONG_OFF },
    { LONG_ON, LETTER_GAP },
    /* S */
    { SHORT_ON, SHORT_OFF },
    { SHORT_ON, SHORT_OFF },
    { SHORT_ON, WORD_GAP },
    /* end marker — restarts sequence */
    { 0, 0 },
};

#define SOS_STEPS  (sizeof(k_sos_pattern) / sizeof(k_sos_pattern[0]))

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

typedef enum {
    FLASH_MODE_OFF,
    FLASH_MODE_ON,
    FLASH_MODE_SOS,
} flash_mode_t;

static struct {
    lv_obj_t      *root;
    lv_obj_t      *flash_btn_lbl;
    lv_obj_t      *sos_btn_lbl;

    flash_mode_t   mode;

    /* SOS timer state */
    lv_timer_t    *sos_timer;
    uint32_t       sos_step;      /* current index into k_sos_pattern */
    bool           sos_screen_on; /* true while screen is white (flash phase) */
} s_fl;

/* ------------------------------------------------------------------ */
/* Screen colour helpers                                                */
/* ------------------------------------------------------------------ */

static void set_screen_white(void)
{
    if (!s_fl.root) return;
    lv_obj_set_style_bg_color(s_fl.root, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fl.root,   LV_OPA_COVER,     LV_PART_MAIN);
}

static void set_screen_normal(void)
{
    if (!s_fl.root) return;
    const theme_colors_t *clr = theme_get_colors();
    lv_obj_set_style_bg_color(s_fl.root, clr->bg,       LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fl.root,   LV_OPA_COVER,  LV_PART_MAIN);
}

/* Raise or restore LCD backlight if display driver supports it */
static void set_backlight(uint8_t pct)
{
    const hal_registry_t *hal = hal_get_registry();
    if (hal && hal->display && hal->display->set_brightness) {
        hal->display->set_brightness(pct);
    }
}

/* ------------------------------------------------------------------ */
/* SOS timer                                                            */
/* ------------------------------------------------------------------ */

static void sos_timer_cb(lv_timer_t *timer)
{
    (void)timer;

    const morse_step_t *step = &k_sos_pattern[s_fl.sos_step];

    if (step->on_ms == 0) {
        /* End-of-sequence marker: restart */
        s_fl.sos_step = 0;
        s_fl.sos_screen_on = false;
        set_screen_normal();
        lv_timer_set_period(s_fl.sos_timer, k_sos_pattern[0].on_ms);
        return;
    }

    if (!s_fl.sos_screen_on) {
        /* Start flash-on phase */
        set_screen_white();
        s_fl.sos_screen_on = true;
        lv_timer_set_period(s_fl.sos_timer, step->on_ms);
    } else {
        /* End flash-on phase, start off phase */
        set_screen_normal();
        s_fl.sos_screen_on = false;
        lv_timer_set_period(s_fl.sos_timer, step->off_ms);
        /* Advance to next step on the following tick */
        s_fl.sos_step++;
        if (s_fl.sos_step >= SOS_STEPS) {
            s_fl.sos_step = 0;
        }
    }
}

static void sos_start(void)
{
    s_fl.sos_step      = 0;
    s_fl.sos_screen_on = false;

    if (s_fl.sos_timer == NULL) {
        s_fl.sos_timer = lv_timer_create(sos_timer_cb,
                                         k_sos_pattern[0].on_ms,
                                         NULL);
    } else {
        lv_timer_set_period(s_fl.sos_timer, k_sos_pattern[0].on_ms);
        lv_timer_resume(s_fl.sos_timer);
    }
}

static void sos_stop(void)
{
    if (s_fl.sos_timer) {
        lv_timer_pause(s_fl.sos_timer);
    }
    s_fl.sos_screen_on = false;
    set_screen_normal();
}

/* ------------------------------------------------------------------ */
/* Mode management                                                      */
/* ------------------------------------------------------------------ */

static void apply_mode(flash_mode_t new_mode)
{
    /* Tear down old mode */
    if (s_fl.mode == FLASH_MODE_SOS) {
        sos_stop();
    }
    if (s_fl.mode == FLASH_MODE_ON) {
        set_screen_normal();
        set_backlight(50); /* restore default brightness */
    }

    s_fl.mode = new_mode;

    switch (new_mode) {
        case FLASH_MODE_ON:
            set_screen_white();
            set_backlight(100);
            if (s_fl.flash_btn_lbl) lv_label_set_text(s_fl.flash_btn_lbl, "OFF");
            if (s_fl.sos_btn_lbl)   lv_label_set_text(s_fl.sos_btn_lbl,   "SOS");
            break;

        case FLASH_MODE_SOS:
            sos_start();
            if (s_fl.flash_btn_lbl) lv_label_set_text(s_fl.flash_btn_lbl, "FLASHLIGHT");
            if (s_fl.sos_btn_lbl)   lv_label_set_text(s_fl.sos_btn_lbl,   "STOP SOS");
            break;

        case FLASH_MODE_OFF:
        default:
            if (s_fl.flash_btn_lbl) lv_label_set_text(s_fl.flash_btn_lbl, "FLASHLIGHT");
            if (s_fl.sos_btn_lbl)   lv_label_set_text(s_fl.sos_btn_lbl,   "SOS");
            break;
    }
}

/* ------------------------------------------------------------------ */
/* Button callbacks                                                     */
/* ------------------------------------------------------------------ */

static void flash_btn_cb(lv_event_t *e)
{
    (void)e;
    if (s_fl.mode == FLASH_MODE_ON) {
        apply_mode(FLASH_MODE_OFF);
    } else {
        apply_mode(FLASH_MODE_ON);
    }
}

static void sos_btn_cb(lv_event_t *e)
{
    (void)e;
    if (s_fl.mode == FLASH_MODE_SOS) {
        apply_mode(FLASH_MODE_OFF);
    } else {
        apply_mode(FLASH_MODE_SOS);
    }
}

/* ------------------------------------------------------------------ */
/* UI construction                                                      */
/* ------------------------------------------------------------------ */

esp_err_t flashlight_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating Flashlight UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_fl, 0, sizeof(s_fl));

    const theme_colors_t *clr = theme_get_colors();

    /* Root container */
    s_fl.root = lv_obj_create(parent);
    lv_obj_set_size(s_fl.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_fl.root, 0, 0);
    lv_obj_set_style_bg_color(s_fl.root, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fl.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_fl.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_fl.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_fl.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_fl.root, LV_OBJ_FLAG_SCROLLABLE);

    /* Centre column layout */
    lv_obj_set_flex_flow(s_fl.root, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_fl.root,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(s_fl.root, 20, LV_PART_MAIN);

    /* ----- FLASHLIGHT button ----- */
    lv_obj_t *flash_btn = lv_button_create(s_fl.root);
    lv_obj_set_size(flash_btn, 200, 52);
    lv_obj_set_style_bg_color(flash_btn, clr->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(flash_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(flash_btn, clr->radius, LV_PART_MAIN);
    lv_obj_set_style_border_width(flash_btn, 0, LV_PART_MAIN);
    lv_obj_add_event_cb(flash_btn, flash_btn_cb, LV_EVENT_CLICKED, NULL);

    s_fl.flash_btn_lbl = lv_label_create(flash_btn);
    lv_label_set_text(s_fl.flash_btn_lbl, "FLASHLIGHT");
    lv_obj_set_style_text_font(s_fl.flash_btn_lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_fl.flash_btn_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(s_fl.flash_btn_lbl);

    /* ----- SOS button ----- */
    lv_obj_t *sos_btn = lv_button_create(s_fl.root);
    lv_obj_set_size(sos_btn, 200, 44);
    lv_obj_set_style_bg_color(sos_btn, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(sos_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(sos_btn, clr->radius, LV_PART_MAIN);
    lv_obj_set_style_border_width(sos_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(sos_btn, clr->text_secondary, LV_PART_MAIN);
    lv_obj_add_event_cb(sos_btn, sos_btn_cb, LV_EVENT_CLICKED, NULL);

    s_fl.sos_btn_lbl = lv_label_create(sos_btn);
    lv_label_set_text(s_fl.sos_btn_lbl, "SOS");
    lv_obj_set_style_text_font(s_fl.sos_btn_lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_fl.sos_btn_lbl, clr->text, LV_PART_MAIN);
    lv_obj_center(s_fl.sos_btn_lbl);

    /* Hint label */
    lv_obj_t *hint = lv_label_create(s_fl.root);
    lv_label_set_text(hint, "Screen turns white when active");
    lv_obj_set_style_text_font(hint, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(hint, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_text_align(hint, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
    lv_obj_set_width(hint, LV_PCT(90));

    return ESP_OK;
}

void flashlight_ui_show(void)
{
    if (s_fl.root) {
        lv_obj_clear_flag(s_fl.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void flashlight_ui_hide(void)
{
    /* Turn off everything when app loses focus */
    if (s_fl.mode != FLASH_MODE_OFF) {
        apply_mode(FLASH_MODE_OFF);
    }
    if (s_fl.root) {
        lv_obj_add_flag(s_fl.root, LV_OBJ_FLAG_HIDDEN);
    }
}
