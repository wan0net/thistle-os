/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — WiFi Scanner UI
 *
 * List screen: scrollable network list + channel utilisation bar.
 * Detail screen: per-network info with signal bar and connect button.
 *
 * Layout (320x216 app area):
 *   ┌─────────────────────────────┐
 *   │  WiFi Scanner  [Scan] N APs │  30 px header
 *   ├─────────────────────────────┤
 *   │  SSID          RSSI Ch Auth │  scrollable list rows (30 px each)
 *   │  ─────────────────────────  │
 *   │  ...                        │
 *   ├─────────────────────────────┤
 *   │  Ch: 1- 2  3. 6# 11-  ...  │  channel bar (20 px)
 *   └─────────────────────────────┘
 */
#include "wifiscanner/wifiscanner_app.h"

#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"
#include "thistle/wifi_manager.h"

#include "lvgl.h"
#include "esp_log.h"

#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "wifiscanner_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      240
#define APP_AREA_H      296
#define HEADER_H         30
#define ITEM_H           28
#define CHANNEL_BAR_H    20
#define LIST_H          (APP_AREA_H - HEADER_H - CHANNEL_BAR_H)

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

#define MAX_SCAN_RESULTS 30

typedef struct {
    char    ssid[33];
    uint8_t bssid[6];       /* all zeros == unavailable */
    int8_t  rssi;
    uint8_t channel;
    char    auth_str[8];    /* "Open", "WEP", "WPA", "WPA2", "WPA3" */
    bool    is_5ghz;
} scan_entry_t;

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t  *root;

    /* List screen */
    lv_obj_t  *list_screen;
    lv_obj_t  *count_label;
    lv_obj_t  *scan_btn_lbl;
    lv_obj_t  *network_list;
    lv_obj_t  *channel_bar;

    /* Detail screen */
    lv_obj_t  *detail_screen;
    lv_obj_t  *detail_ssid_lbl;
    lv_obj_t  *detail_bssid_lbl;
    lv_obj_t  *detail_channel_lbl;
    lv_obj_t  *detail_rssi_lbl;
    lv_obj_t  *detail_auth_lbl;
    lv_obj_t  *detail_signal_bar_lbl;

    /* Scan state */
    scan_entry_t results[MAX_SCAN_RESULTS];
    int          result_count;
    int          selected_idx;
    bool         scanning;
} s_scanner;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void switch_to_list(void);
static void switch_to_detail(int idx);
static void do_scan(void);
static void populate_network_list(void);
static void update_channel_bars(void);

/* ------------------------------------------------------------------ */
/* Signal quality helpers                                               */
/* ------------------------------------------------------------------ */

static const char *rssi_to_quality(int8_t rssi)
{
    if (rssi > -50) return "Excellent";
    if (rssi > -60) return "Good";
    if (rssi > -70) return "Fair";
    if (rssi > -80) return "Weak";
    return "Very Weak";
}

/* Text-based signal bar: 5 chars using '#' (filled) and '.' (empty) */
static void rssi_bar(int8_t rssi, char *buf, size_t len)
{
    int bars;
    if (rssi > -50)      bars = 5;
    else if (rssi > -60) bars = 4;
    else if (rssi > -70) bars = 3;
    else if (rssi > -80) bars = 2;
    else                 bars = 1;

    size_t pos = 0;
    for (int i = 0; i < 5 && pos + 1 < len; i++) {
        buf[pos++] = (i < bars) ? '#' : '.';
    }
    buf[pos] = '\0';
}

/* ------------------------------------------------------------------ */
/* Channel bar                                                          */
/* ------------------------------------------------------------------ */

