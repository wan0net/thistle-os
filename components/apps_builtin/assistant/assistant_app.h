/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — AI Assistant app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t assistant_app_register(void);
esp_err_t assistant_ui_create(lv_obj_t *parent);
void assistant_ui_show(void);
void assistant_ui_hide(void);
void assistant_ui_destroy(void);
