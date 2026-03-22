/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Weather Station UI
 *
 * Reads IMU (BHI260AP) accelerometer/gyro if available.
 * Reads GPS altitude if available.
 * All other fields (pressure, humidity, temperature) show placeholders
 * until hardware or API support is added.
 *
 * Layout (320x216 app area):
 *   ┌────────────────────────────────┐
 *   │  Weather Station               │  30 px header
 *   ├────────────────────────────────┤
 *   │  Temperature:  --°C            │
 *   │  Pressure:     -- hPa          │
 *   │  Altitude:     -- m (baro)     │
 *   │  ─────────────────────────────  │
 *   │  Trend: -- (stable/rising/     │
 *   │               falling)         │
 *   │  ─────────────────────────────  │
 *   │  Humidity:     --              │
 *   │  ─────────────────────────────  │
 *   │  GPS Alt:      -- m            │
 *   │  ─────────────────────────────  │
 *   │  Updated: --:--                │
 *   │  [Refresh]                     │
 *   └────────────────────────────────┘
 */
#include "weather/weather_app.h"

#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"
#include "hal/board.h"
#include "hal/imu.h"
#include "hal/gps.h"

#include "lvgl.h"
#include "esp_log.h"

#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <math.h>

static const char *TAG = "weather_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W  240
#define APP_AREA_H  296
#define HEADER_H     30
#define UPDATE_PERIOD_MS  (30 * 1000)

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t   *root;

    lv_obj_t   *lbl_temp;
    lv_obj_t   *lbl_pressure;
    lv_obj_t   *lbl_baro_alt;
    lv_obj_t   *lbl_trend;
    lv_obj_t   *lbl_humidity;
    lv_obj_t   *lbl_gps_alt;
    lv_obj_t   *lbl_updated;

    lv_timer_t *refresh_timer;

    /* Previous pressure reading for trend (Pa equivalent) */
    float       last_pressure;
    bool        has_last_pressure;
} s_wx;

/* ------------------------------------------------------------------ */
/* Data reading                                                         */
/* ------------------------------------------------------------------ */

/*
 * The BHI260AP HAL only exposes accel/gyro/mag — no pressure/temp/humidity
 * in the current driver interface.  We show "N/A" for those fields and
 * note that a future baro driver will supply them.
 *
 * We do use IMU data to demonstrate the interface is live.
 */
static void read_and_update(void)
{
    const hal_registry_t *hal = hal_get_registry();
    char buf[48];

    /* --- Temperature (baro chip not yet in HAL — placeholder) --- */
    lv_label_set_text(s_wx.lbl_temp,     "Temperature:  N/A");
    lv_label_set_text(s_wx.lbl_pressure, "Pressure:     N/A");
    lv_label_set_text(s_wx.lbl_baro_alt, "Altitude:     N/A (baro)");
    lv_label_set_text(s_wx.lbl_humidity, "Humidity:     N/A");
    lv_label_set_text(s_wx.lbl_trend,    "Trend:        N/A");

    /* --- GPS altitude --- */
    if (hal && hal->gps && hal->gps->get_position) {
        hal_gps_position_t pos;
        memset(&pos, 0, sizeof(pos));
        esp_err_t err = hal->gps->get_position(&pos);
        if (err == ESP_OK && pos.fix_valid) {
            snprintf(buf, sizeof(buf), "GPS Alt:      %.1f m", (double)pos.altitude_m);
        } else {
            snprintf(buf, sizeof(buf), "GPS Alt:      No fix");
        }
    } else {
        snprintf(buf, sizeof(buf), "GPS Alt:      N/A");
    }
    lv_label_set_text(s_wx.lbl_gps_alt, buf);

    /* --- IMU check — confirm sensor is live (accel mag shown in debug) --- */
    if (hal && hal->imu && hal->imu->get_data) {
        hal_imu_data_t imu;
        memset(&imu, 0, sizeof(imu));
        esp_err_t err = hal->imu->get_data(&imu);
        if (err == ESP_OK) {
            ESP_LOGD(TAG, "IMU accel %.2f %.2f %.2f",
                     (double)imu.accel_x, (double)imu.accel_y, (double)imu.accel_z);
        }
    }

    /* --- Timestamp --- */
    /* Use LVGL tick as a fallback (ms since boot) */
    uint32_t ms  = lv_tick_get();
    uint32_t sec = ms / 1000;
    uint32_t m   = (sec / 60) % 60;
    uint32_t s   = sec % 60;
    snprintf(buf, sizeof(buf), "Updated: %02lu:%02lu (uptime)", (unsigned long)m, (unsigned long)s);
    lv_label_set_text(s_wx.lbl_updated, buf);
}

/* ------------------------------------------------------------------ */
/* Timer callback                                                       */
/* ------------------------------------------------------------------ */

static void refresh_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    read_and_update();
}

/* ------------------------------------------------------------------ */
/* Button callback                                                      */
/* ------------------------------------------------------------------ */

