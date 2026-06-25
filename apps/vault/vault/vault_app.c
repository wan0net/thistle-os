/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Vault app lifecycle
 *
 * Registers the vault with the ThistleOS app manager and wires the
 * standard on_create / on_start / on_pause / on_resume / on_destroy
 * lifecycle callbacks.  All UI logic lives in vault_ui.c.
 */
#include "vault/vault_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "vault";

/* ------------------------------------------------------------------ */
/* Forward declarations (vault_ui.c)                                   */
/* ------------------------------------------------------------------ */

extern void vault_ui_lock(void);   /* zero key + show lock screen */

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int vault_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    vault_ui_create(ui_manager_get_app_area());
    return 0;
}

static void vault_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Vault");
    vault_ui_show();
}

static void vault_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    /* Security: lock vault when app is backgrounded */
    vault_ui_lock();
    vault_ui_hide();
}

static void vault_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Vault");
    /* Vault is locked on resume — user must re-enter master password */
    vault_ui_show();
}

static void vault_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    vault_ui_lock();
    vault_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t vault_manifest = {
    .id               = "com.thistle.vault",
    .name             = "Vault",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t vault_entry = {
    .manifest   = &vault_manifest,
    .on_create  = vault_on_create,
    .on_start   = vault_on_start,
    .on_pause   = vault_on_pause,
    .on_resume  = vault_on_resume,
    .on_destroy = vault_on_destroy,
};

esp_err_t vault_app_register(void)
{
    return app_manager_register(&vault_entry);
}
