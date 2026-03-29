/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — GhostTerm serial terminal public interface
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t ghostterm_app_register(void);
esp_err_t ghostterm_ui_create(lv_obj_t *parent);
void ghostterm_ui_show(void);
void ghostterm_ui_hide(void);
void ghostterm_uart_stop(void);
void ghostterm_uart_start(void);
