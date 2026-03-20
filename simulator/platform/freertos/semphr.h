#pragma once

#include "FreeRTOS.h"
#include <pthread.h>
#include <stdlib.h>

/* Use pthread mutex for simulator semaphores */

static inline SemaphoreHandle_t xSemaphoreCreateMutex(void) {
    pthread_mutex_t *mtx = (pthread_mutex_t *)malloc(sizeof(pthread_mutex_t));
    if (mtx) pthread_mutex_init(mtx, NULL);
    return (SemaphoreHandle_t)mtx;
}

static inline BaseType_t xSemaphoreTake(SemaphoreHandle_t sem, TickType_t timeout) {
    (void)timeout;
    if (!sem) return pdTRUE;
    pthread_mutex_lock((pthread_mutex_t *)sem);
    return pdTRUE;
}

static inline BaseType_t xSemaphoreGive(SemaphoreHandle_t sem) {
    if (!sem) return pdTRUE;
    pthread_mutex_unlock((pthread_mutex_t *)sem);
    return pdTRUE;
}
