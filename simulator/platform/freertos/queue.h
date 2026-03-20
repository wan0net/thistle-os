#pragma once

#include "FreeRTOS.h"
#include <stdlib.h>
#include <string.h>

/* Simplified queue using a circular buffer — single-threaded in simulator */
typedef struct {
    uint8_t *buf;
    size_t item_size;
    size_t capacity;
    size_t head;
    size_t tail;
    size_t count;
} sim_queue_t;

static inline QueueHandle_t xQueueCreate(UBaseType_t length, UBaseType_t item_size) {
    sim_queue_t *q = (sim_queue_t *)calloc(1, sizeof(sim_queue_t));
    if (!q) return NULL;
    q->buf = (uint8_t *)calloc(length, item_size);
    if (!q->buf) { free(q); return NULL; }
    q->item_size = item_size;
    q->capacity = length;
    return (QueueHandle_t)q;
}

static inline BaseType_t xQueueSend(QueueHandle_t handle, const void *item, TickType_t timeout) {
    (void)timeout;
    sim_queue_t *q = (sim_queue_t *)handle;
    if (!q || q->count >= q->capacity) return pdFAIL;
    memcpy(q->buf + q->head * q->item_size, item, q->item_size);
    q->head = (q->head + 1) % q->capacity;
    q->count++;
    return pdPASS;
}

static inline BaseType_t xQueueReceive(QueueHandle_t handle, void *item, TickType_t timeout) {
    (void)timeout;
    sim_queue_t *q = (sim_queue_t *)handle;
    if (!q || q->count == 0) return pdFAIL;
    memcpy(item, q->buf + q->tail * q->item_size, q->item_size);
    q->tail = (q->tail + 1) % q->capacity;
    q->count--;
    return pdPASS;
}
