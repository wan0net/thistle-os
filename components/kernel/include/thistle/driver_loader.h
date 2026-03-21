// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors
#pragma once

#include "esp_err.h"

/* Initialize the driver loader subsystem */
esp_err_t driver_loader_init(void);

/* Scan /sdcard/drivers/ and load all .drv.elf files.
 * Each driver ELF is loaded, symbols resolved against the syscall table,
 * and its entry point called. The entry point should register HAL vtables.
 * Returns the number of drivers successfully loaded. */
int driver_loader_scan_and_load(void);

/* Load a single driver ELF from the given path.
 * Returns ESP_OK if the driver was loaded and initialized successfully. */
esp_err_t driver_loader_load(const char *path);

/* Load a driver with JSON config (from board.json).
 * The config is available to the driver via thistle_driver_get_config(). */
esp_err_t driver_loader_load_with_config(const char *path, const char *config_json);

/* Get the current driver config JSON string.
 * Called by the driver during init to retrieve its board.json config.
 * Returns "{}" if no config was provided. */
const char *driver_loader_get_config(void);

/* Return the number of runtime drivers currently loaded */
int driver_loader_get_count(void);
