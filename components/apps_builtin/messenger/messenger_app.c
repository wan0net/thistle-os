/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger app lifecycle
 */
#include "messenger/messenger_app.h"
#include "messenger/messenger_kernel.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "messenger";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int messenger_on_create(void)
{
    ESP_LOGI(TAG, "on_create");

    /* Initialise kernel messaging modules */
    int rc;
    rc = rs_contact_manager_init();
    if (rc != 0) ESP_LOGW(TAG, "contact_manager init: %d", rc);
    rc = rs_burn_timer_init();
    if (rc != 0) ESP_LOGW(TAG, "burn_timer init: %d", rc);
    rc = rs_msg_crypto_init();
    if (rc != 0) ESP_LOGW(TAG, "msg_crypto init: %d", rc);
    rc = rs_msg_queue_init();
    if (rc != 0) ESP_LOGW(TAG, "msg_queue init: %d", rc);

    extern lv_obj_t *ui_manager_get_app_area(void);
    messenger_ui_create(ui_manager_get_app_area());
    return 0;
}

static void messenger_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Messenger");
    messenger_ui_show();
}

static void messenger_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    messenger_ui_hide();
}

static void messenger_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Messenger");
    messenger_ui_show();
}

static void messenger_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    messenger_ui_destroy();
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t messenger_manifest = {
    .id               = "com.thistle.messenger",
    .name             = "Messenger",
    .version          = "0.1.0",
    .allow_background = true,
};

static app_entry_t messenger_entry = {
    .manifest   = &messenger_manifest,
    .on_create  = messenger_on_create,
    .on_start   = messenger_on_start,
    .on_pause   = messenger_on_pause,
    .on_resume  = messenger_on_resume,
    .on_destroy = messenger_on_destroy,
};

esp_err_t messenger_app_register(void)
{
    return app_manager_register(&messenger_entry);
}
