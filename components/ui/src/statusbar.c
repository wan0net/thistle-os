#include "ui/statusbar.h"
#include "ui/theme.h"
#include "esp_log.h"
#include <stdio.h>
#include <stdbool.h>

static const char *TAG = "statusbar";

#define STATUSBAR_HEIGHT 24

/* LVGL object handles */
static lv_obj_t *s_container     = NULL;
static lv_obj_t *s_title_label   = NULL;
static lv_obj_t *s_time_label    = NULL;
static lv_obj_t *s_battery_label = NULL;
static lv_obj_t *s_wifi_label    = NULL;

esp_err_t statusbar_create(lv_obj_t *parent)
{
    if (parent == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    const theme_colors_t *colors = theme_get_colors();

    /* Main container — full width, fixed height, flex-row layout */
    s_container = lv_obj_create(parent);
    lv_obj_set_size(s_container, LV_PCT(100), STATUSBAR_HEIGHT);
    lv_obj_set_pos(s_container, 0, 0);
    lv_obj_set_style_bg_color(s_container, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_container, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_container, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(s_container, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_container, colors->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_container, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(s_container, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_container, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(s_container, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_container, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_container, 0, LV_PART_MAIN);

    /* Flex row layout */
    lv_obj_set_layout(s_container, LV_LAYOUT_FLEX);
    lv_obj_set_flex_flow(s_container, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(s_container,
                          LV_FLEX_ALIGN_SPACE_BETWEEN,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    /* Title label — left */
    s_title_label = lv_label_create(s_container);
    lv_label_set_text(s_title_label, "ThistleOS");
    lv_obj_set_style_text_color(s_title_label, colors->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(s_title_label, 1);

    /* Time label — center */
    s_time_label = lv_label_create(s_container);
    lv_label_set_text(s_time_label, "--:--");
    lv_obj_set_style_text_color(s_time_label, colors->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(s_time_label, 1);
    lv_obj_set_style_text_align(s_time_label, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);

    /* Right-side cluster: wifi + battery */
    lv_obj_t *right = lv_obj_create(s_container);
    lv_obj_set_height(right, STATUSBAR_HEIGHT - 2);
    lv_obj_set_style_bg_opa(right, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(right, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(right, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(right, 0, LV_PART_MAIN);
    lv_obj_set_layout(right, LV_LAYOUT_FLEX);
    lv_obj_set_flex_flow(right, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(right, LV_FLEX_ALIGN_END, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(right, 4, LV_PART_MAIN);

    s_wifi_label = lv_label_create(right);
    lv_label_set_text(s_wifi_label, "--");
    lv_obj_set_style_text_color(s_wifi_label, colors->text, LV_PART_MAIN);

    s_battery_label = lv_label_create(right);
    lv_label_set_text(s_battery_label, "BAT:--%%");
    lv_obj_set_style_text_color(s_battery_label, colors->text, LV_PART_MAIN);

    ESP_LOGI(TAG, "status bar created (%d px)", STATUSBAR_HEIGHT);
    return ESP_OK;
}

void statusbar_set_battery(uint8_t percent, bool charging)
{
    if (s_battery_label == NULL) {
        return;
    }
    char buf[16];
    if (charging) {
        snprintf(buf, sizeof(buf), "CHG:%d%%", percent);
    } else {
        snprintf(buf, sizeof(buf), "BAT:%d%%", percent);
    }
    lv_label_set_text(s_battery_label, buf);
}

void statusbar_set_wifi(bool connected, int rssi)
{
    if (s_wifi_label == NULL) {
        return;
    }
    if (connected) {
        char buf[16];
        snprintf(buf, sizeof(buf), "W:%d", rssi);
        lv_label_set_text(s_wifi_label, buf);
    } else {
        lv_label_set_text(s_wifi_label, "--");
    }
}

void statusbar_set_title(const char *title)
{
    if (s_title_label == NULL || title == NULL) {
        return;
    }
    lv_label_set_text(s_title_label, title);
}

void statusbar_set_time(uint8_t hour, uint8_t minute)
{
    if (s_time_label == NULL) {
        return;
    }
    char buf[8];
    snprintf(buf, sizeof(buf), "%02d:%02d", hour, minute);
    lv_label_set_text(s_time_label, buf);
}

uint16_t statusbar_get_height(void)
{
    return STATUSBAR_HEIGHT;
}
