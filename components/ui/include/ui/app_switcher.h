#pragma once

#include "esp_err.h"
#include "lvgl.h"

/* Show the app switcher overlay */
esp_err_t app_switcher_show(void);

/* Hide the app switcher overlay */
esp_err_t app_switcher_hide(void);

/* Check if app switcher is visible */
bool app_switcher_is_visible(void);
