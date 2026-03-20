#include "ui/app_switcher.h"
#include "ui/manager.h"
#include "ui/theme.h"
#include "thistle/app_manager.h"
#include "esp_log.h"
#include <stdbool.h>

static const char *TAG = "app_sw";

static lv_obj_t *s_overlay = NULL;
static bool      s_visible  = false;

esp_err_t app_switcher_show(void)
{
    if (s_visible) {
        ESP_LOGD(TAG, "app switcher already visible");
        return ESP_OK;
    }

    lv_obj_t *screen = ui_manager_get_screen();
    if (screen == NULL) {
        ESP_LOGE(TAG, "no screen available");
        return ESP_FAIL;
    }

    const theme_colors_t *colors = theme_get_colors();

    /* Semi-transparent overlay covering the full screen */
    s_overlay = lv_obj_create(screen);
    lv_obj_set_size(s_overlay, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_overlay, 0, 0);
    lv_obj_set_style_bg_color(s_overlay, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_overlay, LV_OPA_90, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_overlay, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_overlay, colors->text, LV_PART_MAIN);
    lv_obj_set_style_radius(s_overlay, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_overlay, 8, LV_PART_MAIN);

    /* Vertical flex layout for app entries */
    lv_obj_set_layout(s_overlay, LV_LAYOUT_FLEX);
    lv_obj_set_flex_flow(s_overlay, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_overlay,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);

    /* Header label */
    lv_obj_t *header = lv_label_create(s_overlay);
    lv_label_set_text(header, "Running Apps");
    lv_obj_set_style_text_color(header, colors->text, LV_PART_MAIN);

    /* Phase 2: iterate running apps and create clickable entries.
     * For now, display a placeholder. */
    app_handle_t fg = app_manager_get_foreground();
    if (fg == APP_HANDLE_INVALID) {
        lv_obj_t *no_apps = lv_label_create(s_overlay);
        lv_label_set_text(no_apps, "No apps running");
        lv_obj_set_style_text_color(no_apps, colors->text_secondary, LV_PART_MAIN);
    } else {
        /* TODO (Phase 2): enumerate all apps, create one lv_btn per app,
         * bind app_manager_switch_to(handle) as click callback. */
        lv_obj_t *placeholder = lv_label_create(s_overlay);
        lv_label_set_text(placeholder, "[app list — Phase 2]");
        lv_obj_set_style_text_color(placeholder, colors->text, LV_PART_MAIN);
    }

    s_visible = true;
    ESP_LOGI(TAG, "app switcher shown");
    return ESP_OK;
}

esp_err_t app_switcher_hide(void)
{
    if (!s_visible || s_overlay == NULL) {
        return ESP_OK;
    }

    lv_obj_delete(s_overlay);
    s_overlay = NULL;
    s_visible = false;

    ESP_LOGI(TAG, "app switcher hidden");
    return ESP_OK;
}

bool app_switcher_is_visible(void)
{
    return s_visible;
}
