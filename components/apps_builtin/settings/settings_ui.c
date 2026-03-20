/*
 * settings_ui.c — ThistleOS Settings application UI
 *
 * Navigation: main list -> WiFi
 *                       -> Drivers -> per-driver detail
 *                       -> About
 *             Back button returns up one level.
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
#include "thistle/ble_manager.h"
#include "thistle/kernel.h"
#include "thistle/signing.h"
#include "thistle/ota.h"
#include "thistle/driver_loader.h"
#include "hal/board.h"
#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/toast.h"
#include "ui/statusbar.h"

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
    SETTINGS_BLUETOOTH,
    SETTINGS_APPEARANCE,
    SETTINGS_ABOUT,
    SETTINGS_DRIVERS,
    SETTINGS_DRIVER_DETAIL,
} settings_screen_t;

static settings_screen_t  s_current_screen = SETTINGS_MAIN;
static lv_obj_t          *s_root           = NULL;
static lv_obj_t          *s_main_list      = NULL;
static lv_obj_t          *s_sub_screen     = NULL; /* level-1 sub-screen */
static lv_obj_t          *s_detail_screen  = NULL; /* level-2 driver detail */

/* WiFi init guard — only call wifi_manager_init() once */
static bool s_wifi_inited = false;

/* WiFi sub-screen state */
static lv_obj_t *s_wifi_status_label = NULL;
static lv_obj_t *s_wifi_scan_list    = NULL;

/* BLE init guard — only call ble_manager_init() once */
static bool s_ble_inited = false;

/* BLE sub-screen state */
static lv_obj_t *s_ble_status_label   = NULL;
static lv_obj_t *s_ble_name_label     = NULL;
static lv_obj_t *s_ble_peer_label     = NULL;
static lv_obj_t *s_ble_toggle_btn_lbl = NULL;

/* Power detail live-update timer */
static lv_timer_t *s_power_timer    = NULL;
static lv_obj_t   *s_power_batt_lbl = NULL;
static lv_obj_t   *s_power_pct_lbl  = NULL;
static lv_obj_t   *s_power_state_lbl= NULL;

/* GPS detail live-update timer */
static lv_timer_t *s_gps_timer      = NULL;
static lv_obj_t   *s_gps_status_lbl = NULL;
static lv_obj_t   *s_gps_lat_lbl    = NULL;
static lv_obj_t   *s_gps_lon_lbl    = NULL;
static lv_obj_t   *s_gps_sat_lbl    = NULL;

/* GPS enabled state */
static bool s_gps_enabled = false;

/* Main list dynamic value labels */
static lv_obj_t *s_wifi_value_label = NULL;
static lv_obj_t *s_bt_value_label   = NULL;

/* Main list live-update timer */
static lv_timer_t *s_main_timer = NULL;

/* ------------------------------------------------------------------ */
/* Driver type enum                                                     */
/* ------------------------------------------------------------------ */

typedef enum {
    DRIVER_TYPE_DISPLAY,
    DRIVER_TYPE_INPUT,
    DRIVER_TYPE_RADIO,
    DRIVER_TYPE_GPS,
    DRIVER_TYPE_AUDIO,
    DRIVER_TYPE_POWER,
    DRIVER_TYPE_IMU,
    DRIVER_TYPE_STORAGE,
} driver_type_t;

typedef struct {
    driver_type_t type;
    int           index;
} driver_row_data_t;

#define MAX_DRIVER_ROWS 16
static driver_row_data_t s_driver_row_pool[MAX_DRIVER_ROWS];
static int               s_driver_row_pool_used = 0;

/* ------------------------------------------------------------------ */
/* Settings category definitions                                        */
/* ------------------------------------------------------------------ */

typedef struct {
    const char *name;
    const char *value; /* NULL = no value shown */
} settings_item_t;

static const settings_item_t s_items[] = {
    { "WiFi",        "Off" },
    { "Bluetooth",   "Off" },
    { "Appearance",  NULL  },
    { "Drivers",     NULL  },
    { "About",       NULL  },
};
#define ITEMS_COUNT (sizeof(s_items) / sizeof(s_items[0]))

/* ------------------------------------------------------------------ */
/* Style helpers                                                        */
/* ------------------------------------------------------------------ */

/* Apply the standard BlackBerry monochrome style to a container/panel. */
static void style_panel(lv_obj_t *obj)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_set_style_bg_color(obj, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(obj, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(obj, 0, LV_PART_MAIN);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE);
}

/* Apply the title bar style (bg color, 1px bottom border). */
static void style_title_bar(lv_obj_t *obj)
{
    style_panel(obj);
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_set_style_pad_left(obj, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(obj, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_border_side(obj, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(obj, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 1, LV_PART_MAIN);
}

/* Create a standard row separator (1px horizontal line). */
static lv_obj_t *create_separator(lv_obj_t *parent)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *sep = lv_obj_create(parent);
    lv_obj_set_size(sep, LV_PCT(100), 1);
    lv_obj_set_style_bg_color(sep, tc->text, LV_PART_MAIN);
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
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *row = lv_obj_create(parent);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
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
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);

    return row;
}

/* Create a plain label row and return the label for live updates. */
static lv_obj_t *create_live_row(lv_obj_t *parent, const char *initial_text)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *row = lv_obj_create(parent);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *lbl = lv_label_create(row);
    lv_label_set_text(lbl, initial_text ? initial_text : "");
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);

    return lbl; /* caller saves for lv_label_set_text later */
}

/* ------------------------------------------------------------------ */
/* Main list live-update timer                                          */
/* ------------------------------------------------------------------ */

static void settings_main_update_timer(lv_timer_t *t)
{
    (void)t;
    if (s_current_screen != SETTINGS_MAIN) return;

    if (s_wifi_value_label) {
        wifi_state_t ws = (wifi_state_t)wifi_manager_get_state();
        const char *wifi_val = (ws == WIFI_STATE_CONNECTED)  ? "Connected"    :
                               (ws == WIFI_STATE_CONNECTING) ? "Connecting..." : "Off";
        lv_label_set_text(s_wifi_value_label, wifi_val);
    }
    if (s_bt_value_label) {
        ble_state_t bs = ble_manager_get_state();
        const char *bt_val = (bs == BLE_STATE_CONNECTED)   ? "Connected"  :
                             (bs == BLE_STATE_ADVERTISING) ? "Scanning..." : "Off";
        lv_label_set_text(s_bt_value_label, bt_val);
    }
}

/* ------------------------------------------------------------------ */
/* Back navigation                                                      */
/* ------------------------------------------------------------------ */

/* Detail -> Drivers */
static void back_to_drivers(lv_event_t *e)
{
    (void)e;

    /* Destroy live-update timers */
    if (s_power_timer) {
        lv_timer_delete(s_power_timer);
        s_power_timer     = NULL;
        s_power_batt_lbl  = NULL;
        s_power_pct_lbl   = NULL;
        s_power_state_lbl = NULL;
    }
    if (s_gps_timer) {
        lv_timer_delete(s_gps_timer);
        s_gps_timer      = NULL;
        s_gps_status_lbl = NULL;
        s_gps_lat_lbl    = NULL;
        s_gps_lon_lbl    = NULL;
        s_gps_sat_lbl    = NULL;
    }

    if (s_detail_screen) {
        lv_obj_delete(s_detail_screen);
        s_detail_screen = NULL;
    }
    if (s_sub_screen) {
        lv_obj_remove_flag(s_sub_screen, LV_OBJ_FLAG_HIDDEN);
    }
    s_current_screen = SETTINGS_DRIVERS;
}

