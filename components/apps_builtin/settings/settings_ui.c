/*
 * settings_ui.c — ThistleOS Settings application UI
 *
 * Navigation: main category list -> WiFi sub-screen
 *                                -> About sub-screen
 *                                <- Back button returns to list
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "settings/settings_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_err.h"
#include "esp_system.h"
#include "esp_heap_caps.h"

#include "thistle/wifi_manager.h"
#include "thistle/kernel.h"
#include "hal/board.h"

#include <stdio.h>
#include <string.h>
#include <stdint.h>

static const char *TAG = "settings_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      320
#define APP_AREA_H      216
#define TITLE_BAR_H      30
#define ITEM_H           30
#define ITEM_PAD_LEFT     8
#define ITEM_PAD_RIGHT    6
#define SUB_CONTENT_Y    TITLE_BAR_H   /* content starts below title bar */

/* ------------------------------------------------------------------ */
/* Navigation state                                                     */
/* ------------------------------------------------------------------ */

typedef enum {
    SETTINGS_MAIN,
    SETTINGS_WIFI,
    SETTINGS_ABOUT,
} settings_screen_t;

static settings_screen_t  s_current_screen = SETTINGS_MAIN;
static lv_obj_t          *s_root           = NULL;
static lv_obj_t          *s_main_list      = NULL; /* scrollable category list */
static lv_obj_t          *s_sub_screen     = NULL; /* current sub-screen container */

/* WiFi init guard — only call wifi_manager_init() once */
static bool s_wifi_inited = false;

/* WiFi sub-screen state */
static lv_obj_t *s_wifi_status_label  = NULL;
static lv_obj_t *s_wifi_scan_list     = NULL;

/* ------------------------------------------------------------------ */
/* Settings category definitions                                        */
/* ------------------------------------------------------------------ */

typedef struct {
    const char *name;
    const char *value; /* NULL = no value shown */
} settings_item_t;

static const settings_item_t s_items[] = {
    { "Display",   NULL      },
    { "WiFi",      "Off"     },
    { "Bluetooth", "Off"     },
    { "Radio",     "915 MHz" },
    { "Storage",   NULL      },
    { "About",     NULL      },
};
#define ITEMS_COUNT (sizeof(s_items) / sizeof(s_items[0]))

/* ------------------------------------------------------------------ */
/* Style helpers                                                        */
/* ------------------------------------------------------------------ */

