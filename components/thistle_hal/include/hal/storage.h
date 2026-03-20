#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

typedef enum {
    HAL_STORAGE_TYPE_SD,
    HAL_STORAGE_TYPE_INTERNAL,
} hal_storage_type_t;

typedef struct {
    esp_err_t (*init)(const void *config);
    void (*deinit)(void);
    esp_err_t (*mount)(const char *mount_point);
    esp_err_t (*unmount)(void);
    bool (*is_mounted)(void);
    uint64_t (*get_total_bytes)(void);
    uint64_t (*get_free_bytes)(void);
    hal_storage_type_t type;
    const char *name;
} hal_storage_driver_t;
