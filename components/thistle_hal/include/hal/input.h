#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef enum {
    HAL_INPUT_EVENT_KEY_DOWN,
    HAL_INPUT_EVENT_KEY_UP,
    HAL_INPUT_EVENT_TOUCH_DOWN,
    HAL_INPUT_EVENT_TOUCH_UP,
    HAL_INPUT_EVENT_TOUCH_MOVE,
} hal_input_event_type_t;

typedef struct {
    hal_input_event_type_t type;
    uint32_t timestamp;
    union {
        struct { uint16_t keycode; } key;
        struct { uint16_t x; uint16_t y; } touch;
    };
} hal_input_event_t;

typedef void (*hal_input_cb_t)(const hal_input_event_t *event, void *user_data);

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*register_callback)(hal_input_cb_t cb, void *user_data);
    esp_err_t (*poll)(void);
    const char *name;
    bool is_touch;
} hal_input_driver_t;
