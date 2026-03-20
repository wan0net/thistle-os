#include "hal/sdcard_path.h"
#include <inttypes.h>
/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Navigator GPS dashboard UI
 *
 * Layout (320x216 app area):
 *   Coordinates panel (LAT/LON/ALT) — top section
 *   Movement panel    (SPEED/HEAD)  — middle section
 *   Signal panel      (SAT/FIX)     — middle-lower
 *   Control bar       (Record/Stop buttons + track stats) — bottom
 *
 * GPS data sourced from HAL: hal_get_registry()->gps->get_position()
 * Track recording: GPX files written to /sdcard/tracks/
 */
#include "navigator/navigator_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "hal/board.h"
#include "ui/theme.h"

#include <stdio.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <sys/stat.h>

static const char *TAG = "navigator_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W   320
#define APP_AREA_H   216

/* Section heights */
#define SECTION_H     28   /* each data row */
#define DIVIDER_H      1
#define CTRL_BAR_H    36
#define BEACON_ROW_H  22

/* Total rows: 3 coord + div + 2 move + div + 1 signal + div + ctrl + beacon
 * = 3*28 + 3*1 + 2*28 + 1*28 + 36 + 22 = 84+3+56+28+36+22 = 229 → too tall.
 * Reduce: collapse beacon row, tighten heights.
 * Revised: coord_h=26*3=78, move_h=26*2=52, sat_h=26, ctrl_h=34, beacon_h=20, div*3=3
 * = 78+52+26+34+20+3 = 213 — fits in 216.
 */
#define ROW_H         26
#define CTRL_H        34
#define BEACON_H      20

/* ------------------------------------------------------------------ */
/* Track recording state                                               */
/* ------------------------------------------------------------------ */

static struct {
    bool    recording;
    FILE   *gpx_file;
    uint32_t start_time_ms;   /* lv_tick_get() at start */
    double  total_distance_m;
    double  last_lat, last_lon;
    int     fix_count;
} s_track;

/* ------------------------------------------------------------------ */
/* UI widget state                                                      */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;

    /* Coordinate labels (values) */
    lv_obj_t *lbl_lat;
    lv_obj_t *lbl_lon;
    lv_obj_t *lbl_alt;

    /* Movement labels */
    lv_obj_t *lbl_speed;
    lv_obj_t *lbl_head;

    /* Signal labels */
    lv_obj_t *lbl_sat;
    lv_obj_t *lbl_fix;

    /* Control bar */
    lv_obj_t *btn_record;
    lv_obj_t *btn_stop;
    lv_obj_t *lbl_track_stats;

    /* Beacon row */
    lv_obj_t *lbl_beacon;

    /* Update timer */
    lv_timer_t *update_timer;

    /* GPS availability */
    bool gps_available;
} s_ui;

/* ------------------------------------------------------------------ */
/* Haversine distance (metres) between two lat/lon points              */
/* ------------------------------------------------------------------ */

static double haversine_m(double lat1, double lon1, double lat2, double lon2)
{
    double dlat = (lat2 - lat1) * M_PI / 180.0;
    double dlon = (lon2 - lon1) * M_PI / 180.0;
    double a = sin(dlat / 2) * sin(dlat / 2) +
               cos(lat1 * M_PI / 180.0) * cos(lat2 * M_PI / 180.0) *
               sin(dlon / 2) * sin(dlon / 2);
    return 6371000.0 * 2.0 * atan2(sqrt(a), sqrt(1.0 - a));
}

/* ------------------------------------------------------------------ */
/* GPX track file helpers                                              */
/* ------------------------------------------------------------------ */

static void ensure_tracks_dir(void)
{
    struct stat st;
    if (stat(THISTLE_SDCARD "/tracks", &st) != 0) {
        /* Directory does not exist — create it */
#ifdef _WIN32
        mkdir(THISTLE_SDCARD "/tracks");
#else
        mkdir(THISTLE_SDCARD "/tracks", 0755);
#endif
    }
}

