#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <inttypes.h>
#include <stdio.h>

typedef int esp_err_t;

#define ESP_OK          0
#define ESP_FAIL        (-1)
#define ESP_ERR_NO_MEM          0x101
#define ESP_ERR_INVALID_ARG     0x102
#define ESP_ERR_INVALID_STATE   0x103
#define ESP_ERR_INVALID_SIZE    0x104
#define ESP_ERR_NOT_FOUND       0x105
#define ESP_ERR_NOT_SUPPORTED   0x106
#define ESP_ERR_TIMEOUT         0x107

#define ESP_ERROR_CHECK(x) do { \
    esp_err_t __err = (x); \
    if (__err != ESP_OK) { \
        printf("ESP_ERROR_CHECK failed: %d at %s:%d\n", __err, __FILE__, __LINE__); \
    } \
} while(0)

static inline const char *esp_err_to_name(esp_err_t code) {
    switch(code) {
        case ESP_OK:                return "ESP_OK";
        case ESP_FAIL:              return "ESP_FAIL";
        case ESP_ERR_NO_MEM:        return "ESP_ERR_NO_MEM";
        case ESP_ERR_INVALID_ARG:   return "ESP_ERR_INVALID_ARG";
        case ESP_ERR_INVALID_STATE: return "ESP_ERR_INVALID_STATE";
        case ESP_ERR_NOT_FOUND:     return "ESP_ERR_NOT_FOUND";
        case ESP_ERR_NOT_SUPPORTED: return "ESP_ERR_NOT_SUPPORTED";
        case ESP_ERR_TIMEOUT:       return "ESP_ERR_TIMEOUT";
        default:                    return "UNKNOWN";
    }
}