static void update_channel_bars(void)
{
    int channels[14] = {0};
    for (int i = 0; i < s_scanner.result_count; i++) {
        int ch = s_scanner.results[i].channel;
        if (ch >= 1 && ch <= 14) channels[ch - 1]++;
    }

    char bar_text[128];
    int pos = 0;
    for (int i = 0; i < 14 && pos < (int)sizeof(bar_text) - 4; i++) {
        char symbol = ' ';
        if (channels[i] >= 3)      symbol = '#';   /* heavy   */
        else if (channels[i] == 2) symbol = '=';   /* medium  */
        else if (channels[i] == 1) symbol = '-';   /* light   */
        pos += snprintf(bar_text + pos, sizeof(bar_text) - (size_t)pos,
                        "%2d%c ", i + 1, symbol);
    }
    bar_text[sizeof(bar_text) - 1] = '\0';

    if (s_scanner.channel_bar) {
        lv_label_set_text(s_scanner.channel_bar, bar_text);
    }
}

/* ------------------------------------------------------------------ */
/* Scan                                                                 */
/* ------------------------------------------------------------------ */

static void do_scan(void)
{
    if (s_scanner.scanning) return;
    s_scanner.scanning = true;

    if (s_scanner.scan_btn_lbl) {
        lv_label_set_text(s_scanner.scan_btn_lbl, "...");
    }
    if (s_scanner.count_label) {
        lv_label_set_text(s_scanner.count_label, "Scanning...");
    }

    wifi_scan_result_t raw[WIFI_SCAN_MAX_RESULTS];
    uint8_t count = 0;

    esp_err_t err = wifi_manager_scan(raw, WIFI_SCAN_MAX_RESULTS, &count);

    if (err != ESP_OK) {
        ESP_LOGW(TAG, "wifi_manager_scan failed: %d", err);
        toast_warn("Scan failed");
        s_scanner.result_count = 0;
        s_scanner.scanning = false;
        if (s_scanner.scan_btn_lbl) lv_label_set_text(s_scanner.scan_btn_lbl, "Scan");
        if (s_scanner.count_label)  lv_label_set_text(s_scanner.count_label,  "No results");
        populate_network_list();
        update_channel_bars();
        return;
    }

    /* Map raw results into our richer scan_entry_t */
    int n = (count < MAX_SCAN_RESULTS) ? count : MAX_SCAN_RESULTS;
    s_scanner.result_count = n;

    for (int i = 0; i < n; i++) {
        scan_entry_t *e = &s_scanner.results[i];
        memset(e, 0, sizeof(*e));

        strncpy(e->ssid, raw[i].ssid, sizeof(e->ssid) - 1);
        e->ssid[sizeof(e->ssid) - 1] = '\0';

        e->rssi    = raw[i].rssi;
        e->channel = raw[i].channel;

        /* BSSID not available via current API — leave zeroed (shown as N/A) */

        /* Auth type: simplified mapping from is_open flag */
        if (raw[i].is_open) {
            strncpy(e->auth_str, "Open", sizeof(e->auth_str) - 1);
        } else {
            strncpy(e->auth_str, "WPA2", sizeof(e->auth_str) - 1);
        }
        e->auth_str[sizeof(e->auth_str) - 1] = '\0';

        /* 5 GHz: channels 36+ */
        e->is_5ghz = (e->channel >= 36);
    }

    ESP_LOGI(TAG, "Scan complete: %d APs", n);

    s_scanner.scanning = false;

    if (s_scanner.scan_btn_lbl) {
        lv_label_set_text(s_scanner.scan_btn_lbl, "Scan");
    }

    char count_buf[24];
    snprintf(count_buf, sizeof(count_buf), "%d AP%s", n, n == 1 ? "" : "s");
    if (s_scanner.count_label) {
        lv_label_set_text(s_scanner.count_label, count_buf);
    }

    populate_network_list();
    update_channel_bars();
}

/* ------------------------------------------------------------------ */
/* Network list population                                              */
/* ------------------------------------------------------------------ */

static void network_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t *row = lv_event_get_target(e);
    intptr_t idx  = (intptr_t)lv_obj_get_user_data(row);
    if (idx >= 0 && idx < s_scanner.result_count) {
        switch_to_detail((int)idx);
    }
}

