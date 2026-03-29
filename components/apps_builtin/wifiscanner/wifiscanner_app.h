/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — WiFi Scanner app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t wifiscanner_app_register(void);
esp_err_t wifiscanner_ui_create(lv_obj_t *parent);
void wifiscanner_ui_show(void);
void wifiscanner_ui_hide(void);
void wifiscanner_ui_destroy(void);