/* Apply the standard BlackBerry monochrome style to a container/panel. */
static void style_panel(lv_obj_t *obj)
{
    lv_obj_set_style_bg_color(obj, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(obj, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(obj, 0, LV_PART_MAIN);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE);
}

/* Apply the title bar style (white bg, 1px bottom border). */
static void style_title_bar(lv_obj_t *obj)
{
    style_panel(obj);
    lv_obj_set_style_pad_left(obj, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(obj, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_border_side(obj, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(obj, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 1, LV_PART_MAIN);
}

/* Create a standard row separator (1px horizontal line). */
static lv_obj_t *create_separator(lv_obj_t *parent)
{
    lv_obj_t *sep = lv_obj_create(parent);
    lv_obj_set_size(sep, LV_PCT(100), 1);
    lv_obj_set_style_bg_color(sep, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(sep, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(sep, 0, LV_PART_MAIN);
    lv_obj_clear_flag(sep, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);
    return sep;
}

/* Create an info row: "label: value" with Montserrat 14. */
static lv_obj_t *create_info_row(lv_obj_t *parent, const char *text)
{
    lv_obj_t *row = lv_obj_create(parent);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *lbl = lv_label_create(row);
    lv_label_set_text(lbl, text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_black(), LV_PART_MAIN);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);

    return row;
}

/* ------------------------------------------------------------------ */
/* Back navigation                                                      */
/* ------------------------------------------------------------------ */

static void back_to_main(lv_event_t *e)
{
    (void)e;
    if (s_sub_screen) {
        lv_obj_delete(s_sub_screen);
        s_sub_screen = NULL;
    }
    s_wifi_status_label = NULL;
    s_wifi_scan_list    = NULL;
    lv_obj_remove_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
    s_current_screen = SETTINGS_MAIN;
}

/* ------------------------------------------------------------------ */
/* Sub-screen scaffold: creates the full-area container + title bar    */
/* Returns the scrollable content area below the title bar.            */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_sub_screen(const char *title)
{
    /* Outer container — same size as the app area */
    s_sub_screen = lv_obj_create(s_root);
    lv_obj_set_pos(s_sub_screen, 0, 0);
    lv_obj_set_size(s_sub_screen, APP_AREA_W, APP_AREA_H);
    style_panel(s_sub_screen);

    /* Title bar row: "< Title" */
    lv_obj_t *title_bar = lv_obj_create(s_sub_screen);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    style_title_bar(title_bar);

    /* Back button label "< Title" — the whole bar is the back button */
    lv_obj_add_flag(title_bar, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(title_bar, lv_color_black(), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_event_cb(title_bar, back_to_main, LV_EVENT_CLICKED, NULL);

    char back_text[48];
    snprintf(back_text, sizeof(back_text), "< %s", title);
    lv_obj_t *lbl = lv_label_create(title_bar);
    lv_label_set_text(lbl, back_text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);

    /* Scrollable content area below the title bar */
    lv_obj_t *content = lv_obj_create(s_sub_screen);
    lv_obj_set_pos(content, 0, TITLE_BAR_H);
    lv_obj_set_size(content, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(content, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(content, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(content, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(content, 0, LV_PART_MAIN);

    /* Vertical flex column */
    lv_obj_set_flex_flow(content, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(content,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);

    /* Scrollbar style */
    lv_obj_set_scrollbar_mode(content, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(content, lv_color_black(), LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(content, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(content, 0, LV_PART_SCROLLBAR);

    return content;
}

/* ------------------------------------------------------------------ */
/* WiFi sub-screen                                                      */
/* ------------------------------------------------------------------ */

static void wifi_update_status_label(void)
{
    if (!s_wifi_status_label) return;

    wifi_state_t state = (wifi_state_t)wifi_manager_get_state();
    char buf[64];

    switch (state) {
    case WIFI_STATE_CONNECTED: {
        const char *ip = wifi_manager_get_ip();
        if (ip) {
            snprintf(buf, sizeof(buf), "Status: Connected (%s)", ip);
        } else {
            snprintf(buf, sizeof(buf), "Status: Connected");
        }
        break;
    }
    case WIFI_STATE_CONNECTING:
        snprintf(buf, sizeof(buf), "Status: Connecting...");
        break;
    case WIFI_STATE_FAILED:
        snprintf(buf, sizeof(buf), "Status: Failed");
        break;
    case WIFI_STATE_DISCONNECTED:
    default:
        snprintf(buf, sizeof(buf), "Status: Disconnected");
        break;
    }

    lv_label_set_text(s_wifi_status_label, buf);
}

/* Called when a scan result network row is clicked. */
static void wifi_network_clicked_cb(lv_event_t *e)
{
    wifi_scan_result_t *net = (wifi_scan_result_t *)lv_event_get_user_data(e);
    if (!net) return;

    ESP_LOGI(TAG, "WiFi: attempting connect to \"%s\" (open=%d)", net->ssid, net->is_open);

    /* Update status label to "Connecting..." immediately */
    if (s_wifi_status_label) {
        lv_label_set_text(s_wifi_status_label, "Status: Connecting...");
    }

    const char *password = net->is_open ? NULL : "";
    esp_err_t err = wifi_manager_connect(net->ssid, password, 10000);

    if (err == ESP_OK) {
        ESP_LOGI(TAG, "WiFi: connected to \"%s\"", net->ssid);
        wifi_manager_ntp_sync();
    } else if (err == ESP_ERR_NOT_SUPPORTED) {
        ESP_LOGI(TAG, "WiFi: connect not supported in simulator");
    } else {
        ESP_LOGW(TAG, "WiFi: connect to \"%s\" failed: 0x%x", net->ssid, err);
    }

    wifi_update_status_label();
}

/* Scan results buffer — persists for the lifetime of the WiFi screen. */
static wifi_scan_result_t s_scan_results[WIFI_SCAN_MAX_RESULTS];
static uint8_t            s_scan_count = 0;

static void wifi_scan_clicked_cb(lv_event_t *e)
{
    (void)e;
    ESP_LOGI(TAG, "WiFi: scanning...");

    if (s_wifi_status_label) {
        lv_label_set_text(s_wifi_status_label, "Status: Scanning...");
    }

    /* Clear previous scan list entries */
    if (s_wifi_scan_list) {
        lv_obj_clean(s_wifi_scan_list);
    }

    s_scan_count = 0;
    esp_err_t err = wifi_manager_scan(s_scan_results, WIFI_SCAN_MAX_RESULTS, &s_scan_count);

    if (err == ESP_ERR_NOT_SUPPORTED) {
        ESP_LOGI(TAG, "WiFi: scan not supported in simulator");
        /* Show a placeholder entry in the list */
        if (s_wifi_scan_list) {
            create_info_row(s_wifi_scan_list, "(Scan not available in simulator)");
        }
        wifi_update_status_label();
        return;
    }

    if (err != ESP_OK) {
        ESP_LOGW(TAG, "WiFi: scan failed: 0x%x", err);
        wifi_update_status_label();
        return;
    }

    ESP_LOGI(TAG, "WiFi: found %d networks", s_scan_count);
    wifi_update_status_label();

    if (!s_wifi_scan_list) return;

    for (uint8_t i = 0; i < s_scan_count; i++) {
        wifi_scan_result_t *net = &s_scan_results[i];

        /* Row */
        lv_obj_t *row = lv_obj_create(s_wifi_scan_list);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, lv_color_white(), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(row, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
        lv_obj_set_style_bg_color(row, lv_color_black(), LV_STATE_PRESSED);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(row,
                              LV_FLEX_ALIGN_START,
                              LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_add_event_cb(row, wifi_network_clicked_cb, LV_EVENT_CLICKED, net);

        /* SSID label */
        lv_obj_t *lbl_ssid = lv_label_create(row);
        char ssid_text[WIFI_SSID_MAX_LEN + 4];
        if (!net->is_open) {
            snprintf(ssid_text, sizeof(ssid_text), "%s [*]", net->ssid);
        } else {
            snprintf(ssid_text, sizeof(ssid_text), "%s", net->ssid);
        }
        lv_label_set_text(lbl_ssid, ssid_text);
        lv_obj_set_style_text_font(lbl_ssid, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_ssid, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_ssid, lv_color_white(), LV_STATE_PRESSED);
        lv_obj_set_flex_grow(lbl_ssid, 1);

        /* RSSI label */
        lv_obj_t *lbl_rssi = lv_label_create(row);
        char rssi_text[16];
        snprintf(rssi_text, sizeof(rssi_text), "%d dBm", net->rssi);
        lv_label_set_text(lbl_rssi, rssi_text);
        lv_obj_set_style_text_font(lbl_rssi, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_rssi, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_rssi, lv_color_white(), LV_STATE_PRESSED);
    }
}

static void open_wifi_screen(void)
{
    /* Initialize WiFi subsystem once */
    if (!s_wifi_inited) {
        esp_err_t err = wifi_manager_init();
        if (err == ESP_OK) {
            s_wifi_inited = true;
        } else {
            ESP_LOGW(TAG, "wifi_manager_init failed: 0x%x", err);
        }
    }

    lv_obj_t *content = create_sub_screen("WiFi");
    s_current_screen = SETTINGS_WIFI;

    /* Status row */
    lv_obj_t *status_row = lv_obj_create(content);
    lv_obj_set_size(status_row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(status_row, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(status_row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(status_row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(status_row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(status_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(status_row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    s_wifi_status_label = lv_label_create(status_row);
    lv_label_set_text(s_wifi_status_label, "Status: Disconnected");
    lv_obj_set_style_text_font(s_wifi_status_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_wifi_status_label, lv_color_black(), LV_PART_MAIN);
    lv_obj_align(s_wifi_status_label, LV_ALIGN_LEFT_MID, 0, 0);
    wifi_update_status_label();

    create_separator(content);

    /* Scan Networks button */
    lv_obj_t *btn = lv_obj_create(content);
    lv_obj_set_size(btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(btn, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, lv_color_black(), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(btn, wifi_scan_clicked_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *lbl_btn = lv_label_create(btn);
    lv_label_set_text(lbl_btn, "Scan Networks");
    lv_obj_set_style_text_font(lbl_btn, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_btn, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_btn, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_align(lbl_btn, LV_ALIGN_CENTER, 0, 0);

    create_separator(content);

    /* Scan results list (initially empty, populated by scan button) */
    s_wifi_scan_list = lv_obj_create(content);
    lv_obj_set_width(s_wifi_scan_list, LV_PCT(100));
    lv_obj_set_height(s_wifi_scan_list, LV_SIZE_CONTENT);
    lv_obj_set_style_bg_color(s_wifi_scan_list, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_wifi_scan_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_wifi_scan_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_wifi_scan_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_wifi_scan_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_wifi_scan_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_wifi_scan_list,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_clear_flag(s_wifi_scan_list, LV_OBJ_FLAG_SCROLLABLE);
}

/* ------------------------------------------------------------------ */
/* About sub-screen                                                     */
/* ------------------------------------------------------------------ */

static void open_about_screen(void)
{
    lv_obj_t *content = create_sub_screen("About");
    s_current_screen = SETTINGS_ABOUT;

    /* ThistleOS title */
    lv_obj_t *title_row = lv_obj_create(content);
    lv_obj_set_size(title_row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(title_row, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(title_row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(title_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(title_row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(title_row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(title_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(title_row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(title_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(title_row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *lbl_os = lv_label_create(title_row);
    lv_label_set_text(lbl_os, "ThistleOS");
    lv_obj_set_style_text_font(lbl_os, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_os, lv_color_black(), LV_PART_MAIN);
    lv_obj_align(lbl_os, LV_ALIGN_LEFT_MID, 0, 0);

    /* Version */
    char ver_buf[32];
    snprintf(ver_buf, sizeof(ver_buf), "Version: %s", THISTLE_VERSION_STRING);
    create_info_row(content, ver_buf);

    create_separator(content);

    /* Board name */
    const hal_registry_t *reg = hal_get_registry();
    char board_buf[64];
    snprintf(board_buf, sizeof(board_buf), "Board: %s",
             (reg && reg->board_name) ? reg->board_name : "Unknown");
    create_info_row(content, board_buf);

    create_separator(content);

    /* Display driver */
    char disp_buf[64];
    if (reg && reg->display && reg->display->name) {
        snprintf(disp_buf, sizeof(disp_buf), "Display: %s %ux%u",
                 reg->display->name,
                 reg->display->width,
                 reg->display->height);
    } else {
        snprintf(disp_buf, sizeof(disp_buf), "Display: Unknown");
    }
    create_info_row(content, disp_buf);

    /* Radio driver */
    char radio_buf[48];
    if (reg && reg->radio && reg->radio->name) {
        snprintf(radio_buf, sizeof(radio_buf), "Radio: %s", reg->radio->name);
    } else {
        snprintf(radio_buf, sizeof(radio_buf), "Radio: None");
    }
    create_info_row(content, radio_buf);

    /* GPS driver */
    char gps_buf[48];
    if (reg && reg->gps && reg->gps->name) {
        snprintf(gps_buf, sizeof(gps_buf), "GPS: %s", reg->gps->name);
    } else {
        snprintf(gps_buf, sizeof(gps_buf), "GPS: None");
    }
    create_info_row(content, gps_buf);

    create_separator(content);

    /* Free heap */
    char heap_buf[48];
    snprintf(heap_buf, sizeof(heap_buf), "Free heap: %lu bytes",
             (unsigned long)esp_get_free_heap_size());
    create_info_row(content, heap_buf);

    /* PSRAM free */
    char psram_buf[48];
    snprintf(psram_buf, sizeof(psram_buf), "PSRAM free: %lu bytes",
             (unsigned long)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    create_info_row(content, psram_buf);

    /* Uptime */
    uint32_t uptime_ms = kernel_uptime_ms();
    uint32_t s  = uptime_ms / 1000;
    uint32_t h  = s / 3600;
    uint32_t m  = (s % 3600) / 60;
    uint32_t sc = s % 60;
    char uptime_buf[48];
    snprintf(uptime_buf, sizeof(uptime_buf), "Uptime: %02lu:%02lu:%02lu",
             (unsigned long)h, (unsigned long)m, (unsigned long)sc);
    create_info_row(content, uptime_buf);
}

/* ------------------------------------------------------------------ */
/* Main list — category row creation                                   */
/* ------------------------------------------------------------------ */

static void item_clicked_cb(lv_event_t *e)
{
    const char *name = (const char *)lv_event_get_user_data(e);
    if (!name) return;

    if (strcmp(name, "WiFi") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_wifi_screen();
    } else if (strcmp(name, "About") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_about_screen();
    } else {
        ESP_LOGI(TAG, "Settings: %s — not implemented yet", name);
    }
}

static void create_list_item(lv_obj_t *list, const settings_item_t *item)
{
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
    lv_obj_set_style_bg_color(row, lv_color_black(), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, item_clicked_cb, LV_EVENT_CLICKED,
                        (void *)item->name);

    /* Name label — flex_grow=1 fills available space */
    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, item->name);
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    /* Value label (optional) */
    if (item->value != NULL) {
        lv_obj_t *lbl_val = lv_label_create(row);
        lv_label_set_text(lbl_val, item->value);
        lv_obj_set_style_text_font(lbl_val, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, lv_color_white(), LV_STATE_PRESSED);
    }

    /* Chevron */
    lv_obj_t *lbl_chevron = lv_label_create(row);
    lv_label_set_text(lbl_chevron, ">");
    lv_obj_set_style_text_font(lbl_chevron, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, lv_color_white(), LV_STATE_PRESSED);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t settings_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating settings UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Root container — fills the entire app area, transparent bg */
    s_root = lv_obj_create(parent);
    lv_obj_set_size(s_root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_root, 0, 0);
    lv_obj_set_style_bg_opa(s_root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Title bar (30px)
     * ---------------------------------------------------------------- */
    lv_obj_t *title_bar = lv_obj_create(s_root);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    style_title_bar(title_bar);

    lv_obj_t *lbl_title = lv_label_create(title_bar);
    lv_label_set_text(lbl_title, "Settings");
    lv_obj_set_style_text_font(lbl_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_title, lv_color_black(), LV_PART_MAIN);
    lv_obj_align(lbl_title, LV_ALIGN_LEFT_MID, 0, 0);

    /* ----------------------------------------------------------------
     * Scrollable list container below title bar
     * ---------------------------------------------------------------- */
    s_main_list = lv_obj_create(s_root);
    lv_obj_set_pos(s_main_list, 0, TITLE_BAR_H);
    lv_obj_set_size(s_main_list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(s_main_list, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_main_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_main_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_main_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_main_list, 0, LV_PART_MAIN);

    lv_obj_set_flex_flow(s_main_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_main_list,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);

    lv_obj_set_scrollbar_mode(s_main_list, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_main_list, lv_color_black(), LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_main_list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_main_list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_main_list, 0, LV_PART_SCROLLBAR);

    for (size_t i = 0; i < ITEMS_COUNT; i++) {
        create_list_item(s_main_list, &s_items[i]);
    }

    s_current_screen = SETTINGS_MAIN;
    s_sub_screen     = NULL;

    return ESP_OK;
}

void settings_ui_show(void)
{
    if (s_root) {
        lv_obj_clear_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}

void settings_ui_hide(void)
{
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