static void populate_network_list(void)
{
    if (!s_scanner.network_list) return;

    lv_obj_clean(s_scanner.network_list);
    lv_obj_scroll_to_y(s_scanner.network_list, 0, LV_ANIM_OFF);

    const theme_colors_t *clr = theme_get_colors();

    if (s_scanner.result_count == 0) {
        lv_obj_t *lbl = lv_label_create(s_scanner.network_list);
        lv_label_set_text(lbl, "(no networks — press Scan)");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        return;
    }

    for (int i = 0; i < s_scanner.result_count; i++) {
        const scan_entry_t *ap = &s_scanner.results[i];

        lv_obj_t *row = lv_obj_create(s_scanner.network_list);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, clr->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, 6, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, 4, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(row, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
        lv_obj_set_style_bg_color(row, clr->primary, LV_STATE_PRESSED);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_set_user_data(row, (void *)(intptr_t)i);
        lv_obj_add_event_cb(row, network_row_clicked_cb, LV_EVENT_CLICKED, NULL);

        /* SSID — takes up most of the row */
        lv_obj_t *ssid_lbl = lv_label_create(row);
        const char *ssid_text = (ap->ssid[0] != '\0') ? ap->ssid : "(hidden)";
        lv_label_set_text(ssid_lbl, ssid_text);
        lv_label_set_long_mode(ssid_lbl, LV_LABEL_LONG_CLIP);
        lv_obj_set_style_text_font(ssid_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(ssid_lbl, clr->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(ssid_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_flex_grow(ssid_lbl, 1);

        /* RSSI */
        char rssi_buf[8];
        snprintf(rssi_buf, sizeof(rssi_buf), "%d", (int)ap->rssi);
        lv_obj_t *rssi_lbl = lv_label_create(row);
        lv_label_set_text(rssi_lbl, rssi_buf);
        lv_obj_set_style_text_font(rssi_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(rssi_lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(rssi_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_width(rssi_lbl, 34);

        /* Channel */
        char ch_buf[10];
        snprintf(ch_buf, sizeof(ch_buf), "Ch%d", (int)ap->channel);
        lv_obj_t *ch_lbl = lv_label_create(row);
        lv_label_set_text(ch_lbl, ch_buf);
        lv_obj_set_style_text_font(ch_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(ch_lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(ch_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_width(ch_lbl, 36);

        /* Auth abbreviation */
        lv_obj_t *auth_lbl = lv_label_create(row);
        lv_label_set_text(auth_lbl, ap->auth_str);
        lv_obj_set_style_text_font(auth_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(auth_lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(auth_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_width(auth_lbl, 36);
    }
}

/* ------------------------------------------------------------------ */
/* Event callbacks — list screen                                        */
/* ------------------------------------------------------------------ */

static void scan_btn_cb(lv_event_t *e)
{
    (void)e;
    do_scan();
}

static void list_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC || key == 'q' || key == 'Q') {
        app_manager_launch("com.thistle.launcher");
    }
}

/* ------------------------------------------------------------------ */
/* Event callbacks — detail screen                                      */
/* ------------------------------------------------------------------ */

static void back_btn_cb(lv_event_t *e)
{
    (void)e;
    switch_to_list();
}

static void connect_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_scanner.selected_idx;
    if (idx < 0 || idx >= s_scanner.result_count) return;

    const scan_entry_t *ap = &s_scanner.results[idx];

    if (ap->ssid[0] == '\0') {
        toast_warn("Cannot connect: hidden network");
        return;
    }

    /* For open networks, connect with empty password */
    const char *pass = (strcmp(ap->auth_str, "Open") == 0) ? "" : NULL;

    if (pass != NULL) {
        /* Open network — attempt direct connect */
        esp_err_t err = wifi_manager_connect(ap->ssid, pass, 0);
        if (err == ESP_OK) {
            toast_show("Connected!", TOAST_SUCCESS, 2000);
        } else {
            toast_warn("Connection failed");
        }
    } else {
        /* Secured network — inform user (no password entry in MVP) */
        toast_info("Use Settings > WiFi to connect with password");
    }
}

static void detail_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC) {
        switch_to_list();
    }
}

/* ------------------------------------------------------------------ */
/* Screen transitions                                                   */
/* ------------------------------------------------------------------ */

static void switch_to_list(void)
{
    if (s_scanner.detail_screen) {
        lv_obj_add_flag(s_scanner.detail_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_scanner.list_screen) {
        lv_obj_clear_flag(s_scanner.list_screen, LV_OBJ_FLAG_HIDDEN);
    }
}

static void switch_to_detail(int idx)
{
    if (idx < 0 || idx >= s_scanner.result_count) return;

    s_scanner.selected_idx = idx;
    const scan_entry_t *ap = &s_scanner.results[idx];

    /* SSID */
    const char *ssid_text = (ap->ssid[0] != '\0') ? ap->ssid : "(hidden)";
    char ssid_buf[64];
    snprintf(ssid_buf, sizeof(ssid_buf), "SSID: %s", ssid_text);
    lv_label_set_text(s_scanner.detail_ssid_lbl, ssid_buf);

    /* BSSID — not available via current API */
    lv_label_set_text(s_scanner.detail_bssid_lbl, "BSSID: N/A");

    /* Channel + band */
    char ch_buf[40];
    snprintf(ch_buf, sizeof(ch_buf), "Channel: %d (%s)",
             (int)ap->channel, ap->is_5ghz ? "5 GHz" : "2.4 GHz");
    lv_label_set_text(s_scanner.detail_channel_lbl, ch_buf);

    /* RSSI + quality */
    char rssi_buf[48];
    snprintf(rssi_buf, sizeof(rssi_buf), "RSSI: %d dBm (%s)",
             (int)ap->rssi, rssi_to_quality(ap->rssi));
    lv_label_set_text(s_scanner.detail_rssi_lbl, rssi_buf);

    /* Security */
    char auth_buf[32];
    snprintf(auth_buf, sizeof(auth_buf), "Security: %s", ap->auth_str);
    lv_label_set_text(s_scanner.detail_auth_lbl, auth_buf);

    /* Signal bar + percentage */
    char bar[8];
    rssi_bar(ap->rssi, bar, sizeof(bar));
    /* Percentage: map -100 dBm..0 dBm to 0%..100% (clamp) */
    int pct = (int)ap->rssi + 100;
    if (pct < 0)   pct = 0;
    if (pct > 100) pct = 100;
    char sig_buf[32];
    snprintf(sig_buf, sizeof(sig_buf), "Signal: %s %d%%", bar, pct);
    lv_label_set_text(s_scanner.detail_signal_bar_lbl, sig_buf);

    /* Show detail, hide list */
    lv_obj_add_flag(s_scanner.list_screen, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(s_scanner.detail_screen, LV_OBJ_FLAG_HIDDEN);
}

/* ------------------------------------------------------------------ */
/* UI construction helpers                                              */
/* ------------------------------------------------------------------ */

/* Standard header bar used by both screens */
static lv_obj_t *make_header(lv_obj_t *parent, const theme_colors_t *clr)
{
    lv_obj_t *hdr = lv_obj_create(parent);
    lv_obj_set_size(hdr, APP_AREA_W, HEADER_H);
    lv_obj_set_pos(hdr, 0, 0);
    lv_obj_set_style_bg_color(hdr, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(hdr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(hdr, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(hdr, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(hdr, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(hdr, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_right(hdr, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_top(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(hdr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(hdr, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(hdr, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(hdr, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(hdr, 6, LV_PART_MAIN);
    return hdr;
}

/* Small text button */
static lv_obj_t *make_btn(lv_obj_t *parent, const char *text, const theme_colors_t *clr, bool filled)
{
    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_height(btn, 22);
    lv_obj_set_style_bg_color(btn, filled ? clr->primary : clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, filled ? 0 : 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_hor(btn, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_ver(btn, 2, LV_PART_MAIN);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, filled ? lv_color_white() : clr->text, LV_PART_MAIN);
    lv_obj_center(lbl);

    return btn;
}

/* Detail info row label */
static lv_obj_t *make_detail_label(lv_obj_t *parent, const theme_colors_t *clr)
{
    lv_obj_t *lbl = lv_label_create(parent);
    lv_obj_set_width(lbl, LV_PCT(100));
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, clr->text, LV_PART_MAIN);
    lv_obj_set_style_pad_left(lbl, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(lbl, 4, LV_PART_MAIN);
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_WRAP);
    return lbl;
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t wifiscanner_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating WiFi Scanner UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_scanner, 0, sizeof(s_scanner));
    s_scanner.selected_idx = -1;

    const theme_colors_t *clr = theme_get_colors();

    /* ----------------------------------------------------------------
     * Root container
     * ---------------------------------------------------------------- */
    s_scanner.root = lv_obj_create(parent);
    lv_obj_set_size(s_scanner.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_scanner.root, 0, 0);
    lv_obj_set_style_bg_opa(s_scanner.root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_scanner.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_scanner.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_scanner.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_scanner.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ================================================================
     * LIST SCREEN
     * ================================================================ */
    s_scanner.list_screen = lv_obj_create(s_scanner.root);
    lv_obj_set_size(s_scanner.list_screen, APP_AREA_W, APP_AREA_H);
    lv_obj_set_pos(s_scanner.list_screen, 0, 0);
    lv_obj_set_style_bg_color(s_scanner.list_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_scanner.list_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_scanner.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_scanner.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_scanner.list_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_scanner.list_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_scanner.list_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_scanner.list_screen, list_key_cb, LV_EVENT_KEY, NULL);

    /* List header */
    lv_obj_t *list_hdr = make_header(s_scanner.list_screen, clr);
    lv_obj_set_flex_align(list_hdr, LV_FLEX_ALIGN_SPACE_BETWEEN, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);

    lv_obj_t *list_title = lv_label_create(list_hdr);
    lv_label_set_text(list_title, "WiFi Scanner");
    lv_obj_set_style_text_font(list_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(list_title, clr->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(list_title, 1);

    /* "Scan" button */
    lv_obj_t *scan_btn = make_btn(list_hdr, "Scan", clr, true);
    lv_obj_add_event_cb(scan_btn, scan_btn_cb, LV_EVENT_CLICKED, NULL);
    s_scanner.scan_btn_lbl = lv_obj_get_child(scan_btn, 0);

    /* AP count label */
    s_scanner.count_label = lv_label_create(list_hdr);
    lv_label_set_text(s_scanner.count_label, "0 APs");
    lv_obj_set_style_text_font(s_scanner.count_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_scanner.count_label, clr->text_secondary, LV_PART_MAIN);

    /* Scrollable network list */
    s_scanner.network_list = lv_obj_create(s_scanner.list_screen);
    lv_obj_set_pos(s_scanner.network_list, 0, HEADER_H);
    lv_obj_set_size(s_scanner.network_list, APP_AREA_W, LIST_H);
    lv_obj_set_style_bg_color(s_scanner.network_list, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_scanner.network_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_scanner.network_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_scanner.network_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_scanner.network_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_scanner.network_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_scanner.network_list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(s_scanner.network_list, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_scanner.network_list, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_scanner.network_list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_scanner.network_list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_scanner.network_list, 0, LV_PART_SCROLLBAR);

    populate_network_list();

    /* Channel utilisation bar */
    s_scanner.channel_bar = lv_label_create(s_scanner.list_screen);
    lv_obj_set_pos(s_scanner.channel_bar, 0, HEADER_H + LIST_H);
    lv_obj_set_size(s_scanner.channel_bar, APP_AREA_W, CHANNEL_BAR_H);
    lv_obj_set_style_text_font(s_scanner.channel_bar, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_scanner.channel_bar, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_bg_color(s_scanner.channel_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_scanner.channel_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_scanner.channel_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_scanner.channel_bar, 2, LV_PART_MAIN);
    lv_label_set_long_mode(s_scanner.channel_bar, LV_LABEL_LONG_CLIP);
    update_channel_bars();

    /* ================================================================
     * DETAIL SCREEN
     * ================================================================ */
    s_scanner.detail_screen = lv_obj_create(s_scanner.root);
    lv_obj_set_size(s_scanner.detail_screen, APP_AREA_W, APP_AREA_H);
    lv_obj_set_pos(s_scanner.detail_screen, 0, 0);
    lv_obj_set_style_bg_color(s_scanner.detail_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_scanner.detail_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_scanner.detail_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_scanner.detail_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_scanner.detail_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_scanner.detail_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_scanner.detail_screen, LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(s_scanner.detail_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_scanner.detail_screen, detail_key_cb, LV_EVENT_KEY, NULL);

    /* Detail header */
    lv_obj_t *det_hdr = make_header(s_scanner.detail_screen, clr);

    lv_obj_t *back_btn = make_btn(det_hdr, "< Back", clr, false);
    lv_obj_add_event_cb(back_btn, back_btn_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *det_title = lv_label_create(det_hdr);
    lv_label_set_text(det_title, "Network Detail");
    lv_obj_set_style_text_font(det_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(det_title, clr->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(det_title, 1);

    /* Detail info container (scrollable in case content overflows) */
    lv_obj_t *det_content = lv_obj_create(s_scanner.detail_screen);
    lv_obj_set_pos(det_content, 0, HEADER_H);
    lv_obj_set_size(det_content, APP_AREA_W, APP_AREA_H - HEADER_H);
    lv_obj_set_style_bg_color(det_content, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(det_content, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(det_content, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(det_content, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(det_content, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(det_content, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(det_content, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(det_content, LV_SCROLLBAR_MODE_OFF);
    lv_obj_clear_flag(det_content, LV_OBJ_FLAG_SCROLLABLE);

    s_scanner.detail_ssid_lbl       = make_detail_label(det_content, clr);
    s_scanner.detail_bssid_lbl      = make_detail_label(det_content, clr);
    s_scanner.detail_channel_lbl    = make_detail_label(det_content, clr);
    s_scanner.detail_rssi_lbl       = make_detail_label(det_content, clr);
    s_scanner.detail_auth_lbl       = make_detail_label(det_content, clr);

    /* Divider */
    lv_obj_t *divider = lv_obj_create(det_content);
    lv_obj_set_size(divider, APP_AREA_W - 16, 1);
    lv_obj_set_style_bg_color(divider, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(divider, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(divider, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(divider, 0, LV_PART_MAIN);
    lv_obj_set_style_margin_left(divider, 8, LV_PART_MAIN);
    lv_obj_set_style_margin_top(divider, 4, LV_PART_MAIN);
    lv_obj_set_style_margin_bottom(divider, 4, LV_PART_MAIN);

    s_scanner.detail_signal_bar_lbl = make_detail_label(det_content, clr);

    /* Connect button row */
    lv_obj_t *btn_row = lv_obj_create(det_content);
    lv_obj_set_size(btn_row, APP_AREA_W, 34);
    lv_obj_set_style_bg_opa(btn_row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(btn_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(btn_row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_left(btn_row, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn_row, 4, LV_PART_MAIN);

    lv_obj_t *connect_btn = make_btn(btn_row, "Connect", clr, true);
    lv_obj_add_event_cb(connect_btn, connect_btn_cb, LV_EVENT_CLICKED, NULL);

    /* Populate placeholder text */
    lv_label_set_text(s_scanner.detail_ssid_lbl,       "SSID: —");
    lv_label_set_text(s_scanner.detail_bssid_lbl,      "BSSID: N/A");
    lv_label_set_text(s_scanner.detail_channel_lbl,    "Channel: —");
    lv_label_set_text(s_scanner.detail_rssi_lbl,       "RSSI: —");
    lv_label_set_text(s_scanner.detail_auth_lbl,       "Security: —");
    lv_label_set_text(s_scanner.detail_signal_bar_lbl, "Signal: —");

    return ESP_OK;
}

void wifiscanner_ui_show(void)
{
    if (s_scanner.root) {
        lv_obj_clear_flag(s_scanner.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void wifiscanner_ui_hide(void)
{
    if (s_scanner.root) {
        lv_obj_add_flag(s_scanner.root, LV_OBJ_FLAG_HIDDEN);
    }
}
