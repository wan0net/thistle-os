/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — App Store built-in application header
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t appstore_app_register(void);
esp_err_t appstore_ui_create(lv_obj_t *parent);
void appstore_ui_show(void);
void appstore_ui_hide(void);
