#pragma once

#include "esp_err.h"
#include <stdint.h>

#define THISTLE_VERSION_MAJOR 0
#define THISTLE_VERSION_MINOR 1
#define THISTLE_VERSION_PATCH 0
#define THISTLE_VERSION_STRING "0.1.0"

/* Initialize kernel subsystems (event bus, IPC, app manager, driver manager) */
esp_err_t kernel_init(void);

/* Enter the kernel main loop (never returns). Runs LVGL tick, processes events. */
void kernel_run(void);

/* Get kernel uptime in milliseconds */
uint32_t kernel_uptime_ms(void);
