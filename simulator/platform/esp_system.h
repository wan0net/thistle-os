#pragma once

#include <stdio.h>
#include <stdlib.h>

static inline void esp_restart(void) {
    printf("esp_restart() called — exiting simulator\n");
    exit(0);
}

static inline uint32_t esp_get_free_heap_size(void) {
    return 256 * 1024; /* Simulate 256 KB free heap in simulator */
}

/* Stub: hardware RNG not available in simulator — vault_ui uses fill_random()
 * wrapper instead, which calls rand() under SIMULATOR_BUILD. */
static inline uint32_t esp_random(void) {
    return (uint32_t)rand();
}
