// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Simulator — fake RTC HAL driver (host clock)
#include "sim_rtc.h"
#include <time.h>
#include <sys/time.h>
#include <stdbool.h>

// Offset (in seconds) from host clock, updated by set_time.
static time_t s_offset = 0;

static esp_err_t sim_rtc_init(const void *config)
{
    (void)config;
    s_offset = 0;
    return ESP_OK;
}

static void sim_rtc_deinit(void)
{
}

static esp_err_t sim_rtc_get_time(hal_datetime_t *dt)
{
    if (!dt) return ESP_ERR_INVALID_ARG;

    struct timeval tv;
    gettimeofday(&tv, NULL);
    time_t now = tv.tv_sec + s_offset;

    struct tm tm;
    gmtime_r(&now, &tm);

    dt->year    = (uint16_t)(tm.tm_year + 1900);
    dt->month   = (uint8_t)(tm.tm_mon + 1);
    dt->day     = (uint8_t)tm.tm_mday;
    dt->weekday = (uint8_t)tm.tm_wday;
    dt->hour    = (uint8_t)tm.tm_hour;
    dt->minute  = (uint8_t)tm.tm_min;
    dt->second  = (uint8_t)tm.tm_sec;

    return ESP_OK;
}

static esp_err_t sim_rtc_set_time(const hal_datetime_t *dt)
{
    if (!dt) return ESP_ERR_INVALID_ARG;

    struct tm tm = {
        .tm_year = dt->year - 1900,
        .tm_mon  = dt->month - 1,
        .tm_mday = dt->day,
        .tm_hour = dt->hour,
        .tm_min  = dt->minute,
        .tm_sec  = dt->second,
        .tm_isdst = 0,
    };
    time_t target = timegm(&tm);

    struct timeval tv;
    gettimeofday(&tv, NULL);
    s_offset = target - tv.tv_sec;

    return ESP_OK;
}

static bool sim_rtc_is_valid(void)
{
    return true;
}

static const hal_rtc_driver_t s_driver = {
    .init     = sim_rtc_init,
    .deinit   = sim_rtc_deinit,
    .get_time = sim_rtc_get_time,
    .set_time = sim_rtc_set_time,
    .is_valid = sim_rtc_is_valid,
    .name     = "Simulator RTC (host clock)",
};

const hal_rtc_driver_t *sim_rtc_get(void)
{
    return &s_driver;
}
