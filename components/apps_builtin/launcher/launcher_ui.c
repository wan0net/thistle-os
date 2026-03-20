#include "launcher/launcher_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "thistle/app_manager.h"
#include "thistle/wifi_manager.h"
#include "ui/toast.h"
#include "ui/theme.h"

static const char *TAG = "launcher_ui";

/* ------------------------------------------------------------------ */
/* Internal callbacks                                                   */
/* ------------------------------------------------------------------ */

static void dock_icon_clicked_cb(lv_event_t *e)
{
    const char *app_id = (const char *)lv_obj_get_user_data(lv_event_get_target(e));
    if (!app_id) {
        ESP_LOGW(TAG, "dock icon pressed: app not installed");
        return;
    }

    ESP_LOGI(TAG, "Launching app: %s", app_id);
    esp_err_t ret = app_manager_launch(app_id);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to launch %s: %s", app_id, esp_err_to_name(ret));
        toast_warn("App not available");
    }
}

/* App-area dimensions (full 320x216 after the 24px status bar) */
#define APP_AREA_W   320
#define APP_AREA_H   216
#define DOCK_H        60
#define ICON_SIZE     48

static lv_obj_t *s_root = NULL;

/* ------------------------------------------------------------------ */
/* Dock icon helper                                                     */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_dock_icon(lv_obj_t *parent, const char *label, const char *app_name)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, ICON_SIZE, ICON_SIZE);

    /* Theme-aware: surface fill, text-colored border, no radius, no shadow */
    lv_obj_set_style_bg_color(btn, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, colors->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_shadow_width(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn, 0, LV_PART_MAIN);

    /* Pressed state: primary accent fill, white text */
    lv_obj_set_style_bg_color(btn, colors->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);

    /* Single-character label centred inside the button */
    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, colors->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_center(lbl);

    /* Store app_name for click handler via user_data */
    lv_obj_set_user_data(btn, (void *)app_name);

    lv_obj_add_event_cb(btn, dock_icon_clicked_cb, LV_EVENT_CLICKED, NULL);

    return btn;
}

/* ------------------------------------------------------------------ */
/* Clock update timer callback                                          */
/* ------------------------------------------------------------------ */

static void launcher_clock_update(lv_timer_t *timer)
{
    lv_obj_t *clock_label = (lv_obj_t *)lv_timer_get_user_data(timer);
    char time_buf[8];
    wifi_manager_get_time_str(time_buf, sizeof(time_buf));
    lv_label_set_text(clock_label, time_buf);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t launcher_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "creating BlackBerry-style launcher UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    const theme_colors_t *colors = theme_get_colors();

    /* Root container — fills the entire app area */
    s_root = lv_obj_create(parent);
    lv_obj_set_size(s_root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_root, 0, 0);
    lv_obj_set_style_bg_opa(s_root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Wallpaper area — sits above the dock, uses theme background color.
     * If /sdcard/config/wallpaper.bin exists it can be loaded as an LVGL
     * image source via lv_image_set_src(); for now we use the theme bg.
     * ---------------------------------------------------------------- */
    lv_obj_t *wallpaper = lv_obj_create(s_root);
    lv_obj_set_pos(wallpaper, 0, 0);
    lv_obj_set_size(wallpaper, APP_AREA_W, APP_AREA_H - DOCK_H);
    lv_obj_set_style_bg_color(wallpaper, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(wallpaper, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(wallpaper, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(wallpaper, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(wallpaper, 0, LV_PART_MAIN);
    lv_obj_clear_flag(wallpaper, LV_OBJ_FLAG_SCROLLABLE);

    /* Center column inside wallpaper */
    lv_obj_set_flex_flow(wallpaper, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(wallpaper,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(wallpaper, 6, LV_PART_MAIN);

    /* Large clock — set to real time immediately, then update every 10s */
    lv_obj_t *lbl_clock = lv_label_create(wallpaper);
    char time_init[8];
    wifi_manager_get_time_str(time_init, sizeof(time_init));
    lv_label_set_text(lbl_clock, time_init);
    lv_obj_set_style_text_font(lbl_clock, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_clock, colors->text, LV_PART_MAIN);

    /* Update clock every 10 seconds */
    lv_timer_create(launcher_clock_update, 10000, lbl_clock);

    /* Branding subtitle */
    lv_obj_t *lbl_brand = lv_label_create(wallpaper);
    lv_label_set_text(lbl_brand, "ThistleOS");
    lv_obj_set_style_text_font(lbl_brand, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_brand, colors->text_secondary, LV_PART_MAIN);

    /* ----------------------------------------------------------------
     * App dock — bottom 60 px, 1px top border, theme surface color
     * ---------------------------------------------------------------- */
    lv_obj_t *dock = lv_obj_create(s_root);
    lv_obj_set_pos(dock, 0, APP_AREA_H - DOCK_H);
    lv_obj_set_size(dock, APP_AREA_W, DOCK_H);
    lv_obj_set_style_bg_color(dock, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(dock, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(dock, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(dock, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(dock, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_all(dock, 6, LV_PART_MAIN);
    lv_obj_set_style_radius(dock, 0, LV_PART_MAIN);
    lv_obj_clear_flag(dock, LV_OBJ_FLAG_SCROLLABLE);

    /* Horizontal flex row, centred */
    lv_obj_set_flex_flow(dock, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(dock,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(dock, 12, LV_PART_MAIN);

    /* Dock icons: Settings, Files, Reader, Messenger */
    create_dock_icon(dock, "S", "com.thistle.settings");
    create_dock_icon(dock, "F", "com.thistle.filemgr");
    create_dock_icon(dock, "R", "com.thistle.reader");
    create_dock_icon(dock, "M", "com.thistle.messenger");

    return ESP_OK;
}

void launcher_ui_show(void)
{
    if (s_root) {
        lv_obj_clear_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}

void launcher_ui_hide(void)
{
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
