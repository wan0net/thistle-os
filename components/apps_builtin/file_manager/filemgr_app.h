#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t filemgr_app_register(void);
esp_err_t filemgr_ui_create(lv_obj_t *parent);
void filemgr_ui_show(void);
void filemgr_ui_hide(void);
