/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Vault app public interface
 *
 * AES-256-CBC encrypted password vault, protected by a PBKDF2-derived key.
 * Hardware AES acceleration used on ESP32-S3 via mbedtls.
 */
#pragma once
#include "esp_err.h"
#include "lvgl.h"

esp_err_t vault_app_register(void);
esp_err_t vault_ui_create(lv_obj_t *parent);
void      vault_ui_show(void);
void      vault_ui_hide(void);
void      vault_ui_destroy(void);
