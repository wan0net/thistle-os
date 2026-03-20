#include "file_manager/filemgr_app.h"

#include "lvgl.h"
#include "esp_log.h"

static const char *TAG = "filemgr_ui";

static lv_obj_t *s_root = NULL;

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t filemgr_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating file manager UI (stub)");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    s_root = lv_obj_create(parent);
    lv_obj_set_size(s_root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(s_root, LV_OPA_TRANSP, 0);
    lv_obj_set_flex_flow(s_root, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_root,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    lv_obj_t *lbl = lv_label_create(s_root);
    lv_label_set_text(lbl, "File Manager -- Not yet implemented");

    return ESP_OK;
}

void filemgr_ui_show(void)
{
    if (s_root) {
        lv_obj_clear_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}

void filemgr_ui_hide(void)
{
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
