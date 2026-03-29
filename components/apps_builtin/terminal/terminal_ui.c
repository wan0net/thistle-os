/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Terminal UI
 *
 * Simple system console: scrollable output area + single-line command input.
 * Supports built-in commands: help, heap, uptime, apps, version, reboot,
 * clear, wifi status, ble status.
 *
 * Layout (320x216 app area):
 *   ┌────────────────────────────────┐
 *   │  Terminal                      │  30 px header
 *   ├────────────────────────────────┤
 *   │  > ThistleOS v0.1.0           │
 *   │  > Kernel ready                │  scrollable output (monospace)
 *   │  > ...                         │
 *   │                                │
 *   ├────────────────────────────────┤
 *   │  $ [command input           ]> │  28 px input bar
 *   └────────────────────────────────┘
 */
#include "terminal/terminal_app.h"

#include "ui/theme.h"
#include "thistle/kernel.h"
#include "thistle/app_manager.h"
#include "thistle/wifi_manager.h"
#include "thistle/ble_manager.h"
#include "hal/board.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_heap_caps.h"

#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "terminal_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

static int s_app_w = 240;
static int s_app_h = 296;
#define HEADER_H      30
#define INPUT_BAR_H   28
static int s_output_h = 238; /* s_app_h - HEADER_H - INPUT_BAR_H */

/* Max characters in the output textarea before we trim */
#define OUTPUT_MAX_CHARS  2048

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t  *root;
    lv_obj_t  *output_ta;   /* read-only monospace textarea */
    lv_obj_t  *input_ta;    /* one-line command input */
} s_term;

/* ------------------------------------------------------------------ */
/* Output helpers                                                       */
/* ------------------------------------------------------------------ */

static void term_print(const char *line)
{
    if (!s_term.output_ta) return;

    /* Trim if oversized */
    const char *cur = lv_textarea_get_text(s_term.output_ta);
    if (cur && strlen(cur) > OUTPUT_MAX_CHARS - 128) {
        /* Find first newline and drop everything before it */
        const char *nl = strchr(cur, '\n');
        if (nl) {
            lv_textarea_set_text(s_term.output_ta, nl + 1);
        } else {
            lv_textarea_set_text(s_term.output_ta, "");
        }
    }

    lv_textarea_add_text(s_term.output_ta, "> ");
    lv_textarea_add_text(s_term.output_ta, line);
    lv_textarea_add_char(s_term.output_ta, '\n');

    /* Scroll to end */
    lv_obj_scroll_to_y(s_term.output_ta,
                       lv_obj_get_scroll_bottom(s_term.output_ta),
                       LV_ANIM_OFF);
}

/* ------------------------------------------------------------------ */
/* Built-in command handlers                                            */
/* ------------------------------------------------------------------ */

static void cmd_help(void)
{
    term_print("Commands:");
    term_print("  help        - this list");
    term_print("  heap        - free heap / PSRAM");
    term_print("  uptime      - kernel uptime");
    term_print("  apps        - list registered apps");
    term_print("  version     - OS version");
    term_print("  reboot      - restart device");
    term_print("  clear       - clear terminal");
    term_print("  wifi status - WiFi state");
    term_print("  ble status  - BLE state");
}

static void cmd_heap(void)
{
    char buf[64];
    uint32_t free_heap = esp_get_free_heap_size();
    size_t   free_psram = heap_caps_get_free_size(MALLOC_CAP_SPIRAM);
    snprintf(buf, sizeof(buf), "Heap: %lu B free", (unsigned long)free_heap);
    term_print(buf);
    snprintf(buf, sizeof(buf), "PSRAM: %zu B free", free_psram);
    term_print(buf);
}

static void cmd_uptime(void)
{
    char buf[48];
    uint32_t ms  = kernel_uptime_ms();
    uint32_t sec = ms / 1000;
    uint32_t min = sec / 60;
    uint32_t hr  = min / 60;
    snprintf(buf, sizeof(buf), "Uptime: %luh %02lum %02lus",
             (unsigned long)hr,
             (unsigned long)(min % 60),
             (unsigned long)(sec % 60));
    term_print(buf);
}

static void cmd_apps(void)
{
    /* App manager doesn't expose an iterator in this kernel version —
     * list the known built-ins manually. */
    term_print("Registered apps:");
    term_print("  com.thistle.launcher");
    term_print("  com.thistle.settings");
    term_print("  com.thistle.filemgr");
    term_print("  com.thistle.reader");
    term_print("  com.thistle.messenger");
    term_print("  com.thistle.navigator");
    term_print("  com.thistle.notes");
    term_print("  com.thistle.appstore");
    term_print("  com.thistle.assistant");
    term_print("  com.thistle.wifiscanner");
    term_print("  com.thistle.flashlight");
    term_print("  com.thistle.weather");
    term_print("  com.thistle.terminal");
}

