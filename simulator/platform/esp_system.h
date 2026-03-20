#pragma once

#include <stdio.h>
#include <stdlib.h>

static inline void esp_restart(void) {
    printf("esp_restart() called — exiting simulator\n");
    exit(0);
}
