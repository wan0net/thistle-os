/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Simulator GPS HAL driver — returns a static position (San Francisco by default).
 */
#include "sim_gps.h"
#include <stddef.h>
#include <time.h>

static double   s_lat  = 37.7749;
static double   s_lon  = -122.4194;
static float    s_alt  = 15.0f;
static uint8_t  s_sats = 10;
static bool     s_fix  = true;

static hal_gps_cb_t s_cb        = NULL;
static void        *s_cb_user   = NULL;

/* ---- vtable functions --------------------------------------------------- */

static esp_err_t sim_gps_init(const void *config)
{
    (void)config;
    return ESP_OK;
}

static void sim_gps_deinit(void)
{
}

static esp_err_t sim_gps_enable(void)
{
    return ESP_OK;
}

static esp_err_t sim_gps_disable(void)
{
    return ESP_OK;
}

static esp_err_t sim_gps_get_position(hal_gps_position_t *pos)
{
    if (!pos) return ESP_ERR_INVALID_ARG;

    pos->latitude    = s_lat;
    pos->longitude   = s_lon;
    pos->altitude_m  = s_alt;
    pos->speed_kmh   = 0.0f;
    pos->heading_deg = 0.0f;
    pos->satellites  = s_sats;
    pos->fix_valid   = s_fix;
    pos->timestamp   = (uint32_t)time(NULL);
    return ESP_OK;
}

static esp_err_t sim_gps_register_callback(hal_gps_cb_t cb, void *user_data)
{
    s_cb      = cb;
    s_cb_user = user_data;
    return ESP_OK;
}

static esp_err_t sim_gps_sleep(bool enter)
{
    (void)enter;
    return ESP_OK;
}

/* ---- driver instance ---------------------------------------------------- */

static const hal_gps_driver_t sim_gps_driver = {
    .init              = sim_gps_init,
    .deinit            = sim_gps_deinit,
    .enable            = sim_gps_enable,
    .disable           = sim_gps_disable,
    .get_position      = sim_gps_get_position,
    .register_callback = sim_gps_register_callback,
    .sleep             = sim_gps_sleep,
    .name              = "Simulator GPS",
};

const hal_gps_driver_t *sim_gps_get(void)
{
    return &sim_gps_driver;
}

void sim_gps_set_position(double lat, double lon, float alt, uint8_t sats, bool fix)
{
    s_lat  = lat;
    s_lon  = lon;
    s_alt  = alt;
    s_sats = sats;
    s_fix  = fix;
}
