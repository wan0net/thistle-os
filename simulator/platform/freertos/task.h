#pragma once

#include "FreeRTOS.h"
#include "esp_timer.h"
#include <unistd.h>

/* Implemented in platform_stubs.c with real pthreads */
extern BaseType_t xTaskCreate(void (*fn)(void*), const char *name, uint32_t stack,
                               void *arg, UBaseType_t prio, TaskHandle_t *handle);
extern void vTaskDelay(TickType_t ticks);
extern void vTaskDelete(TaskHandle_t handle);

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
