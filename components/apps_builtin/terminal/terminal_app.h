/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Terminal app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t terminal_app_register(void);
esp_err_t terminal_ui_create(lv_obj_t *parent);
void terminal_ui_show(void);
void terminal_ui_hide(void);
