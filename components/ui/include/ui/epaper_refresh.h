#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/* Initialize e-paper refresh tracker */
esp_err_t epaper_refresh_init(uint16_t display_width, uint16_t display_height);

/* Mark an area as dirty */
void epaper_refresh_mark_dirty(uint16_t x1, uint16_t y1, uint16_t x2, uint16_t y2);

/* Mark entire screen as dirty */
void epaper_refresh_mark_full(void);

/* Check if any area is dirty */
bool epaper_refresh_is_dirty(void);

/* Get the bounding box of all dirty areas */
void epaper_refresh_get_bounds(uint16_t *x1, uint16_t *y1, uint16_t *x2, uint16_t *y2);

/* Clear dirty state after refresh */
void epaper_refresh_clear(void);

/* Get refresh counter (increments with each refresh cycle) */
uint32_t epaper_refresh_get_count(void);