static bool open_gpx_file(void)
{
    ensure_tracks_dir();

    /* Build filename from current time if available, else use tick counter */
    char path[64];
    time_t now = (time_t)(lv_tick_get() / 1000);  /* fallback: seconds since boot */

    /* Try to get real wall-clock time */
    struct tm *tm_info = NULL;
#ifndef SIMULATOR_BUILD
    time_t wall = time(NULL);
    if (wall > 1000000000L) {
        now = wall;
    }
#endif
    tm_info = localtime(&now);

    if (tm_info) {
        snprintf(path, sizeof(path),
                 "/sdcard/tracks/%04d-%02d-%02d_%02d%02d%02d.gpx",
                 tm_info->tm_year + 1900,
                 tm_info->tm_mon + 1,
                 tm_info->tm_mday,
                 tm_info->tm_hour,
                 tm_info->tm_min,
                 tm_info->tm_sec);
    } else {
        snprintf(path, sizeof(path),
                 "/sdcard/tracks/track_%lu.gpx",
                 (unsigned long)lv_tick_get());
    }

    s_track.gpx_file = fopen(path, "w");
    if (!s_track.gpx_file) {
        ESP_LOGE(TAG, "Failed to open GPX file: %s", path);
        return false;
    }

    /* Write GPX header */
    fprintf(s_track.gpx_file,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
            "<gpx version=\"1.1\" creator=\"ThistleOS Navigator\">\n"
            "  <trk><name>ThistleOS Track</name><trkseg>\n");
    fflush(s_track.gpx_file);
    ESP_LOGI(TAG, "Track recording started: %s", path);
    return true;
}

static void close_gpx_file(void)
{
    if (s_track.gpx_file) {
        fprintf(s_track.gpx_file, "  </trkseg></trk>\n</gpx>\n");
        fclose(s_track.gpx_file);
        s_track.gpx_file = NULL;
        ESP_LOGI(TAG, "Track recording stopped. Fixes: %d, Distance: %.1f m",
                 s_track.fix_count, s_track.total_distance_m);
    }
}

static void write_gpx_trkpt(double lat, double lon, float alt, uint32_t ts_unix)
{
    if (!s_track.gpx_file) return;

    /* ISO 8601 timestamp */
    char ts_buf[32] = "1970-01-01T00:00:00Z";
    if (ts_unix > 0) {
        time_t t = (time_t)ts_unix;
        struct tm *tm_info = gmtime(&t);
        if (tm_info) {
            strftime(ts_buf, sizeof(ts_buf), "%Y-%m-%dT%H:%M:%SZ", tm_info);
        }
    }

    fprintf(s_track.gpx_file,
            "    <trkpt lat=\"%.7f\" lon=\"%.7f\">"
            "<ele>%.1f</ele>"
            "<time>%s</time>"
            "</trkpt>\n",
            lat, lon, alt, ts_buf);
    fflush(s_track.gpx_file);
}

/* ------------------------------------------------------------------ */
/* Track stats string helper                                           */
/* ------------------------------------------------------------------ */

static void format_track_stats(char *buf, size_t len)
{
    if (!s_track.recording) {
        snprintf(buf, len, "Track: --:--:--  0.0 km");
        return;
    }

    uint32_t elapsed_ms = lv_tick_get() - s_track.start_time_ms;
    uint32_t elapsed_s  = elapsed_ms / 1000;
    uint32_t hh = elapsed_s / 3600;
    uint32_t mm = (elapsed_s % 3600) / 60;
    uint32_t ss = elapsed_s % 60;
    double km = s_track.total_distance_m / 1000.0;

    snprintf(buf, len, "Track: %02" PRIu32 ":%02" PRIu32 ":%02" PRIu32 "  %.1f km", hh, mm, ss, km);
}

/* ------------------------------------------------------------------ */
/* GPS update timer callback (every 2 seconds)                         */
/* ------------------------------------------------------------------ */

