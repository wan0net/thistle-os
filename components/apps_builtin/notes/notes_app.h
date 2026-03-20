/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Notes app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t notes_app_register(void);
esp_err_t notes_ui_create(lv_obj_t *parent);
void notes_ui_show(void);
void notes_ui_hide(void);
