#include "ui/toast.h"
#include "ui/manager.h"
#include "lvgl.h"
#include "esp_log.h"
#include <string.h>

static const char *TAG = "toast";

static lv_obj_t *s_toast_obj = NULL;
static lv_timer_t *s_dismiss_timer = NULL;

/* Timer callback — auto-dismiss */
static void toast_dismiss_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    toast_dismiss();
}

void toast_dismiss(void)
{
    if (s_dismiss_timer) {
        lv_timer_delete(s_dismiss_timer);
        s_dismiss_timer = NULL;
    }
    if (s_toast_obj) {
        lv_obj_delete(s_toast_obj);
        s_toast_obj = NULL;
    }
}

esp_err_t toast_show(const char *message, toast_level_t level, uint32_t duration_ms)
{
    if (!message) return ESP_ERR_INVALID_ARG;

    /* Dismiss any existing toast first */
    toast_dismiss();

    lv_obj_t *screen = ui_manager_get_screen();
    if (!screen) return ESP_ERR_INVALID_STATE;

    /* Create toast container — positioned at bottom of screen, centered */
    s_toast_obj = lv_obj_create(screen);
    lv_obj_set_size(s_toast_obj, 280, LV_SIZE_CONTENT);
    lv_obj_set_style_pad_all(s_toast_obj, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_toast_obj, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_toast_obj, 6, LV_PART_MAIN);

    /* Position: bottom center, above the dock area */
    lv_obj_align(s_toast_obj, LV_ALIGN_BOTTOM_MID, 0, -70);

    /* Style based on level */
    if (level == TOAST_WARNING || level == TOAST_ERROR) {
        /* Inverted: black bg, white text */
        lv_obj_set_style_bg_color(s_toast_obj, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_toast_obj, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_color(s_toast_obj, lv_color_white(), LV_PART_MAIN);
    } else {
        /* Normal: white bg, black text, black border */
        lv_obj_set_style_bg_color(s_toast_obj, lv_color_white(), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_toast_obj, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_color(s_toast_obj, lv_color_black(), LV_PART_MAIN);
    }

    lv_obj_set_style_border_width(s_toast_obj, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(s_toast_obj, 4, LV_PART_MAIN);

    /* Bring to front */
    lv_obj_move_foreground(s_toast_obj);

    /* Build message with prefix based on level */
    char display_msg[128];
    const char *prefix = "";
    switch (level) {
        case TOAST_SUCCESS: prefix = "OK "; break;
        case TOAST_WARNING: prefix = "! ";  break;
        case TOAST_ERROR:   prefix = "X ";  break;
        default: break;
    }
    snprintf(display_msg, sizeof(display_msg), "%s%s", prefix, message);

    /* Label */
    lv_obj_t *label = lv_label_create(s_toast_obj);
    lv_label_set_text(label, display_msg);
    lv_label_set_long_mode(label, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(label, 260);
    lv_obj_set_style_text_font(label, &lv_font_montserrat_14, LV_PART_MAIN);

    if (level == TOAST_WARNING || level == TOAST_ERROR) {
        lv_obj_set_style_text_color(label, lv_color_white(), LV_PART_MAIN);
    } else {
        lv_obj_set_style_text_color(label, lv_color_black(), LV_PART_MAIN);
    }

    /* Auto-dismiss timer */
    s_dismiss_timer = lv_timer_create(toast_dismiss_timer_cb, duration_ms, NULL);
    lv_timer_set_repeat_count(s_dismiss_timer, 1);  /* fire once */

    ESP_LOGD(TAG, "toast: %s", message);
    return ESP_OK;
}

esp_err_t toast_info(const char *message)
{
    return toast_show(message, TOAST_INFO, 3000);
}

esp_err_t toast_warn(const char *message)
{
    return toast_show(message, TOAST_WARNING, 4000);
}