static void navigator_update_cb(lv_timer_t *timer)
{
    (void)timer;

    if (!s_ui.gps_available) {
        /* GPS driver not present — show static "No GPS" message */
        lv_label_set_text(s_ui.lbl_lat,   "No GPS available");
        lv_label_set_text(s_ui.lbl_lon,   "—");
        lv_label_set_text(s_ui.lbl_alt,   "—");
        lv_label_set_text(s_ui.lbl_speed, "—");
        lv_label_set_text(s_ui.lbl_head,  "—");
        lv_label_set_text(s_ui.lbl_sat,   "—");
        lv_label_set_text(s_ui.lbl_fix,   "None");
        return;
    }

    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->gps || !reg->gps->get_position) {
        lv_label_set_text(s_ui.lbl_lat, "GPS error");
        return;
    }

    hal_gps_position_t pos;
    esp_err_t err = reg->gps->get_position(&pos);
    if (err != ESP_OK) {
        lv_label_set_text(s_ui.lbl_lat, "GPS error");
        return;
    }

    if (!pos.fix_valid) {
        lv_label_set_text(s_ui.lbl_lat,   "Waiting for fix...");
        lv_label_set_text(s_ui.lbl_lon,   "—");
        lv_label_set_text(s_ui.lbl_alt,   "—");
        lv_label_set_text(s_ui.lbl_speed, "—");
        lv_label_set_text(s_ui.lbl_head,  "—");

        char sat_buf[8];
        snprintf(sat_buf, sizeof(sat_buf), "%u", (unsigned)pos.satellites);
        lv_label_set_text(s_ui.lbl_sat, sat_buf);
        lv_label_set_text(s_ui.lbl_fix, "None");
        return;
    }

    /* Valid fix — update all fields */
    char buf[48];

    /* LAT — e.g. "55.8642degN" */
    snprintf(buf, sizeof(buf), "%.4f\xC2\xB0%c",
             fabs(pos.latitude),
             pos.latitude >= 0.0 ? 'N' : 'S');
    lv_label_set_text(s_ui.lbl_lat, buf);

    /* LON — e.g. "-4.2518degW" */
    snprintf(buf, sizeof(buf), "%.4f\xC2\xB0%c",
             fabs(pos.longitude),
             pos.longitude >= 0.0 ? 'E' : 'W');
    lv_label_set_text(s_ui.lbl_lon, buf);

    /* ALT */
    snprintf(buf, sizeof(buf), "%.1f m", pos.altitude_m);
    lv_label_set_text(s_ui.lbl_alt, buf);

    /* SPEED */
    snprintf(buf, sizeof(buf), "%.1f km/h", pos.speed_kmh);
    lv_label_set_text(s_ui.lbl_speed, buf);

    /* HEADING */
    snprintf(buf, sizeof(buf), "%.1f\xC2\xB0", pos.heading_deg);
    lv_label_set_text(s_ui.lbl_head, buf);

    /* SAT count */
    snprintf(buf, sizeof(buf), "%u", (unsigned)pos.satellites);
    lv_label_set_text(s_ui.lbl_sat, buf);

    /* FIX type — satellites >= 4 → "3D", >= 3 → "2D", else "No" */
    if (pos.satellites >= 4) {
        lv_label_set_text(s_ui.lbl_fix, "3D");
    } else if (pos.satellites >= 3) {
        lv_label_set_text(s_ui.lbl_fix, "2D");
    } else {
        lv_label_set_text(s_ui.lbl_fix, "No");
    }

    /* Track recording: append fix and accumulate distance */
    if (s_track.recording) {
        if (s_track.fix_count > 0) {
            double d = haversine_m(s_track.last_lat, s_track.last_lon,
                                   pos.latitude, pos.longitude);
            s_track.total_distance_m += d;
        }
        s_track.last_lat = pos.latitude;
        s_track.last_lon = pos.longitude;
        s_track.fix_count++;
        write_gpx_trkpt(pos.latitude, pos.longitude, pos.altitude_m, pos.timestamp);
    }

    /* Track stats label */
    char stats_buf[48];
    format_track_stats(stats_buf, sizeof(stats_buf));
    lv_label_set_text(s_ui.lbl_track_stats, stats_buf);
}