static void cmd_version(void)
{
    term_print("ThistleOS v" THISTLE_VERSION_STRING);
    const hal_registry_t *hal = hal_get_registry();
    if (hal && hal->board_name) {
        char buf[48];
        snprintf(buf, sizeof(buf), "Board: %s", hal->board_name);
        term_print(buf);
    }
}

static void cmd_wifi_status(void)
{
    wifi_state_t state = wifi_manager_get_state();
    const char *state_str;
    switch (state) {
        case WIFI_STATE_CONNECTED:    state_str = "connected";    break;
        case WIFI_STATE_CONNECTING:   state_str = "connecting";   break;
        case WIFI_STATE_FAILED:       state_str = "failed";       break;
        case WIFI_STATE_DISCONNECTED:
        default:                      state_str = "disconnected"; break;
    }
    char buf[64];
    snprintf(buf, sizeof(buf), "WiFi: %s", state_str);
    term_print(buf);

    if (state == WIFI_STATE_CONNECTED) {
        const char *ip = wifi_manager_get_ip();
        snprintf(buf, sizeof(buf), "IP: %s", ip ? ip : "unknown");
        term_print(buf);
        snprintf(buf, sizeof(buf), "RSSI: %d dBm", (int)wifi_manager_get_rssi());
        term_print(buf);
    }
}

static void cmd_ble_status(void)
{
    ble_state_t state = ble_manager_get_state();
    const char *state_str;
    switch (state) {
        case BLE_STATE_ADVERTISING: state_str = "advertising"; break;
        case BLE_STATE_CONNECTED:   state_str = "connected";   break;
        case BLE_STATE_OFF:
        default:                    state_str = "off";         break;
    }
    char buf[48];
    snprintf(buf, sizeof(buf), "BLE: %s", state_str);
    term_print(buf);

    if (state == BLE_STATE_CONNECTED) {
        const char *peer = ble_manager_get_peer_name();
        snprintf(buf, sizeof(buf), "Peer: %s", peer ? peer : "unknown");
        term_print(buf);
    }
}

/* ------------------------------------------------------------------ */
/* Command dispatch                                                     */
/* ------------------------------------------------------------------ */

static void dispatch_command(const char *raw)
{
    /* Skip leading whitespace */
    while (*raw == ' ') raw++;
    if (*raw == '\0') return;

    /* Echo the command */
    char echo[256];
    snprintf(echo, sizeof(echo), "$ %s", raw);
    term_print(echo);

    /* Simple if/else dispatch on first word */
    if (strcmp(raw, "help") == 0) {
        cmd_help();
    } else if (strcmp(raw, "heap") == 0) {
        cmd_heap();
    } else if (strcmp(raw, "uptime") == 0) {
        cmd_uptime();
    } else if (strcmp(raw, "apps") == 0) {
        cmd_apps();
    } else if (strcmp(raw, "version") == 0) {
        cmd_version();
    } else if (strcmp(raw, "reboot") == 0) {
        term_print("Rebooting...");
        esp_restart();
    } else if (strcmp(raw, "clear") == 0) {
        lv_textarea_set_text(s_term.output_ta, "");
    } else if (strcmp(raw, "wifi status") == 0) {
        cmd_wifi_status();
    } else if (strcmp(raw, "ble status") == 0) {
        cmd_ble_status();
    } else {
        char err[256];
        snprintf(err, sizeof(err), "Unknown command: %s", raw);
        term_print(err);
        term_print("Type 'help' for available commands.");
    }
}

/* ------------------------------------------------------------------ */
/* Input event callback                                                 */
/* ------------------------------------------------------------------ */

