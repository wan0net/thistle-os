#pragma once

#include <stdint.h>
#include <sys/time.h>
#include "esp_err.h"

typedef void (*esp_timer_cb_t)(void *arg);

typedef struct {
    esp_timer_cb_t callback;
    void *arg;
    const char *name;
    int dispatch_method;
    /* ignored in sim */
} esp_timer_create_args_t;

typedef struct esp_timer *esp_timer_handle_t;

#define ESP_TIMER_TASK 0

int64_t esp_timer_get_time(void);

/* Timer create/start are stubs — LVGL tick is driven by main loop */
esp_err_t esp_timer_create(const esp_timer_create_args_t *args, esp_timer_handle_t *handle);
esp_err_t esp_timer_start_periodic(esp_timer_handle_t handle, uint64_t period_us);
esp_err_t esp_timer_start_once(esp_timer_handle_t handle, uint64_t timeout_us);
esp_err_t esp_timer_delete(esp_timer_handle_t handle);
esp_err_t esp_timer_stop(esp_timer_handle_t handle);
