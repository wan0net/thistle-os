/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Reader app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t reader_app_register(void);
esp_err_t reader_ui_create(lv_obj_t *parent);
void reader_ui_show(void);
void reader_ui_hide(void);