static void input_ta_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);

    if (code == LV_EVENT_KEY) {
        uint32_t key = lv_event_get_key(e);
        if (key == LV_KEY_ENTER) {
            const char *text = lv_textarea_get_text(s_term.input_ta);
            if (text && text[0] != '\0') {
                /* Copy before clearing */
                char cmd[128];
                strncpy(cmd, text, sizeof(cmd) - 1);
                cmd[sizeof(cmd) - 1] = '\0';

                lv_textarea_set_text(s_term.input_ta, "");
                dispatch_command(cmd);
            }
        }
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t terminal_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating Terminal UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_term, 0, sizeof(s_term));

    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;
    if (s_app_h == 0) s_app_h = 296;
    s_output_h = s_app_h - HEADER_H - INPUT_BAR_H;

    const theme_colors_t *clr = theme_get_colors();

    /* Root container */
    s_term.root = lv_obj_create(parent);
    lv_obj_set_size(s_term.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_term.root, 0, 0);
    lv_obj_set_style_bg_color(s_term.root, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_term.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_term.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_term.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_term.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_term.root, LV_OBJ_FLAG_SCROLLABLE);

    /* Header */
    lv_obj_t *hdr = lv_obj_create(s_term.root);
    lv_obj_set_size(hdr, s_app_w, HEADER_H);
    lv_obj_set_pos(hdr, 0, 0);
    lv_obj_set_style_bg_color(hdr, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(hdr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(hdr, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(hdr, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(hdr, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(hdr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(hdr, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(hdr, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(hdr, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);

    lv_obj_t *title = lv_label_create(hdr);
    lv_label_set_text(title, "Terminal");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, clr->text, LV_PART_MAIN);

    /* Output textarea (read-only, auto-scroll, monospace via montserrat_14) */
    s_term.output_ta = lv_textarea_create(s_term.root);
    lv_obj_set_pos(s_term.output_ta, 0, HEADER_H);
    lv_obj_set_size(s_term.output_ta, s_app_w, s_output_h);
    lv_textarea_set_one_line(s_term.output_ta, false);
    lv_textarea_set_cursor_click_pos(s_term.output_ta, false);
    lv_obj_clear_flag(s_term.output_ta, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(s_term.output_ta, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_term.output_ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_term.output_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_term.output_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_term.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(s_term.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_term.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_term.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_text_font(s_term.output_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_term.output_ta, clr->text, LV_PART_MAIN);
    lv_obj_set_scrollbar_mode(s_term.output_ta, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_term.output_ta, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_term.output_ta, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_term.output_ta, 2, LV_PART_SCROLLBAR);
    /* Hide cursor */
    lv_obj_set_style_bg_opa(s_term.output_ta, LV_OPA_TRANSP, LV_PART_CURSOR);

    /* Input bar */
    lv_obj_t *input_bar = lv_obj_create(s_term.root);
    lv_obj_set_pos(input_bar, 0, HEADER_H + s_output_h);
    lv_obj_set_size(input_bar, s_app_w, INPUT_BAR_H);
    lv_obj_set_style_bg_color(input_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(input_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(input_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(input_bar, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(input_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(input_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(input_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(input_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(input_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(input_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(input_bar, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(input_bar, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(input_bar, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(input_bar, 4, LV_PART_MAIN);

    /* "$" prompt label */
    lv_obj_t *prompt = lv_label_create(input_bar);
    lv_label_set_text(prompt, "$");
    lv_obj_set_style_text_font(prompt, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(prompt, clr->primary, LV_PART_MAIN);

    /* One-line input textarea */
    s_term.input_ta = lv_textarea_create(input_bar);
    lv_textarea_set_one_line(s_term.input_ta, true);
    lv_textarea_set_placeholder_text(s_term.input_ta, "command...");
    lv_obj_set_flex_grow(s_term.input_ta, 1);
    lv_obj_set_height(s_term.input_ta, INPUT_BAR_H - 4);
    lv_obj_set_style_bg_color(s_term.input_ta, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_term.input_ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_term.input_ta, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_term.input_ta, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(s_term.input_ta, 3, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_term.input_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(s_term.input_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_term.input_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_term.input_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_text_font(s_term.input_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_term.input_ta, clr->text, LV_PART_MAIN);
    lv_obj_add_event_cb(s_term.input_ta, input_ta_cb, LV_EVENT_KEY, NULL);

    /* ">" send indicator label */
    lv_obj_t *send_lbl = lv_label_create(input_bar);
    lv_label_set_text(send_lbl, ">");
    lv_obj_set_style_text_font(send_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(send_lbl, clr->primary, LV_PART_MAIN);

    /* Print startup banner */
    term_print("ThistleOS v" THISTLE_VERSION_STRING);
    term_print("Kernel ready");
    {
        char buf[48];
        const hal_registry_t *hal = hal_get_registry();
        if (hal && hal->board_name) {
            snprintf(buf, sizeof(buf), "Board: %s", hal->board_name);
            term_print(buf);
        }
        uint32_t free_heap = esp_get_free_heap_size();
        snprintf(buf, sizeof(buf), "Free heap: %lu B", (unsigned long)free_heap);
        term_print(buf);
    }
    term_print("Type 'help' for commands");

    return ESP_OK;
}

void terminal_ui_show(void)
{
    if (s_term.root) {
        lv_obj_clear_flag(s_term.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void terminal_ui_hide(void)
{
    if (s_term.root) {
        lv_obj_add_flag(s_term.root, LV_OBJ_FLAG_HIDDEN);
    }
}
