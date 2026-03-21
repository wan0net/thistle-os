// SPDX-License-Identifier: BSD-3-Clause
#pragma once

#include "esp_err.h"

/* Initialize board from JSON config file.
 * Reads board.json from config_path (typically from SPIFFS),
 * initializes SPI/I2C buses, and loads drivers.
 *
 * This replaces the compiled-in board_init() approach.
 * Falls back to compiled board_init() if board.json is not found. */
esp_err_t board_config_init(const char *config_path);

/* Get the board name as read from board.json */
const char *board_config_get_name(void);
