#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t launcher_app_register(void);
esp_err_t launcher_ui_create(lv_obj_t *parent);
void launcher_ui_show(void);
void launcher_ui_hide(void);
