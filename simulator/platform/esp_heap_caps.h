#pragma once

#include <stdlib.h>
#include <stdint.h>

#define MALLOC_CAP_DMA       (1 << 0)
#define MALLOC_CAP_8BIT      (1 << 1)
#define MALLOC_CAP_SPIRAM    (1 << 2)
#define MALLOC_CAP_DEFAULT   (1 << 3)

static inline void *heap_caps_malloc(size_t size, uint32_t caps) {
    (void)caps;
    return malloc(size);
}

static inline size_t heap_caps_get_free_size(uint32_t caps) {
    (void)caps;
    return 4 * 1024 * 1024; /* Simulate 4 MB PSRAM free in simulator */
}
