/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Navigator app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t navigator_app_register(void);
esp_err_t navigator_ui_create(lv_obj_t *parent);
void navigator_ui_show(void);
void navigator_ui_hide(void);
