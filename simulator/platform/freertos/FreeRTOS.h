#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>

typedef int BaseType_t;
typedef unsigned int UBaseType_t;
typedef uint32_t TickType_t;
typedef void *TaskHandle_t;
typedef void *SemaphoreHandle_t;
typedef void *QueueHandle_t;

#define pdTRUE  1
#define pdFALSE 0
#define pdPASS  1
#define pdFAIL  0
#define portMAX_DELAY 0xFFFFFFFF
#define portMUX_TYPE int
#define portMUX_INITIALIZER_UNLOCKED 0

#define pdMS_TO_TICKS(ms) ((TickType_t)(ms))

#define configMINIMAL_STACK_SIZE 256
#define configMAX_PRIORITIES 25

/* Port macros — no-ops in simulator */
#define portENTER_CRITICAL(mux) (void)(mux)
#define portEXIT_CRITICAL(mux)  (void)(mux)
#define portYIELD_FROM_ISR(x)   (void)(x)

#define IRAM_ATTR
