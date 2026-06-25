/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Flashlight/SOS app public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t flashlight_app_register(void);
esp_err_t flashlight_ui_create(lv_obj_t *parent);
void flashlight_ui_show(void);
void flashlight_ui_hide(void);
void flashlight_ui_destroy(void);
