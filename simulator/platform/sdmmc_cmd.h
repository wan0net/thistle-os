#pragma once

#include <stdio.h>

typedef struct { int dummy; } sdmmc_card_t;
typedef struct { int dummy; } sdmmc_host_t;

#define SDSPI_HOST_DEFAULT() (sdmmc_host_t){0}

static inline void sdmmc_card_print_info(void *f, sdmmc_card_t *card) {
    (void)f; (void)card;
}
