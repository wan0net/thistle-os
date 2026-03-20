#pragma once

#include "esp_err.h"
#include "lvgl.h"

/* Create the status bar on the given parent */
esp_err_t statusbar_create(lv_obj_t *parent);

/* Update status bar content */
void statusbar_set_battery(uint8_t percent, bool charging);
void statusbar_set_wifi(bool connected, int rssi);
void statusbar_set_title(const char *title);
void statusbar_set_time(uint8_t hour, uint8_t minute);

/* Get status bar height in pixels */
uint16_t statusbar_get_height(void);
