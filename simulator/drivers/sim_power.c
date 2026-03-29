// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Simulator — fake power HAL driver
#include "sim_power.h"
#include <stdbool.h>

static uint16_t          s_voltage_mv = 3850;
static uint8_t           s_percent    = 72;
static hal_power_state_t s_state      = HAL_POWER_STATE_DISCHARGING;

static esp_err_t sim_power_init(const void *config)
{
    (void)config;
    return ESP_OK;
}

static void sim_power_deinit(void)
{
}

static esp_err_t sim_power_get_info(hal_power_info_t *info)
{
    if (!info) return ESP_ERR_INVALID_ARG;
    info->voltage_mv = s_voltage_mv;
    info->percent    = s_percent;
    info->state      = s_state;
    return ESP_OK;
}

static uint16_t sim_power_get_battery_mv(void)
{
    return s_voltage_mv;
}

static uint8_t sim_power_get_battery_percent(void)
{
    return s_percent;
}

static bool sim_power_is_charging(void)
{
    return s_state == HAL_POWER_STATE_CHARGING;
}

static esp_err_t sim_power_sleep(bool enter)
{
    (void)enter;
    return ESP_OK;
}

static const hal_power_driver_t s_driver = {
    .init                = sim_power_init,
    .deinit              = sim_power_deinit,
    .get_info            = sim_power_get_info,
    .get_battery_mv      = sim_power_get_battery_mv,
    .get_battery_percent = sim_power_get_battery_percent,
    .is_charging         = sim_power_is_charging,
    .sleep               = sim_power_sleep,
    .name                = "Simulator Power",
};

const hal_power_driver_t *sim_power_get(void)
{
    return &s_driver;
}

void sim_power_set(uint16_t voltage_mv, uint8_t percent, hal_power_state_t state)
{
    s_voltage_mv = voltage_mv;
    s_percent    = percent;
    s_state      = state;
}