static void refresh_btn_cb(lv_event_t *e)
{
    (void)e;
    read_and_update();
    toast_info("Refreshed");
}

/* ------------------------------------------------------------------ */
/* UI helpers                                                           */
/* ------------------------------------------------------------------ */

static lv_obj_t *make_row_label(lv_obj_t *parent, const theme_colors_t *clr,
                                const char *initial_text)
{
    lv_obj_t *lbl = lv_label_create(parent);
    lv_label_set_text(lbl, initial_text);
    lv_obj_set_width(lbl, LV_PCT(100));
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, clr->text, LV_PART_MAIN);
    lv_obj_set_style_pad_left(lbl, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(lbl, 3, LV_PART_MAIN);
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_WRAP);
    return lbl;
}

static void make_divider(lv_obj_t *parent, const theme_colors_t *clr)
{
    lv_obj_t *div = lv_obj_create(parent);
    lv_obj_set_size(div, APP_AREA_W - 16, 1);
    lv_obj_set_style_bg_color(div, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(div, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(div, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(div, 0, LV_PART_MAIN);
    lv_obj_set_style_margin_left(div, 8, LV_PART_MAIN);
    lv_obj_set_style_margin_top(div, 2, LV_PART_MAIN);
    lv_obj_set_style_margin_bottom(div, 2, LV_PART_MAIN);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t weather_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating Weather UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_wx, 0, sizeof(s_wx));

    const theme_colors_t *clr = theme_get_colors();

    /* Root container */
    s_wx.root = lv_obj_create(parent);
    lv_obj_set_size(s_wx.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_wx.root, 0, 0);
    lv_obj_set_style_bg_color(s_wx.root, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_wx.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_wx.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_wx.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_wx.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_wx.root, LV_OBJ_FLAG_SCROLLABLE);

    /* Header */
    lv_obj_t *hdr = lv_obj_create(s_wx.root);
    lv_obj_set_size(hdr, APP_AREA_W, HEADER_H);
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
    lv_label_set_text(title, "Weather Station");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, clr->text, LV_PART_MAIN);

    /* Scrollable content area */
    lv_obj_t *content = lv_obj_create(s_wx.root);
    lv_obj_set_pos(content, 0, HEADER_H);
    lv_obj_set_size(content, APP_AREA_W, APP_AREA_H - HEADER_H);
    lv_obj_set_style_bg_color(content, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(content, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(content, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(content, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(content, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(content, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(content, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(content, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(content, 2, LV_PART_SCROLLBAR);

    /* Data rows */
    s_wx.lbl_temp     = make_row_label(content, clr, "Temperature:  --\xc2\xb0""C");
    s_wx.lbl_pressure = make_row_label(content, clr, "Pressure:     -- hPa");
    s_wx.lbl_baro_alt = make_row_label(content, clr, "Altitude:     -- m (baro)");
    make_divider(content, clr);
    s_wx.lbl_trend    = make_row_label(content, clr, "Trend:        --");
    make_divider(content, clr);
    s_wx.lbl_humidity = make_row_label(content, clr, "Humidity:     --");
    make_divider(content, clr);
    s_wx.lbl_gps_alt  = make_row_label(content, clr, "GPS Alt:      -- m");
    make_divider(content, clr);
    s_wx.lbl_updated  = make_row_label(content, clr, "Updated: --:--");

    /* Refresh button */
    lv_obj_t *btn_row = lv_obj_create(content);
    lv_obj_set_size(btn_row, APP_AREA_W, 36);
    lv_obj_set_style_bg_opa(btn_row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(btn_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(btn_row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_left(btn_row, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(btn_row, 4, LV_PART_MAIN);

    lv_obj_t *ref_btn = lv_button_create(btn_row);
    lv_obj_set_height(ref_btn, 26);
    lv_obj_set_style_bg_color(ref_btn, clr->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(ref_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(ref_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_border_width(ref_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_hor(ref_btn, 10, LV_PART_MAIN);
    lv_obj_set_style_pad_ver(ref_btn, 2, LV_PART_MAIN);
    lv_obj_add_event_cb(ref_btn, refresh_btn_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *ref_lbl = lv_label_create(ref_btn);
    lv_label_set_text(ref_lbl, "Refresh");
    lv_obj_set_style_text_font(ref_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(ref_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(ref_lbl);

    /* Auto-refresh timer */
    s_wx.refresh_timer = lv_timer_create(refresh_timer_cb, UPDATE_PERIOD_MS, NULL);

    /* Initial read */
    read_and_update();

    return ESP_OK;
}

void weather_ui_show(void)
{
    if (s_wx.root) {
        lv_obj_clear_flag(s_wx.root, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_wx.refresh_timer) {
        lv_timer_resume(s_wx.refresh_timer);
    }
}

void weather_ui_hide(void)
{
    if (s_wx.refresh_timer) {
        lv_timer_pause(s_wx.refresh_timer);
    }
    if (s_wx.root) {
        lv_obj_add_flag(s_wx.root, LV_OBJ_FLAG_HIDDEN);
    }
}
