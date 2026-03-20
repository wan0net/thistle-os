#pragma once

#include "FreeRTOS.h"
#include "esp_timer.h"
#include <unistd.h>

static inline BaseType_t xTaskCreate(
    void (*fn)(void*), const char *name, uint32_t stack,
    void *arg, UBaseType_t prio, TaskHandle_t *handle)
{
    (void)fn; (void)name; (void)stack; (void)arg; (void)prio;
    if (handle) *handle = NULL;
    /* In the simulator, tasks are not actually created —
     * the kernel runs single-threaded in the main loop.
     * For tasks that need to run (like LVGL handler),
     * we handle them in main.c directly. */
    return pdPASS;
}

static inline void vTaskDelay(TickType_t ticks) {
    usleep(ticks * 1000);
}

static inline void vTaskDelete(TaskHandle_t handle) {
    (void)handle;
}

static inline TickType_t xTaskGetTickCount(void) {
    return (TickType_t)(esp_timer_get_time() / 1000);
}

static inline UBaseType_t ulTaskNotifyTake(BaseType_t clear, TickType_t timeout) {
    (void)clear; (void)timeout;
    return 0;
}

static inline void vTaskNotifyGiveFromISR(TaskHandle_t task, BaseType_t *wake) {
    (void)task;
    if (wake) *wake = pdFALSE;
}
