// SPDX-License-Identifier: BSD-3-Clause
#pragma once

#include <stdbool.h>

typedef enum {
    A7682E_LIFECYCLE_STOPPED = 0,
    A7682E_LIFECYCLE_STARTING,
    A7682E_LIFECYCLE_RUNNING,
    A7682E_LIFECYCLE_STOPPING,
} a7682e_lifecycle_state_t;

typedef enum {
    A7682E_INIT_ACQUIRED = 0,
    A7682E_INIT_ALREADY_RUNNING,
    A7682E_INIT_FAILED,
} a7682e_init_result_t;

/* The successful begin calls retain the process-lifetime lifecycle mutex.
 * Their matching finish call must always be made. */
a7682e_init_result_t a7682e_lifecycle_begin_init(void);
void a7682e_lifecycle_finish_init(bool success);

bool a7682e_lifecycle_begin_operation(void);
void a7682e_lifecycle_finish_operation(void);

bool a7682e_lifecycle_begin_stop(void);
void a7682e_lifecycle_finish_stop(void);
