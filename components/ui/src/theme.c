#include "ui/theme.h"
#include "esp_log.h"
#include <string.h>

static const char *TAG = "theme";

/* Default monochrome theme — optimized for e-paper (black on white) */
static theme_colors_t s_current_theme;

static void theme_colors_init_defaults(void)
{
    s_current_theme.primary        = lv_color_black();
    s_current_theme.secondary      = lv_color_black();
    s_current_theme.bg             = lv_color_white();
    s_current_theme.surface        = lv_color_white();
    s_current_theme.text           = lv_color_black();
    s_current_theme.text_secondary = lv_color_black();
    s_current_theme.radius         = 2;
    s_current_theme.padding        = 4;
}

/* LVGL theme + style storage (heap-allocated via lv_theme_create) */
static lv_theme_t  *s_theme;
static lv_style_t  s_style_btn;
static lv_style_t  s_style_label;
static lv_style_t  s_style_panel;

/* -------------------------------------------------------------------------
 * LVGL theme apply callback
 * Called by LVGL for every widget — we pattern-match on widget class.
 * ------------------------------------------------------------------------- */
static void theme_apply_cb(lv_theme_t *th, lv_obj_t *obj)
{
    (void)th;

    if (lv_obj_check_type(obj, &lv_button_class)) {
        lv_obj_add_style(obj, &s_style_btn, LV_PART_MAIN);
    } else if (lv_obj_check_type(obj, &lv_label_class)) {
        lv_obj_add_style(obj, &s_style_label, LV_PART_MAIN);
    } else if (lv_obj_check_type(obj, &lv_obj_class)) {
        /* Plain lv_obj acts as a panel/container */
        lv_obj_add_style(obj, &s_style_panel, LV_PART_MAIN);
    }
}

/* -------------------------------------------------------------------------
 * Public API
 * ------------------------------------------------------------------------- */

esp_err_t theme_init(lv_display_t *disp)
{
    /* Button style: white fill, 1 px black border, black text */
    lv_style_init(&s_style_btn);
    lv_style_set_bg_color(&s_style_btn, lv_color_white());
    lv_style_set_bg_opa(&s_style_btn, LV_OPA_COVER);
    lv_style_set_border_color(&s_style_btn, lv_color_black());
    lv_style_set_border_width(&s_style_btn, 1);
    lv_style_set_text_color(&s_style_btn, lv_color_black());
    lv_style_set_radius(&s_style_btn, s_current_theme.radius);
    lv_style_set_pad_all(&s_style_btn, s_current_theme.padding);

    /* Label style: black text, transparent background */
    lv_style_init(&s_style_label);
    lv_style_set_text_color(&s_style_label, lv_color_black());
    lv_style_set_bg_opa(&s_style_label, LV_OPA_TRANSP);

    /* Panel/container style: white background, thin black border */
    lv_style_init(&s_style_panel);
    lv_style_set_bg_color(&s_style_panel, lv_color_white());
    lv_style_set_bg_opa(&s_style_panel, LV_OPA_COVER);
    lv_style_set_border_color(&s_style_panel, lv_color_black());
    lv_style_set_border_width(&s_style_panel, 1);
    lv_style_set_radius(&s_style_panel, 0);

    /* Initialize default colors */
    theme_colors_init_defaults();

    /* Create theme and wire into LVGL */
    s_theme = lv_theme_simple_init(disp);
    lv_theme_set_apply_cb(s_theme, theme_apply_cb);

    if (disp != NULL) {
        lv_display_set_theme(disp, s_theme);
    }

    ESP_LOGI(TAG, "default monochrome theme applied");
    return ESP_OK;
}

esp_err_t theme_load(const char *json_path)
{
    if (json_path == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    /* TODO: parse JSON, populate s_current_theme, call theme_apply() */
    ESP_LOGW(TAG, "theme loading not yet implemented (path: %s)", json_path);
    return ESP_OK;
}

const theme_colors_t *theme_get_colors(void)
{
    return &s_current_theme;
}

esp_err_t theme_apply(lv_display_t *disp)
{
    if (disp == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    lv_display_set_theme(disp, s_theme);
    ESP_LOGI(TAG, "theme applied to display");
    return ESP_OK;
}
