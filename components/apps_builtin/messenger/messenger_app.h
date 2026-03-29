/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t messenger_app_register(void);
esp_err_t messenger_ui_create(lv_obj_t *parent);
void messenger_ui_show(void);
void messenger_ui_hide(void);
void messenger_ui_destroy(void);
