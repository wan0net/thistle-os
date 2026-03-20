#pragma once

#include "esp_err.h"

#define ESP_RETURN_ON_ERROR(x, tag, fmt, ...) \
    do { \
        esp_err_t __e = (x); \
        if (__e != ESP_OK) return __e; \
    } while(0)
