#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t settings_app_register(void);
esp_err_t settings_ui_create(lv_obj_t *parent);
void settings_ui_show(void);
void settings_ui_hide(void);
void settings_ui_destroy(void);