/* ------------------------------------------------------------------ */
/* Button callbacks                                                     */
/* ------------------------------------------------------------------ */

static void record_btn_cb(lv_event_t *e)
{
    (void)e;
    if (s_track.recording) return;  /* already recording */

    if (open_gpx_file()) {
        s_track.recording        = true;
        s_track.start_time_ms    = lv_tick_get();
        s_track.total_distance_m = 0.0;
        s_track.fix_count        = 0;

        /* Visual feedback — dim Record button, highlight Stop */
        const theme_colors_t *colors = theme_get_colors();
        lv_obj_set_style_bg_color(s_ui.btn_record, colors->text_secondary,
                                  LV_PART_MAIN);
        lv_obj_set_style_bg_color(s_ui.btn_stop, colors->primary, LV_PART_MAIN);

        ESP_LOGI(TAG, "Track recording started");
    }
}

static void stop_btn_cb(lv_event_t *e)
{
    (void)e;
    if (!s_track.recording) return;

    close_gpx_file();
    s_track.recording = false;

    /* Reset visual state */
    const theme_colors_t *colors = theme_get_colors();
    lv_obj_set_style_bg_color(s_ui.btn_record, colors->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_color(s_ui.btn_stop, colors->text_secondary, LV_PART_MAIN);

    /* Reset stats label */
    lv_label_set_text(s_ui.lbl_track_stats, "Track: --:--:--  0.0 km");

    ESP_LOGI(TAG, "Track recording stopped");
}

/* ------------------------------------------------------------------ */
/* Row creation helpers                                                 */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_data_row(lv_obj_t *parent, int y_pos,
                                  const char *label_text,
                                  const lv_font_t *value_font,
                                  lv_obj_t **out_value_label)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *row = lv_obj_create(parent);
    lv_obj_set_size(row, APP_AREA_W, ROW_H);
    lv_obj_set_pos(row, 0, y_pos);
    lv_obj_set_style_bg_opa(row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(row, 2, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    /* Fixed-width label column (60 px) */
    lv_obj_t *lbl_key = lv_label_create(row);
    lv_label_set_text(lbl_key, label_text);
    lv_obj_set_style_text_font(lbl_key, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_key, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_pos(lbl_key, 4, 4);

    /* Value label — positioned after the key */
    lv_obj_t *lbl_val = lv_label_create(row);
    lv_label_set_text(lbl_val, "—");
    lv_obj_set_style_text_font(lbl_val, value_font, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_val, colors->text, LV_PART_MAIN);
    lv_obj_set_pos(lbl_val, 68, 2);

    if (out_value_label) {
        *out_value_label = lbl_val;
    }

    return row;
}

static void create_divider(lv_obj_t *parent, int y_pos)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *div = lv_obj_create(parent);
    lv_obj_set_size(div, APP_AREA_W, 1);
    lv_obj_set_pos(div, 0, y_pos);
    lv_obj_set_style_bg_color(div, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(div, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(div, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(div, 0, LV_PART_MAIN);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t navigator_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "creating Navigator UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Check GPS availability once at create time */
    const hal_registry_t *reg = hal_get_registry();
    s_ui.gps_available = (reg && reg->gps && reg->gps->get_position);

    if (!s_ui.gps_available) {
        ESP_LOGW(TAG, "no GPS driver registered — display-only mode");
    }

    const theme_colors_t *colors = theme_get_colors();

    /* Root container */
    s_ui.root = lv_obj_create(parent);
    lv_obj_set_size(s_ui.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_ui.root, 0, 0);
    lv_obj_set_style_bg_color(s_ui.root, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ui.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ui.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_ui.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ui.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_ui.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Coordinate rows: LAT, LON, ALT  (y=0..78, 3 rows × 26px)
     * ---------------------------------------------------------------- */
    int y = 0;
    create_data_row(s_ui.root, y, "LAT", &lv_font_montserrat_22, &s_ui.lbl_lat);
    y += ROW_H;
    create_data_row(s_ui.root, y, "LON", &lv_font_montserrat_22, &s_ui.lbl_lon);
    y += ROW_H;
    create_data_row(s_ui.root, y, "ALT", &lv_font_montserrat_14, &s_ui.lbl_alt);
    y += ROW_H;

    /* Divider */
    create_divider(s_ui.root, y);
    y += 1;

    /* ----------------------------------------------------------------
     * Movement rows: SPEED, HEADING  (2 × 26px)
     * ---------------------------------------------------------------- */
    create_data_row(s_ui.root, y, "SPEED", &lv_font_montserrat_14, &s_ui.lbl_speed);
    y += ROW_H;
    create_data_row(s_ui.root, y, "HEAD",  &lv_font_montserrat_14, &s_ui.lbl_head);
    y += ROW_H;

    /* Divider */
    create_divider(s_ui.root, y);
    y += 1;

    /* ----------------------------------------------------------------
     * Signal row: SAT + FIX on same row (1 × 26px)
     * ---------------------------------------------------------------- */
    {
        lv_obj_t *row = lv_obj_create(s_ui.root);
        lv_obj_set_size(row, APP_AREA_W, ROW_H);
        lv_obj_set_pos(row, 0, y);
        lv_obj_set_style_bg_opa(row, LV_OPA_TRANSP, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(row, 2, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

        /* SAT label + value */
        lv_obj_t *lbl_sat_key = lv_label_create(row);
        lv_label_set_text(lbl_sat_key, "SAT");
        lv_obj_set_style_text_font(lbl_sat_key, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_sat_key, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_pos(lbl_sat_key, 4, 4);

        s_ui.lbl_sat = lv_label_create(row);
        lv_label_set_text(s_ui.lbl_sat, "—");
        lv_obj_set_style_text_font(s_ui.lbl_sat, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(s_ui.lbl_sat, colors->text, LV_PART_MAIN);
        lv_obj_set_pos(s_ui.lbl_sat, 40, 4);

        /* FIX label + value */
        lv_obj_t *lbl_fix_key = lv_label_create(row);
        lv_label_set_text(lbl_fix_key, "FIX");
        lv_obj_set_style_text_font(lbl_fix_key, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_fix_key, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_pos(lbl_fix_key, 160, 4);

        s_ui.lbl_fix = lv_label_create(row);
        lv_label_set_text(s_ui.lbl_fix, "None");
        lv_obj_set_style_text_font(s_ui.lbl_fix, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(s_ui.lbl_fix, colors->text, LV_PART_MAIN);
        lv_obj_set_pos(s_ui.lbl_fix, 196, 4);
    }
    y += ROW_H;

    /* Divider */
    create_divider(s_ui.root, y);
    y += 1;

    /* ----------------------------------------------------------------
     * Control bar: [Record Track] [Stop] + track stats label
     * Height: CTRL_H = 34px
     * ---------------------------------------------------------------- */
    {
        lv_obj_t *ctrl = lv_obj_create(s_ui.root);
        lv_obj_set_size(ctrl, APP_AREA_W, CTRL_H);
        lv_obj_set_pos(ctrl, 0, y);
        lv_obj_set_style_bg_color(ctrl, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(ctrl, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_side(ctrl, LV_BORDER_SIDE_NONE, LV_PART_MAIN);
        lv_obj_set_style_pad_all(ctrl, 4, LV_PART_MAIN);
        lv_obj_set_style_radius(ctrl, 0, LV_PART_MAIN);
        lv_obj_clear_flag(ctrl, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_set_flex_flow(ctrl, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(ctrl, LV_FLEX_ALIGN_START,
                              LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(ctrl, 6, LV_PART_MAIN);

        /* Record button */
        s_ui.btn_record = lv_button_create(ctrl);
        lv_obj_set_size(s_ui.btn_record, 90, 26);
        lv_obj_set_style_bg_color(s_ui.btn_record, colors->primary, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_ui.btn_record, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(s_ui.btn_record, 4, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_ui.btn_record, 2, LV_PART_MAIN);

        lv_obj_t *rec_lbl = lv_label_create(s_ui.btn_record);
        lv_label_set_text(rec_lbl, "Record Track");
        lv_obj_set_style_text_font(rec_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(rec_lbl, lv_color_white(), LV_PART_MAIN);
        lv_obj_center(rec_lbl);
        lv_obj_add_event_cb(s_ui.btn_record, record_btn_cb, LV_EVENT_CLICKED, NULL);

        /* Stop button */
        s_ui.btn_stop = lv_button_create(ctrl);
        lv_obj_set_size(s_ui.btn_stop, 50, 26);
        lv_obj_set_style_bg_color(s_ui.btn_stop, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_ui.btn_stop, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(s_ui.btn_stop, 4, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_ui.btn_stop, 2, LV_PART_MAIN);

        lv_obj_t *stop_lbl = lv_label_create(s_ui.btn_stop);
        lv_label_set_text(stop_lbl, "Stop");
        lv_obj_set_style_text_font(stop_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(stop_lbl, lv_color_white(), LV_PART_MAIN);
        lv_obj_center(stop_lbl);
        lv_obj_add_event_cb(s_ui.btn_stop, stop_btn_cb, LV_EVENT_CLICKED, NULL);

        /* Track stats label (flex-grow fills remaining width) */
        s_ui.lbl_track_stats = lv_label_create(ctrl);
        lv_label_set_text(s_ui.lbl_track_stats, "Track: --:--:--  0.0 km");
        lv_obj_set_style_text_font(s_ui.lbl_track_stats, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(s_ui.lbl_track_stats, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_flex_grow(s_ui.lbl_track_stats, 1);
    }
    y += CTRL_H;

    /* ----------------------------------------------------------------
     * Beacon row — nearby nodes from radio (optional)
     * Height: BEACON_H = 20px — fits within remaining 216-y space
     * ---------------------------------------------------------------- */
    {
        lv_obj_t *beacon_row = lv_obj_create(s_ui.root);
        lv_obj_set_size(beacon_row, APP_AREA_W, BEACON_H);
        lv_obj_set_pos(beacon_row, 0, y);
        lv_obj_set_style_bg_color(beacon_row, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(beacon_row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_side(beacon_row, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
        lv_obj_set_style_border_color(beacon_row, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(beacon_row, 1, LV_PART_MAIN);
        lv_obj_set_style_pad_all(beacon_row, 2, LV_PART_MAIN);
        lv_obj_set_style_radius(beacon_row, 0, LV_PART_MAIN);
        lv_obj_clear_flag(beacon_row, LV_OBJ_FLAG_SCROLLABLE);

        s_ui.lbl_beacon = lv_label_create(beacon_row);
        lv_label_set_text(s_ui.lbl_beacon, "No nearby beacons");
        lv_obj_set_style_text_font(s_ui.lbl_beacon, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(s_ui.lbl_beacon, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_pos(s_ui.lbl_beacon, 4, 1);
    }

    /* ----------------------------------------------------------------
     * LVGL timer: refresh GPS data every 2 seconds
     * ---------------------------------------------------------------- */
    s_ui.update_timer = lv_timer_create(navigator_update_cb, 2000, NULL);

    /* Immediate first render */
    navigator_update_cb(NULL);

    return ESP_OK;
}

void navigator_ui_show(void)
{
    if (s_ui.root) {
        lv_obj_clear_flag(s_ui.root, LV_OBJ_FLAG_HIDDEN);
    }
    /* Resume timer when app comes to foreground */
    if (s_ui.update_timer) {
        lv_timer_resume(s_ui.update_timer);
    }
}

void navigator_ui_hide(void)
{
    if (s_ui.root) {
        lv_obj_add_flag(s_ui.root, LV_OBJ_FLAG_HIDDEN);
    }
    /* Pause timer when app is backgrounded — saves CPU */
    if (s_ui.update_timer) {
        lv_timer_pause(s_ui.update_timer);
    }
}
