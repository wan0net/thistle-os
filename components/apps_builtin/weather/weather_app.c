/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Weather Station app lifecycle
 *
 * App ID  : com.thistle.weather
 * Name    : Weather
 */
#include "weather/weather_app.h"

#include "thistle/app_manager.h"
#include "ui/statusbar.h"
#include "esp_log.h"

static const char *TAG = "weather";

/* ------------------------------------------------------------------ */
/* Lifecycle callbacks                                                  */
/* ------------------------------------------------------------------ */

static int weather_on_create(void)
{
    ESP_LOGI(TAG, "on_create");
    extern lv_obj_t *ui_manager_get_app_area(void);
    weather_ui_create(ui_manager_get_app_area());
    return 0;
}

static void weather_on_start(void)
{
    ESP_LOGI(TAG, "on_start");
    statusbar_set_title("Weather");
    weather_ui_show();
}

static void weather_on_pause(void)
{
    ESP_LOGI(TAG, "on_pause");
    weather_ui_hide();
}

static void weather_on_resume(void)
{
    ESP_LOGI(TAG, "on_resume");
    statusbar_set_title("Weather");
    weather_ui_show();
}

static void weather_on_destroy(void)
{
    ESP_LOGI(TAG, "on_destroy");
    /* UI objects are cleaned up by LVGL when the app area parent is destroyed */
}

/* ------------------------------------------------------------------ */
/* App manifest                                                         */
/* ------------------------------------------------------------------ */

static const app_manifest_t weather_manifest = {
    .id               = "com.thistle.weather",
    .name             = "Weather",
    .version          = "0.1.0",
    .allow_background = false,
};

static app_entry_t weather_entry = {
    .manifest   = &weather_manifest,
    .on_create  = weather_on_create,
    .on_start   = weather_on_start,
    .on_pause   = weather_on_pause,
    .on_resume  = weather_on_resume,
    .on_destroy = weather_on_destroy,
};

esp_err_t weather_app_register(void)
{
    return app_manager_register(&weather_entry);
}
