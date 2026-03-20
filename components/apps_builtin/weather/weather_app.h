/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Weather Station app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t weather_app_register(void);
esp_err_t weather_ui_create(lv_obj_t *parent);
void weather_ui_show(void);
void weather_ui_hide(void);