/* Drivers / WiFi / Bluetooth / About -> Main */
static void back_to_main(lv_event_t *e)
{
    (void)e;
    if (s_sub_screen) {
        lv_obj_delete(s_sub_screen);
        s_sub_screen = NULL;
    }
    s_wifi_status_label    = NULL;
    s_wifi_scan_list       = NULL;
    s_ble_status_label     = NULL;
    s_ble_name_label       = NULL;
    s_ble_peer_label       = NULL;
    s_ble_toggle_btn_lbl   = NULL;
    s_driver_row_pool_used = 0;
    /* Value labels on the main list are still valid after returning */
    lv_obj_remove_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
    s_current_screen = SETTINGS_MAIN;
}

/* ------------------------------------------------------------------ */
/* Sub-screen scaffold: creates the full-area container + title bar    */
/* Returns the scrollable content area below the title bar.            */
/* ------------------------------------------------------------------ */

/*
 * alloc_sub_screen: generic helper — allocates a full-area container with a
 * title/back bar, parented to container_parent.  Stores the outer container
 * in *out_screen (if non-NULL).  Returns the scrollable content area.
 */
static lv_obj_t *alloc_sub_screen(lv_obj_t *container_parent,
                                  const char *title,
                                  lv_event_cb_t back_cb,
                                  void *back_udata,
                                  lv_obj_t **out_screen)
{
    lv_obj_t *screen = lv_obj_create(container_parent);
    lv_obj_set_pos(screen, 0, 0);
    lv_obj_set_size(screen, APP_AREA_W, APP_AREA_H);
    style_panel(screen);

    if (out_screen) *out_screen = screen;

    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *title_bar = lv_obj_create(screen);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    style_title_bar(title_bar);
    lv_obj_add_flag(title_bar, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(title_bar, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_event_cb(title_bar, back_cb, LV_EVENT_CLICKED, back_udata);

    char back_text[48];
    snprintf(back_text, sizeof(back_text), "< %s", title);
    lv_obj_t *lbl = lv_label_create(title_bar);
    lv_label_set_text(lbl, back_text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);

    lv_obj_t *content = lv_obj_create(screen);
    lv_obj_set_pos(content, 0, TITLE_BAR_H);
    lv_obj_set_size(content, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(content, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(content, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(content, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(content, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(content, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(content,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(content, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(content, tc->text, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(content, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(content, 0, LV_PART_SCROLLBAR);

    return content;
}

/* Convenience: level-1 sub-screen (back goes to main, stored in s_sub_screen) */
static lv_obj_t *create_sub_screen(const char *title)
{
    return alloc_sub_screen(s_root, title, back_to_main, NULL, &s_sub_screen);
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

/* ------------------------------------------------------------------ */
/* WiFi password dialog                                                 */
/* ------------------------------------------------------------------ */

typedef struct {
    char  ssid[WIFI_SSID_MAX_LEN + 1];
    lv_obj_t *dialog;
    lv_obj_t *ta;
} wifi_pwd_ctx_t;

static wifi_pwd_ctx_t s_wifi_pwd_ctx; /* single-instance, screen lifetime */

static void wifi_pwd_connect_cb(lv_event_t *e)
{
    wifi_pwd_ctx_t *ctx = (wifi_pwd_ctx_t *)lv_event_get_user_data(e);
    if (!ctx) return;

    const char *password = lv_textarea_get_text(ctx->ta);
    ESP_LOGI(TAG, "WiFi: connecting to \"%s\" with password", ctx->ssid);

    if (s_wifi_status_label) {
        lv_label_set_text(s_wifi_status_label, "Status: Connecting...");
    }

    /* Delete dialog before blocking connect call */
    if (ctx->dialog) {
        lv_obj_delete(ctx->dialog);
        ctx->dialog = NULL;
        ctx->ta     = NULL;
    }

    esp_err_t err = wifi_manager_connect(ctx->ssid, password, 10000);
    if (err == ESP_OK) {
        ESP_LOGI(TAG, "WiFi: connected to \"%s\"", ctx->ssid);
        wifi_manager_ntp_sync();
    } else if (err == ESP_ERR_NOT_SUPPORTED) {
        ESP_LOGI(TAG, "WiFi: connect not supported in simulator");
    } else {
        ESP_LOGW(TAG, "WiFi: connect to \"%s\" failed: 0x%x", ctx->ssid, err);
    }
    wifi_update_status_label();
}

static void wifi_pwd_cancel_cb(lv_event_t *e)
{
    wifi_pwd_ctx_t *ctx = (wifi_pwd_ctx_t *)lv_event_get_user_data(e);
    if (!ctx) return;
    if (ctx->dialog) {
        lv_obj_delete(ctx->dialog);
        ctx->dialog = NULL;
        ctx->ta     = NULL;
    }
}

static void show_wifi_password_dialog(const char *ssid)
{
    const theme_colors_t *tc = theme_get_colors();

    /* Fill context */
    strncpy(s_wifi_pwd_ctx.ssid, ssid, sizeof(s_wifi_pwd_ctx.ssid) - 1);
    s_wifi_pwd_ctx.ssid[sizeof(s_wifi_pwd_ctx.ssid) - 1] = '\0';

    /* Overlay on sub_screen */
    lv_obj_t *dialog = lv_obj_create(s_sub_screen);
    lv_obj_set_size(dialog, 280, 140);
    lv_obj_align(dialog, LV_ALIGN_CENTER, 0, 0);
    lv_obj_set_style_bg_color(dialog, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(dialog, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(dialog, 2, LV_PART_MAIN);
    lv_obj_set_style_border_color(dialog, tc->text, LV_PART_MAIN);
    lv_obj_set_style_radius(dialog, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_all(dialog, 6, LV_PART_MAIN);
    lv_obj_clear_flag(dialog, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_move_foreground(dialog);

    s_wifi_pwd_ctx.dialog = dialog;

    /* SSID title */
    lv_obj_t *title = lv_label_create(dialog);
    lv_label_set_text_fmt(title, "Connect to: %s", ssid);
    lv_obj_set_style_text_font(title, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, tc->text, LV_PART_MAIN);
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 2);

    /* Password textarea */
    lv_obj_t *ta = lv_textarea_create(dialog);
    lv_textarea_set_one_line(ta, true);
    lv_textarea_set_password_mode(ta, true);
    lv_textarea_set_placeholder_text(ta, "Password");
    lv_obj_set_width(ta, 250);
    lv_obj_set_style_text_font(ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(ta, tc->text, LV_PART_MAIN);
    lv_obj_set_style_bg_color(ta, tc->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(ta, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(ta, 1, LV_PART_MAIN);
    lv_obj_align(ta, LV_ALIGN_TOP_MID, 0, 26);

    s_wifi_pwd_ctx.ta = ta;

    /* Cancel button */
    lv_obj_t *btn_cancel = lv_obj_create(dialog);
    lv_obj_set_size(btn_cancel, 100, 28);
    lv_obj_align(btn_cancel, LV_ALIGN_BOTTOM_LEFT, 4, -4);
    lv_obj_set_style_bg_color(btn_cancel, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn_cancel, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn_cancel, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_cancel, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_cancel, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_cancel, 0, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn_cancel, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn_cancel, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn_cancel, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn_cancel, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(btn_cancel, wifi_pwd_cancel_cb, LV_EVENT_CLICKED, &s_wifi_pwd_ctx);

    lv_obj_t *lbl_cancel = lv_label_create(btn_cancel);
    lv_label_set_text(lbl_cancel, "Cancel");
    lv_obj_set_style_text_font(lbl_cancel, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_cancel, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_cancel, tc->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl_cancel, LV_ALIGN_CENTER, 0, 0);

    /* Connect button */
    lv_obj_t *btn_connect = lv_obj_create(dialog);
    lv_obj_set_size(btn_connect, 100, 28);
    lv_obj_align(btn_connect, LV_ALIGN_BOTTOM_RIGHT, -4, -4);
    lv_obj_set_style_bg_color(btn_connect, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn_connect, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn_connect, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_connect, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_connect, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_connect, 0, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn_connect, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn_connect, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn_connect, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn_connect, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(btn_connect, wifi_pwd_connect_cb, LV_EVENT_CLICKED, &s_wifi_pwd_ctx);

    lv_obj_t *lbl_connect = lv_label_create(btn_connect);
    lv_label_set_text(lbl_connect, "Connect");
    lv_obj_set_style_text_font(lbl_connect, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_connect, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_connect, tc->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl_connect, LV_ALIGN_CENTER, 0, 0);
}

/* Called when a scan result network row is clicked. */
static void wifi_network_clicked_cb(lv_event_t *e)
{
    wifi_scan_result_t *net = (wifi_scan_result_t *)lv_event_get_user_data(e);
    if (!net) return;

    ESP_LOGI(TAG, "WiFi: attempting connect to \"%s\" (open=%d)", net->ssid, net->is_open);

    if (!net->is_open) {
        /* Secured network — show password dialog */
        show_wifi_password_dialog(net->ssid);
        return;
    }

    /* Open network — connect directly */
    if (s_wifi_status_label) {
        lv_label_set_text(s_wifi_status_label, "Status: Connecting...");
    }

    esp_err_t err = wifi_manager_connect(net->ssid, NULL, 10000);

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

    const theme_colors_t *tc_scan = theme_get_colors();
    for (uint8_t i = 0; i < s_scan_count; i++) {
        wifi_scan_result_t *net = &s_scan_results[i];

        /* Row */
        lv_obj_t *row = lv_obj_create(s_wifi_scan_list);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, tc_scan->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(row, tc_scan->text, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
        lv_obj_set_style_bg_color(row, tc_scan->primary, LV_STATE_PRESSED);
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
        char ssid_text[WIFI_SSID_MAX_LEN + 8];
        if (!net->is_open) {
            snprintf(ssid_text, sizeof(ssid_text), "%s [*]", net->ssid);
        } else {
            snprintf(ssid_text, sizeof(ssid_text), "%s", net->ssid);
        }
        lv_label_set_text(lbl_ssid, ssid_text);
        lv_obj_set_style_text_font(lbl_ssid, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_ssid, tc_scan->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_ssid, tc_scan->bg, LV_STATE_PRESSED);
        lv_obj_set_flex_grow(lbl_ssid, 1);

        /* RSSI label */
        lv_obj_t *lbl_rssi = lv_label_create(row);
        char rssi_text[16];
        snprintf(rssi_text, sizeof(rssi_text), "%d dBm", net->rssi);
        lv_label_set_text(lbl_rssi, rssi_text);
        lv_obj_set_style_text_font(lbl_rssi, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_rssi, tc_scan->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_rssi, tc_scan->bg, LV_STATE_PRESSED);
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

    const theme_colors_t *tc_wifi = theme_get_colors();

    /* Status row */
    lv_obj_t *status_row = lv_obj_create(content);
    lv_obj_set_size(status_row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(status_row, tc_wifi->bg, LV_PART_MAIN);
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
    lv_obj_set_style_text_color(s_wifi_status_label, tc_wifi->text, LV_PART_MAIN);
    lv_obj_align(s_wifi_status_label, LV_ALIGN_LEFT_MID, 0, 0);
    wifi_update_status_label();

    create_separator(content);

    /* Scan Networks button */
    lv_obj_t *btn = lv_obj_create(content);
    lv_obj_set_size(btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(btn, tc_wifi->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, tc_wifi->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, tc_wifi->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(btn, wifi_scan_clicked_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *lbl_btn = lv_label_create(btn);
    lv_label_set_text(lbl_btn, "Scan Networks");
    lv_obj_set_style_text_font(lbl_btn, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_btn, tc_wifi->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_btn, tc_wifi->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl_btn, LV_ALIGN_CENTER, 0, 0);

    create_separator(content);

    /* Scan results list (initially empty, populated by scan button) */
    s_wifi_scan_list = lv_obj_create(content);
    lv_obj_set_width(s_wifi_scan_list, LV_PCT(100));
    lv_obj_set_height(s_wifi_scan_list, LV_SIZE_CONTENT);
    lv_obj_set_style_bg_color(s_wifi_scan_list, tc_wifi->bg, LV_PART_MAIN);
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
/* Bluetooth sub-screen                                                 */
/* ------------------------------------------------------------------ */

static void ble_update_labels(void)
{
    if (!s_ble_status_label) return;

    ble_state_t state = ble_manager_get_state();
    const char *state_str;
    switch (state) {
    case BLE_STATE_ADVERTISING: state_str = "Status: Advertising"; break;
    case BLE_STATE_CONNECTED:   state_str = "Status: Connected";   break;
    default:                    state_str = "Status: Off";         break;
    }
    lv_label_set_text(s_ble_status_label, state_str);

    if (s_ble_name_label) {
        lv_label_set_text(s_ble_name_label, "Name: ThistleOS");
    }

    if (s_ble_peer_label) {
        const char *peer = ble_manager_get_peer_name();
        char peer_buf[64];
        snprintf(peer_buf, sizeof(peer_buf), "Peer: %s", peer ? peer : "None");
        lv_label_set_text(s_ble_peer_label, peer_buf);
        lv_obj_set_style_opa(s_ble_peer_label,
                             (state == BLE_STATE_CONNECTED) ? LV_OPA_COVER : LV_OPA_40,
                             LV_PART_MAIN);
    }

    if (s_ble_toggle_btn_lbl) {
        lv_label_set_text(s_ble_toggle_btn_lbl,
                          (state == BLE_STATE_OFF) ? "Enable" : "Disable");
    }
}

static void ble_toggle_clicked_cb(lv_event_t *e)
{
    (void)e;
    ble_state_t state = ble_manager_get_state();
    if (state == BLE_STATE_OFF) {
        if (!s_ble_inited) {
            esp_err_t err = ble_manager_init("ThistleOS");
            if (err == ESP_OK) {
                s_ble_inited = true;
            } else {
                ESP_LOGW(TAG, "ble_manager_init failed: 0x%x", err);
                return;
            }
        }
        ble_manager_start_advertising();
    } else if (state == BLE_STATE_CONNECTED) {
        ble_manager_disconnect();
    } else {
        ble_manager_stop_advertising();
    }
    ble_update_labels();
}

static void ble_disconnect_clicked_cb(lv_event_t *e)
{
    (void)e;
    ble_manager_disconnect();
    ble_update_labels();
}

static void open_bluetooth_screen(void)
{
    lv_obj_t *content = create_sub_screen("Bluetooth");
    s_current_screen = SETTINGS_BLUETOOTH;

    const theme_colors_t *tc_bt = theme_get_colors();

    /* Status row */
    lv_obj_t *status_row = lv_obj_create(content);
    lv_obj_set_size(status_row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(status_row, tc_bt->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(status_row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(status_row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(status_row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(status_row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(status_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(status_row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    s_ble_status_label = lv_label_create(status_row);
    lv_label_set_text(s_ble_status_label, "Status: Off");
    lv_obj_set_style_text_font(s_ble_status_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ble_status_label, tc_bt->text, LV_PART_MAIN);
    lv_obj_align(s_ble_status_label, LV_ALIGN_LEFT_MID, 0, 0);

    create_separator(content);

    /* Device name row */
    lv_obj_t *name_row = create_info_row(content, "Name: ThistleOS");
    s_ble_name_label = lv_obj_get_child(name_row, 0);

    create_separator(content);

    /* Peer name row (shown dim when not connected) */
    lv_obj_t *peer_row = create_info_row(content, "Peer: None");
    s_ble_peer_label = lv_obj_get_child(peer_row, 0);
    lv_obj_set_style_opa(s_ble_peer_label, LV_OPA_40, LV_PART_MAIN);

    create_separator(content);

    /* Enable / Disable toggle button */
    lv_obj_t *toggle_btn = lv_obj_create(content);
    lv_obj_set_size(toggle_btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(toggle_btn, tc_bt->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(toggle_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(toggle_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(toggle_btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(toggle_btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(toggle_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(toggle_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(toggle_btn, tc_bt->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(toggle_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(toggle_btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(toggle_btn, tc_bt->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(toggle_btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(toggle_btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(toggle_btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(toggle_btn, ble_toggle_clicked_cb, LV_EVENT_CLICKED, NULL);

    s_ble_toggle_btn_lbl = lv_label_create(toggle_btn);
    lv_label_set_text(s_ble_toggle_btn_lbl, "Enable");
    lv_obj_set_style_text_font(s_ble_toggle_btn_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ble_toggle_btn_lbl, tc_bt->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ble_toggle_btn_lbl, tc_bt->bg, LV_STATE_PRESSED);
    lv_obj_align(s_ble_toggle_btn_lbl, LV_ALIGN_CENTER, 0, 0);

    create_separator(content);

    /* Disconnect button (dimmed when not connected) */
    lv_obj_t *disc_btn = lv_obj_create(content);
    lv_obj_set_size(disc_btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(disc_btn, tc_bt->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(disc_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(disc_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(disc_btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(disc_btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(disc_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(disc_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(disc_btn, tc_bt->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(disc_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(disc_btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(disc_btn, tc_bt->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(disc_btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_set_style_opa(disc_btn,
                         (ble_manager_get_state() == BLE_STATE_CONNECTED)
                             ? LV_OPA_COVER : LV_OPA_40,
                         LV_PART_MAIN);
    lv_obj_add_flag(disc_btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(disc_btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(disc_btn, ble_disconnect_clicked_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *lbl_disc = lv_label_create(disc_btn);
    lv_label_set_text(lbl_disc, "Disconnect");
    lv_obj_set_style_text_font(lbl_disc, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_disc, tc_bt->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_disc, tc_bt->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl_disc, LV_ALIGN_CENTER, 0, 0);

    ble_update_labels();
}

/* ------------------------------------------------------------------ */
/* Appearance sub-screen                                                */
/* ------------------------------------------------------------------ */

#define APPEARANCE_MAX_THEMES 8

/* Payload type for theme selection rows */
typedef struct {
    char name[32]; /* theme filename, e.g. "dark.json" */
} appearance_theme_payload_t;

static appearance_theme_payload_t s_theme_payloads[APPEARANCE_MAX_THEMES + 1]; /* +1 for Default */

static void theme_selected_cb(lv_event_t *e)
{
    const char *theme_name = (const char *)lv_event_get_user_data(e);
    if (!theme_name) return;

    if (strcmp(theme_name, "__default__") == 0) {
        /* Load the built-in default monochrome theme from SD if available,
         * otherwise reinitialise the default without a display argument.
         * theme_init(NULL) skips display wiring but re-applies styles. */
        esp_err_t ret = theme_load("/sdcard/themes/default.json");
        if (ret != ESP_OK) {
            theme_init(NULL);
            statusbar_refresh_theme();
        }
        toast_info("Default theme applied");
    } else {
        char path[72];
        snprintf(path, sizeof(path), "/sdcard/themes/%s", theme_name);
        esp_err_t ret = theme_load(path);
        if (ret == ESP_OK) {
            toast_info("Theme applied");
        } else {
            toast_warn("Failed to load theme");
        }
    }
}

static void wallpaper_browse_cb(lv_event_t *e)
{
    (void)e;
    /* File browser not yet implemented */
    toast_info("File browser: coming soon");
}

static void wallpaper_clear_cb(lv_event_t *e)
{
    (void)e;
    /* Nothing to clear yet — placeholder */
    toast_info("Wallpaper cleared");
}

/* Create a clickable theme-selection row showing [x] or [ ] indicator */
static void create_theme_row(lv_obj_t *content, const char *display_name,
                             const char *payload_name, bool is_active)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *row = lv_obj_create(content);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT + 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
    lv_obj_set_style_bg_color(row, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, theme_selected_cb, LV_EVENT_CLICKED, (void *)payload_name);

    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, display_name);
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, tc->bg, LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    lv_obj_t *lbl_sel = lv_label_create(row);
    lv_label_set_text(lbl_sel, is_active ? "[x]" : "[ ]");
    lv_obj_set_style_text_font(lbl_sel, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_sel, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_sel, tc->bg, LV_STATE_PRESSED);
}

/* Create a clickable action button row (for Browse / Clear wallpaper) */
static void create_action_row(lv_obj_t *content, const char *label,
                               lv_event_cb_t cb)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *btn = lv_obj_create(content);
    lv_obj_set_size(btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(btn, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_event_cb(btn, cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->bg, LV_STATE_PRESSED);
    lv_obj_align(lbl, LV_ALIGN_CENTER, 0, 0);
}

static void open_appearance_screen(void)
{
    lv_obj_t *content = create_sub_screen("Appearance");
    s_current_screen = SETTINGS_APPEARANCE;

    /* ---- Theme section header ---- */
    create_info_row(content, "Theme");
    create_separator(content);

    /* Use theme_get_current_name() to determine active theme.
     * Default name is "Default"; SD themes are stored as e.g. "dark.json". */
    const char *current_name = theme_get_current_name();
    bool default_active = (strcmp(current_name, "Default") == 0);

    /* Store "__default__" in the first payload slot */
    strncpy(s_theme_payloads[0].name, "__default__", sizeof(s_theme_payloads[0].name) - 1);
    create_theme_row(content, "Default", s_theme_payloads[0].name, default_active);

    /* Enumerate themes from /sdcard/themes */
    char theme_names[APPEARANCE_MAX_THEMES][32];
    int theme_count = theme_list_available(theme_names, APPEARANCE_MAX_THEMES);

    for (int i = 0; i < theme_count; i++) {
        /* Build display name: strip ".json" suffix */
        char display[32];
        strncpy(display, theme_names[i], sizeof(display) - 1);
        display[sizeof(display) - 1] = '\0';
        size_t dlen = strlen(display);
        if (dlen > 5 && strcmp(display + dlen - 5, ".json") == 0) {
            display[dlen - 5] = '\0';
        }

        /* Copy filename into persistent payload storage */
        int slot = i + 1; /* slot 0 is Default */
        if (slot >= APPEARANCE_MAX_THEMES + 1) break;
        strncpy(s_theme_payloads[slot].name, theme_names[i],
                sizeof(s_theme_payloads[slot].name) - 1);
        s_theme_payloads[slot].name[sizeof(s_theme_payloads[slot].name) - 1] = '\0';

        /* Mark active if this filename matches the currently loaded theme */
        bool active = (strcmp(current_name, theme_names[i]) == 0);

        create_theme_row(content, display, s_theme_payloads[slot].name, active);
    }

    if (theme_count == 0) {
        create_info_row(content, "  (no themes on SD card)");
    }

    create_separator(content);

    /* ---- Wallpaper section header ---- */
    create_info_row(content, "Wallpaper");
    create_separator(content);

    create_info_row(content, "  (current: none)");
    create_separator(content);

    create_action_row(content, "Browse SD Card", wallpaper_browse_cb);
    create_separator(content);
    create_action_row(content, "Clear Wallpaper", wallpaper_clear_cb);
}

/* ------------------------------------------------------------------ */
/* About sub-screen                                                     */
/* ------------------------------------------------------------------ */

static void install_update_cb(lv_event_t *e)
{
    (void)e;

    /* Show progress toast */
    toast_show("Installing update... Do not power off!", TOAST_WARNING, 30000);

    /* Verify signature first */
    const char *update_path = THISTLE_SDCARD "/update/thistle_os.bin";
    esp_err_t sig_ret = signing_verify_file(update_path);
    if (sig_ret == ESP_ERR_INVALID_CRC) {
        toast_warn("Update signature INVALID — rejected!");
        return;
    }
    /* Missing signature is OK for dev builds */
    if (sig_ret == ESP_ERR_NOT_FOUND) {
        ESP_LOGW(TAG, "Update is unsigned — proceeding anyway");
    }

    /* Apply the update (this reboots on success) */
    esp_err_t ret = ota_apply_from_sd(NULL, NULL);
    if (ret != ESP_OK) {
        toast_warn("Update failed!");
    }
    /* If we get here, the update failed — ota_apply_from_sd reboots on success */
}

static void open_about_screen(void)
{
    lv_obj_t *content = create_sub_screen("About");
    s_current_screen = SETTINGS_ABOUT;

    const theme_colors_t *tc_about = theme_get_colors();

    /* ThistleOS title */
    lv_obj_t *title_row = lv_obj_create(content);
    lv_obj_set_size(title_row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(title_row, tc_about->bg, LV_PART_MAIN);
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
    lv_obj_set_style_text_color(lbl_os, tc_about->text, LV_PART_MAIN);
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

    create_separator(content);

    /* Signing key (truncated) */
    const char *key_hex = signing_get_public_key_hex();
    char sign_buf[48];
    snprintf(sign_buf, sizeof(sign_buf), "Signing key: %.16s...", key_hex);
    create_info_row(content, sign_buf);

    /* Runtime drivers loaded from SD card */
    char drv_buf[64];
    int rt_drv_count = driver_loader_get_count();
    if (rt_drv_count > 0) {
        snprintf(drv_buf, sizeof(drv_buf), "Runtime drivers: %d loaded from SD", rt_drv_count);
    } else {
        snprintf(drv_buf, sizeof(drv_buf), "Runtime drivers: none");
    }
    create_info_row(content, drv_buf);

    /* Check for SD card update */
    if (ota_sd_update_available()) {
        const theme_colors_t *colors = theme_get_colors();

        /* Separator */
        create_separator(content);

        /* Update available banner */
        lv_obj_t *update_row = lv_obj_create(content);
        lv_obj_set_size(update_row, LV_PCT(100), 50);
        lv_obj_set_style_bg_color(update_row, colors->primary, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(update_row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(update_row, 6, LV_PART_MAIN);
        lv_obj_set_style_pad_all(update_row, 8, LV_PART_MAIN);
        lv_obj_clear_flag(update_row, LV_OBJ_FLAG_SCROLLABLE);

        lv_obj_t *update_lbl = lv_label_create(update_row);
        lv_label_set_text(update_lbl, "Update found on SD card");
        lv_obj_set_style_text_color(update_lbl, lv_color_white(), LV_PART_MAIN);
        lv_obj_set_style_text_font(update_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_align(update_lbl, LV_ALIGN_LEFT_MID, 0, -8);

        lv_obj_t *install_btn = lv_button_create(update_row);
        lv_obj_set_size(install_btn, 100, 28);
        lv_obj_align(install_btn, LV_ALIGN_RIGHT_MID, 0, 8);
        lv_obj_set_style_bg_color(install_btn, lv_color_white(), LV_PART_MAIN);

        lv_obj_t *btn_lbl = lv_label_create(install_btn);
        lv_label_set_text(btn_lbl, "Install");
        lv_obj_set_style_text_color(btn_lbl, colors->primary, LV_PART_MAIN);
        lv_obj_center(btn_lbl);

        lv_obj_add_event_cb(install_btn, install_update_cb, LV_EVENT_CLICKED, NULL);
    }
}

/* ------------------------------------------------------------------ */
/* Driver detail screens                                                */
/* ------------------------------------------------------------------ */

/* --- Display --- */

static void display_refresh_mode_cb(lv_event_t *e)
{
    hal_display_refresh_mode_t mode =
        (hal_display_refresh_mode_t)(uintptr_t)lv_event_get_user_data(e);
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->display && reg->display->set_refresh_mode) {
        reg->display->set_refresh_mode(mode);
        ESP_LOGI(TAG, "Display: refresh mode -> %d", (int)mode);
    }
}

static void open_detail_display(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->display) {
        create_info_row(content, "No display registered");
        return;
    }
    const hal_display_driver_t *d = reg->display;
    char buf[64];

    snprintf(buf, sizeof(buf), "Name: %s", d->name ? d->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    snprintf(buf, sizeof(buf), "Type: %s",
             d->type == HAL_DISPLAY_TYPE_EPAPER ? "E-Paper" : "LCD");
    create_info_row(content, buf);
    create_separator(content);

    snprintf(buf, sizeof(buf), "Resolution: %u x %u", d->width, d->height);
    create_info_row(content, buf);
    create_separator(content);

    if (d->set_refresh_mode) {
        create_info_row(content, "Refresh mode:");
        create_separator(content);

        const theme_colors_t *tc_disp = theme_get_colors();
        lv_obj_t *btn_row = lv_obj_create(content);
        lv_obj_set_size(btn_row, LV_PCT(100), ITEM_H + 4);
        lv_obj_set_style_bg_color(btn_row, tc_disp->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(btn_row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(btn_row, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_right(btn_row, ITEM_PAD_RIGHT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(btn_row, 2, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(btn_row, 2, LV_PART_MAIN);
        lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_set_flex_flow(btn_row, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(btn_row,
                              LV_FLEX_ALIGN_START,
                              LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(btn_row, 6, LV_PART_MAIN);

        static const struct {
            const char *label;
            hal_display_refresh_mode_t mode;
        } modes[3] = {
            { "Full",    HAL_DISPLAY_REFRESH_FULL    },
            { "Partial", HAL_DISPLAY_REFRESH_PARTIAL },
            { "Fast",    HAL_DISPLAY_REFRESH_FAST    },
        };

        for (int i = 0; i < 3; i++) {
            lv_obj_t *b = lv_obj_create(btn_row);
            lv_obj_set_size(b, LV_SIZE_CONTENT, ITEM_H - 4);
            lv_obj_set_style_bg_color(b, tc_disp->bg, LV_PART_MAIN);
            lv_obj_set_style_bg_opa(b, LV_OPA_COVER, LV_PART_MAIN);
            lv_obj_set_style_border_color(b, tc_disp->text, LV_PART_MAIN);
            lv_obj_set_style_border_width(b, 1, LV_PART_MAIN);
            lv_obj_set_style_border_side(b, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
            lv_obj_set_style_radius(b, 0, LV_PART_MAIN);
            lv_obj_set_style_pad_left(b, 6, LV_PART_MAIN);
            lv_obj_set_style_pad_right(b, 6, LV_PART_MAIN);
            lv_obj_set_style_pad_top(b, 0, LV_PART_MAIN);
            lv_obj_set_style_pad_bottom(b, 0, LV_PART_MAIN);
            lv_obj_set_style_bg_color(b, tc_disp->primary, LV_STATE_PRESSED);
            lv_obj_set_style_bg_opa(b, LV_OPA_COVER, LV_STATE_PRESSED);
            lv_obj_add_flag(b, LV_OBJ_FLAG_CLICKABLE);
            lv_obj_clear_flag(b, LV_OBJ_FLAG_SCROLLABLE);
            lv_obj_add_event_cb(b, display_refresh_mode_cb, LV_EVENT_CLICKED,
                                (void *)(uintptr_t)modes[i].mode);

            lv_obj_t *bl = lv_label_create(b);
            lv_label_set_text(bl, modes[i].label);
            lv_obj_set_style_text_font(bl, &lv_font_montserrat_14, LV_PART_MAIN);
            lv_obj_set_style_text_color(bl, tc_disp->text, LV_PART_MAIN);
            lv_obj_set_style_text_color(bl, tc_disp->bg, LV_STATE_PRESSED);
            lv_obj_align(bl, LV_ALIGN_CENTER, 0, 0);
        }
        create_separator(content);
    }

    if (d->set_brightness) {
        create_info_row(content, "Brightness: use brightness keys");
        create_separator(content);
    }
}

/* --- Radio --- */

static void open_detail_radio(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio) {
        create_info_row(content, "No radio registered");
        return;
    }
    const hal_radio_driver_t *r = reg->radio;
    char buf[64];

    snprintf(buf, sizeof(buf), "Name: %s", r->name ? r->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    create_info_row(content, "Protocol: LoRa");
    create_separator(content);

    if (r->get_rssi) {
        int rssi = r->get_rssi();
        snprintf(buf, sizeof(buf), "RSSI: %d dBm", rssi);
        create_info_row(content, buf);
        create_separator(content);
    }

    create_info_row(content, "Parameters set at board init.");
    create_separator(content);
    create_info_row(content, "(Editing: future work)");
}

/* --- GPS (with live timer) --- */

static void gps_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->gps || !s_gps_status_lbl) return;

    hal_gps_position_t pos;
    esp_err_t err = reg->gps->get_position(&pos);
    char buf[48];

    if (err != ESP_OK) {
        lv_label_set_text(s_gps_status_lbl, "Status: Error");
        lv_label_set_text(s_gps_lat_lbl,    "Lat: --");
        lv_label_set_text(s_gps_lon_lbl,    "Lon: --");
        lv_label_set_text(s_gps_sat_lbl,    "Satellites: --");
        return;
    }

    if (pos.fix_valid) {
        lv_label_set_text(s_gps_status_lbl, "Status: Fix");
        snprintf(buf, sizeof(buf), "Lat: %.6f", pos.latitude);
        lv_label_set_text(s_gps_lat_lbl, buf);
        snprintf(buf, sizeof(buf), "Lon: %.6f", pos.longitude);
        lv_label_set_text(s_gps_lon_lbl, buf);
    } else {
        lv_label_set_text(s_gps_status_lbl, "Status: No fix");
        lv_label_set_text(s_gps_lat_lbl,    "Lat: --");
        lv_label_set_text(s_gps_lon_lbl,    "Lon: --");
    }
    snprintf(buf, sizeof(buf), "Satellites: %u", pos.satellites);
    lv_label_set_text(s_gps_sat_lbl, buf);
}

static void gps_toggle_cb(lv_event_t *e)
{
    lv_obj_t *btn_lbl = (lv_obj_t *)lv_event_get_user_data(e);
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->gps) return;

    if (s_gps_enabled) {
        if (reg->gps->disable) reg->gps->disable();
        s_gps_enabled = false;
        if (btn_lbl) lv_label_set_text(btn_lbl, "Enable");
        ESP_LOGI(TAG, "GPS disabled");
    } else {
        if (reg->gps->enable) reg->gps->enable();
        s_gps_enabled = true;
        if (btn_lbl) lv_label_set_text(btn_lbl, "Disable");
        ESP_LOGI(TAG, "GPS enabled");
    }
}

static void open_detail_gps(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->gps) {
        create_info_row(content, "No GPS registered");
        return;
    }
    char buf[64];
    snprintf(buf, sizeof(buf), "Name: %s",
             reg->gps->name ? reg->gps->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    s_gps_status_lbl = create_live_row(content, "Status: ...");
    create_separator(content);
    s_gps_lat_lbl    = create_live_row(content, "Lat: --");
    create_separator(content);
    s_gps_lon_lbl    = create_live_row(content, "Lon: --");
    create_separator(content);
    s_gps_sat_lbl    = create_live_row(content, "Satellites: --");
    create_separator(content);

    /* Enable / Disable button */
    const theme_colors_t *tc_gps = theme_get_colors();
    lv_obj_t *btn = lv_obj_create(content);
    lv_obj_set_size(btn, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(btn, tc_gps->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(btn, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(btn, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, tc_gps->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_side(btn, LV_BORDER_SIDE_FULL, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, tc_gps->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *bl = lv_label_create(btn);
    lv_label_set_text(bl, s_gps_enabled ? "Disable" : "Enable");
    lv_obj_set_style_text_font(bl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(bl, tc_gps->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(bl, tc_gps->bg, LV_STATE_PRESSED);
    lv_obj_align(bl, LV_ALIGN_CENTER, 0, 0);
    lv_obj_add_event_cb(btn, gps_toggle_cb, LV_EVENT_CLICKED, bl);

    /* Live update every 2 s */
    s_gps_timer = lv_timer_create(gps_timer_cb, 2000, NULL);
    lv_timer_ready(s_gps_timer);
}

/* --- Power (live timer, 5 s) --- */

static void power_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->power || !s_power_batt_lbl) return;

    hal_power_info_t info = {0};
    char buf[48];

    if (reg->power->get_info) {
        if (reg->power->get_info(&info) != ESP_OK) {
            lv_label_set_text(s_power_batt_lbl,  "Battery: Error");
            lv_label_set_text(s_power_pct_lbl,   "Charge: --");
            lv_label_set_text(s_power_state_lbl, "State: Error");
            return;
        }
    } else {
        if (reg->power->get_battery_mv)      info.voltage_mv = reg->power->get_battery_mv();
        if (reg->power->get_battery_percent) info.percent    = reg->power->get_battery_percent();
        if (reg->power->is_charging) {
            info.state = reg->power->is_charging()
                         ? HAL_POWER_STATE_CHARGING
                         : HAL_POWER_STATE_DISCHARGING;
        }
    }

    snprintf(buf, sizeof(buf), "Battery: %u mV", info.voltage_mv);
    lv_label_set_text(s_power_batt_lbl, buf);

    snprintf(buf, sizeof(buf), "Charge: %u%%", info.percent);
    lv_label_set_text(s_power_pct_lbl, buf);

    const char *state_str;
    switch (info.state) {
    case HAL_POWER_STATE_CHARGING:   state_str = "Charging";    break;
    case HAL_POWER_STATE_CHARGED:    state_str = "Charged";     break;
    case HAL_POWER_STATE_NO_BATTERY: state_str = "No Battery";  break;
    default:                         state_str = "Discharging"; break;
    }
    snprintf(buf, sizeof(buf), "State: %s", state_str);
    lv_label_set_text(s_power_state_lbl, buf);
}

static void open_detail_power(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->power) {
        create_info_row(content, "No power driver registered");
        return;
    }

    char buf[64];
    snprintf(buf, sizeof(buf), "Name: %s",
             reg->power->name ? reg->power->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    s_power_batt_lbl  = create_live_row(content, "Battery: ...");
    create_separator(content);
    s_power_pct_lbl   = create_live_row(content, "Charge: ...");
    create_separator(content);
    s_power_state_lbl = create_live_row(content, "State: ...");

    /* Live update every 5 s, fire immediately */
    s_power_timer = lv_timer_create(power_timer_cb, 5000, NULL);
    lv_timer_ready(s_power_timer);
}

/* --- Audio --- */

static void open_detail_audio(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->audio) {
        create_info_row(content, "No audio driver registered");
        return;
    }
    char buf[64];
    snprintf(buf, sizeof(buf), "Name: %s",
             reg->audio->name ? reg->audio->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    create_info_row(content, "Volume: 100%");
    create_separator(content);
    create_info_row(content, "(Volume control: future work)");
}

/* --- Storage --- */

static void open_detail_storage(lv_obj_t *content, int index)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || index < 0 || index >= (int)reg->storage_count
            || !reg->storage[index]) {
        create_info_row(content, "No storage driver registered");
        return;
    }
    const hal_storage_driver_t *st = reg->storage[index];
    char buf[64];

    snprintf(buf, sizeof(buf), "Name: %s", st->name ? st->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    snprintf(buf, sizeof(buf), "Type: %s",
             st->type == HAL_STORAGE_TYPE_SD ? "SD Card" : "Internal");
    create_info_row(content, buf);
    create_separator(content);

    bool mounted = st->is_mounted ? st->is_mounted() : false;
    snprintf(buf, sizeof(buf), "Mounted: %s", mounted ? "Yes" : "No");
    create_info_row(content, buf);
    create_separator(content);

    if (mounted && st->get_total_bytes && st->get_free_bytes) {
        uint64_t total  = st->get_total_bytes();
        uint64_t free_b = st->get_free_bytes();
        uint64_t used   = (total > free_b) ? (total - free_b) : 0;

#define FMT_BYTES(v, out_buf) \
    do { \
        if ((v) >= (1024ULL * 1024 * 1024)) { \
            snprintf((out_buf), sizeof(out_buf), "%.1f GB", \
                     (double)(v) / (1024.0 * 1024.0 * 1024.0)); \
        } else { \
            snprintf((out_buf), sizeof(out_buf), "%.1f MB", \
                     (double)(v) / (1024.0 * 1024.0)); \
        } \
    } while (0)

        char sz[24];
        FMT_BYTES(total, sz);
        snprintf(buf, sizeof(buf), "Total: %s", sz);
        create_info_row(content, buf);
        create_separator(content);

        FMT_BYTES(free_b, sz);
        snprintf(buf, sizeof(buf), "Free: %s", sz);
        create_info_row(content, buf);
        create_separator(content);

        uint8_t used_pct = (total > 0) ? (uint8_t)((used * 100ULL) / total) : 0;
        FMT_BYTES(used, sz);
        snprintf(buf, sizeof(buf), "Used: %s (%u%%)", sz, used_pct);
        create_info_row(content, buf);

#undef FMT_BYTES
    } else if (!mounted) {
        create_info_row(content, "(Not mounted)");
    }
}

/* --- Input --- */

static void open_detail_input(lv_obj_t *content, int index)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || index < 0 || index >= (int)reg->input_count
            || !reg->inputs[index]) {
        create_info_row(content, "No input driver registered");
        return;
    }
    const hal_input_driver_t *inp = reg->inputs[index];
    char buf[64];

    snprintf(buf, sizeof(buf), "Name: %s", inp->name ? inp->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    snprintf(buf, sizeof(buf), "Type: %s", inp->is_touch ? "Touch" : "Keyboard");
    create_info_row(content, buf);
    create_separator(content);

    create_info_row(content, "(Info only — no settings)");
}

/* --- IMU --- */

static void open_detail_imu(lv_obj_t *content)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->imu) {
        create_info_row(content, "No IMU registered");
        return;
    }
    const hal_imu_driver_t *imu = reg->imu;
    char buf[64];

    snprintf(buf, sizeof(buf), "Name: %s", imu->name ? imu->name : "Unknown");
    create_info_row(content, buf);
    create_separator(content);

    if (imu->get_data) {
        hal_imu_data_t data = {0};
        if (imu->get_data(&data) == ESP_OK) {
            snprintf(buf, sizeof(buf), "Accel X: %.2f m/s2", (double)data.accel_x);
            create_info_row(content, buf);
            create_separator(content);
            snprintf(buf, sizeof(buf), "Accel Y: %.2f m/s2", (double)data.accel_y);
            create_info_row(content, buf);
            create_separator(content);
            snprintf(buf, sizeof(buf), "Accel Z: %.2f m/s2", (double)data.accel_z);
            create_info_row(content, buf);
            create_separator(content);
            snprintf(buf, sizeof(buf), "Gyro X: %.2f deg/s", (double)data.gyro_x);
            create_info_row(content, buf);
            create_separator(content);
            snprintf(buf, sizeof(buf), "Gyro Y: %.2f deg/s", (double)data.gyro_y);
            create_info_row(content, buf);
            create_separator(content);
            snprintf(buf, sizeof(buf), "Gyro Z: %.2f deg/s", (double)data.gyro_z);
            create_info_row(content, buf);
        } else {
            create_info_row(content, "(Read error)");
        }
    } else {
        create_info_row(content, "(Info only — no settings)");
    }
}

/* ------------------------------------------------------------------ */
/* Driver detail dispatcher                                             */
/* ------------------------------------------------------------------ */

static void open_driver_detail(driver_type_t type, int index)
{
    static const char *type_names[] = {
        "Display", "Input", "Radio", "GPS", "Audio", "Power", "IMU", "Storage",
    };
    const char *title = ((unsigned)type < (sizeof(type_names)/sizeof(type_names[0])))
                        ? type_names[type] : "Driver";

    /* Hide drivers list; build detail on s_root */
    if (s_sub_screen) {
        lv_obj_add_flag(s_sub_screen, LV_OBJ_FLAG_HIDDEN);
    }

    lv_obj_t *outer = NULL;
    lv_obj_t *content = alloc_sub_screen(s_root, title,
                                          back_to_drivers, NULL, &outer);
    s_detail_screen  = outer;
    s_current_screen = SETTINGS_DRIVER_DETAIL;

    switch (type) {
    case DRIVER_TYPE_DISPLAY:  open_detail_display(content);        break;
    case DRIVER_TYPE_INPUT:    open_detail_input(content, index);   break;
    case DRIVER_TYPE_RADIO:    open_detail_radio(content);          break;
    case DRIVER_TYPE_GPS:      open_detail_gps(content);            break;
    case DRIVER_TYPE_AUDIO:    open_detail_audio(content);          break;
    case DRIVER_TYPE_POWER:    open_detail_power(content);          break;
    case DRIVER_TYPE_IMU:      open_detail_imu(content);            break;
    case DRIVER_TYPE_STORAGE:  open_detail_storage(content, index); break;
    }
}

/* ------------------------------------------------------------------ */
/* Drivers list screen                                                  */
/* ------------------------------------------------------------------ */

static void driver_row_clicked_cb(lv_event_t *e)
{
    driver_row_data_t *d = (driver_row_data_t *)lv_event_get_user_data(e);
    if (!d) return;
    open_driver_detail(d->type, d->index);
}

static void add_driver_row(lv_obj_t *list,
                            const char *name,
                            const char *type_label,
                            driver_type_t dtype,
                            int index)
{
    if (s_driver_row_pool_used >= MAX_DRIVER_ROWS) {
        ESP_LOGW(TAG, "driver row pool exhausted");
        return;
    }

    driver_row_data_t *payload = &s_driver_row_pool[s_driver_row_pool_used++];
    payload->type  = dtype;
    payload->index = index;

    const theme_colors_t *tc_drv = theme_get_colors();
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, tc_drv->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, tc_drv->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
    lv_obj_set_style_bg_color(row, tc_drv->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, driver_row_clicked_cb, LV_EVENT_CLICKED, payload);

    /* Driver name (grows) */
    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, name ? name : "(unknown)");
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, tc_drv->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, tc_drv->bg, LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    /* Type label */
    lv_obj_t *lbl_type = lv_label_create(row);
    lv_label_set_text(lbl_type, type_label ? type_label : "");
    lv_obj_set_style_text_font(lbl_type, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, tc_drv->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, tc_drv->bg, LV_STATE_PRESSED);

    /* Chevron */
    lv_obj_t *lbl_chev = lv_label_create(row);
    lv_label_set_text(lbl_chev, ">");
    lv_obj_set_style_text_font(lbl_chev, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chev, tc_drv->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chev, tc_drv->bg, LV_STATE_PRESSED);
}

static void open_drivers_screen(void)
{
    s_driver_row_pool_used = 0;

    lv_obj_t *content = create_sub_screen("Drivers");
    s_current_screen = SETTINGS_DRIVERS;

    const hal_registry_t *reg = hal_get_registry();
    if (!reg) {
        create_info_row(content, "HAL registry not available");
        return;
    }

    bool any = false;

    if (reg->display) {
        add_driver_row(content, reg->display->name, "Display", DRIVER_TYPE_DISPLAY, 0);
        any = true;
    }

    for (int i = 0; i < (int)reg->input_count; i++) {
        if (!reg->inputs[i]) continue;
        const char *tlbl = reg->inputs[i]->is_touch ? "Touch" : "Keyboard";
        add_driver_row(content, reg->inputs[i]->name, tlbl, DRIVER_TYPE_INPUT, i);
        any = true;
    }

    if (reg->radio) {
        add_driver_row(content, reg->radio->name, "Radio", DRIVER_TYPE_RADIO, 0);
        any = true;
    }

    if (reg->gps) {
        add_driver_row(content, reg->gps->name, "GPS", DRIVER_TYPE_GPS, 0);
        any = true;
    }

    if (reg->audio) {
        add_driver_row(content, reg->audio->name, "Audio", DRIVER_TYPE_AUDIO, 0);
        any = true;
    }

    if (reg->power) {
        add_driver_row(content, reg->power->name, "Power", DRIVER_TYPE_POWER, 0);
        any = true;
    }

    if (reg->imu) {
        add_driver_row(content, reg->imu->name, "IMU", DRIVER_TYPE_IMU, 0);
        any = true;
    }

    for (int i = 0; i < (int)reg->storage_count; i++) {
        if (!reg->storage[i]) continue;
        add_driver_row(content, reg->storage[i]->name, "Storage", DRIVER_TYPE_STORAGE, i);
        any = true;
    }

    if (!any) {
        create_info_row(content, "No drivers registered");
    }
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
    } else if (strcmp(name, "Bluetooth") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_bluetooth_screen();
    } else if (strcmp(name, "Appearance") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_appearance_screen();
    } else if (strcmp(name, "Drivers") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_drivers_screen();
    } else if (strcmp(name, "About") == 0) {
        lv_obj_add_flag(s_main_list, LV_OBJ_FLAG_HIDDEN);
        open_about_screen();
    } else {
        ESP_LOGI(TAG, "Settings: %s — not implemented yet", name);
    }
}

static void create_list_item(lv_obj_t *list, const settings_item_t *item)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
    lv_obj_set_style_bg_color(row, tc->primary, LV_STATE_PRESSED);
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
    lv_obj_set_style_text_color(lbl_name, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, tc->bg, LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    /* Value label (optional) — capture pointers for WiFi and BT */
    if (item->value != NULL) {
        lv_obj_t *lbl_val = lv_label_create(row);
        lv_label_set_text(lbl_val, item->value);
        lv_obj_set_style_text_font(lbl_val, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, tc->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, tc->bg, LV_STATE_PRESSED);

        /* Store references for live-update timer */
        if (strcmp(item->name, "WiFi") == 0) {
            s_wifi_value_label = lbl_val;
        } else if (strcmp(item->name, "Bluetooth") == 0) {
            s_bt_value_label = lbl_val;
        }
    }

    /* Chevron */
    lv_obj_t *lbl_chevron = lv_label_create(row);
    lv_label_set_text(lbl_chevron, ">");
    lv_obj_set_style_text_font(lbl_chevron, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, tc->bg, LV_STATE_PRESSED);
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

    const theme_colors_t *tc_main = theme_get_colors();

    lv_obj_t *lbl_title = lv_label_create(title_bar);
    lv_label_set_text(lbl_title, "Settings");
    lv_obj_set_style_text_font(lbl_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_title, tc_main->text, LV_PART_MAIN);
    lv_obj_align(lbl_title, LV_ALIGN_LEFT_MID, 0, 0);

    /* ----------------------------------------------------------------
     * Scrollable list container below title bar
     * ---------------------------------------------------------------- */
    s_main_list = lv_obj_create(s_root);
    lv_obj_set_pos(s_main_list, 0, TITLE_BAR_H);
    lv_obj_set_size(s_main_list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(s_main_list, tc_main->bg, LV_PART_MAIN);
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
    lv_obj_set_style_bg_color(s_main_list, tc_main->text, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_main_list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_main_list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_main_list, 0, LV_PART_SCROLLBAR);

    /* Reset value label pointers before rebuilding */
    s_wifi_value_label = NULL;
    s_bt_value_label   = NULL;

    for (size_t i = 0; i < ITEMS_COUNT; i++) {
        create_list_item(s_main_list, &s_items[i]);
    }

    s_current_screen = SETTINGS_MAIN;
    s_sub_screen     = NULL;
    s_detail_screen  = NULL;

    /* Start the main list live-update timer (2 s interval) */
    if (s_main_timer) {
        lv_timer_delete(s_main_timer);
    }
    s_main_timer = lv_timer_create(settings_main_update_timer, 2000, NULL);

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

void settings_ui_destroy(void)
{
    /* Stop all live-update timers */
    if (s_main_timer) {
        lv_timer_delete(s_main_timer);
        s_main_timer = NULL;
    }
    if (s_power_timer) {
        lv_timer_delete(s_power_timer);
        s_power_timer     = NULL;
        s_power_batt_lbl  = NULL;
        s_power_pct_lbl   = NULL;
        s_power_state_lbl = NULL;
    }
    if (s_gps_timer) {
        lv_timer_delete(s_gps_timer);
        s_gps_timer      = NULL;
        s_gps_status_lbl = NULL;
        s_gps_lat_lbl    = NULL;
        s_gps_lon_lbl    = NULL;
        s_gps_sat_lbl    = NULL;
    }

    /* Nullify label pointers — LVGL will clean up widgets when s_root is deleted */
    s_wifi_value_label   = NULL;
    s_bt_value_label     = NULL;
    s_wifi_status_label  = NULL;
    s_wifi_scan_list     = NULL;
    s_ble_status_label   = NULL;
    s_ble_name_label     = NULL;
    s_ble_peer_label     = NULL;
    s_ble_toggle_btn_lbl = NULL;

    if (s_root) {
        lv_obj_delete(s_root);
        s_root          = NULL;
        s_main_list     = NULL;
        s_sub_screen    = NULL;
        s_detail_screen = NULL;
    }

    s_current_screen       = SETTINGS_MAIN;
    s_driver_row_pool_used = 0;
}
