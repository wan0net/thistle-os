#pragma once

#include "FreeRTOS.h"

/* Thread-safe queue — implemented in platform_stubs.c */
extern QueueHandle_t xQueueCreate(UBaseType_t length, UBaseType_t item_size);
extern BaseType_t xQueueSend(QueueHandle_t handle, const void *item, TickType_t timeout);
extern BaseType_t xQueueReceive(QueueHandle_t handle, void *item, TickType_t timeout);
