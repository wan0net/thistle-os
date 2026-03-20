#pragma once

#include "esp_err.h"

/* Initialize driver manager — calls board_init() which registers HAL drivers */
esp_err_t driver_manager_init(void);

/* Initialize all registered drivers */
esp_err_t driver_manager_start_all(void);

/* Deinitialize all registered drivers */
esp_err_t driver_manager_stop_all(void);
