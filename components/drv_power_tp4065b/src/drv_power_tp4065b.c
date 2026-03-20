// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TP4065B power/battery driver

#include "drv_power_tp4065b.h"
#include "esp_log.h"
#include "esp_err.h"
#include "driver/gpio.h"
#include "esp_adc/adc_oneshot.h"
#include "esp_adc/adc_cali.h"
#include "esp_adc/adc_cali_scheme.h"
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

static const char *TAG = "tp4065b";

// Number of ADC samples to average for stable readings
#define ADC_SAMPLES        8

// Voltage divider ratio: battery voltage is divided by 2 before the ADC pin
#define VDIV_RATIO         2

// LiPo 1S discharge curve: {voltage_mv, percent}
// Used for linear interpolation between breakpoints
typedef struct {
    uint16_t mv;
    uint8_t  pct;
} lipo_point_t;

static const lipo_point_t s_lipo_curve[] = {
    { 4200, 100 },
    { 4060,  90 },
    { 3980,  80 },
    { 3920,  70 },
    { 3870,  60 },
    { 3820,  50 },
    { 3750,  40 },
    { 3700,  30 },
    { 3620,  20 },
    { 3500,  10 },
    { 3000,   0 },
};

#define LIPO_CURVE_LEN  (sizeof(s_lipo_curve) / sizeof(s_lipo_curve[0]))

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

static struct {
    power_tp4065b_config_t cfg;
    adc_oneshot_unit_handle_t adc_handle;
    adc_cali_handle_t         cali_handle;
    bool initialized;
    bool has_calibration;
} s_pwr;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

static bool tp4065b_cali_init(adc_unit_t unit, adc_channel_t channel,
                               adc_atten_t atten, adc_cali_handle_t *out)
{
    esp_err_t ret = ESP_FAIL;

#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
    adc_cali_curve_fitting_config_t cali_cfg = {
        .unit_id  = unit,
        .chan     = channel,
        .atten    = atten,
        .bitwidth = ADC_BITWIDTH_12,
    };
    ret = adc_cali_create_scheme_curve_fitting(&cali_cfg, out);
    if (ret == ESP_OK) {
        ESP_LOGI(TAG, "ADC calibration: curve fitting");
        return true;
    }
#endif

#if ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
    adc_cali_line_fitting_config_t line_cfg = {
        .unit_id  = unit,
        .atten    = atten,
        .bitwidth = ADC_BITWIDTH_12,
    };
    ret = adc_cali_create_scheme_line_fitting(&line_cfg, out);
    if (ret == ESP_OK) {
        ESP_LOGI(TAG, "ADC calibration: line fitting");
        return true;
    }
#endif

    ESP_LOGW(TAG, "ADC calibration not available (raw approximation will be used)");
    (void)ret;
    return false;
}

// Map a calibrated ADC voltage (in mV, after divider correction) to a
// battery percentage using linear interpolation of the LiPo discharge curve.
static uint8_t voltage_to_percent(uint16_t mv)
{
    // Above the top breakpoint
    if (mv >= s_lipo_curve[0].mv) {
        return 100;
    }
    // Below the bottom breakpoint
    if (mv <= s_lipo_curve[LIPO_CURVE_LEN - 1].mv) {
        return 0;
    }

    // Find the two surrounding points and interpolate linearly
    for (size_t i = 0; i < LIPO_CURVE_LEN - 1; i++) {
        const lipo_point_t *upper = &s_lipo_curve[i];
        const lipo_point_t *lower = &s_lipo_curve[i + 1];

        if (mv <= upper->mv && mv >= lower->mv) {
            uint16_t mv_range  = upper->mv  - lower->mv;
            uint8_t  pct_range = upper->pct - lower->pct;
            uint16_t mv_above_lower = mv - lower->mv;
            // Weighted interpolation, rounded to nearest integer
            uint8_t pct = lower->pct +
                (uint8_t)(((uint32_t)mv_above_lower * pct_range + mv_range / 2) / mv_range);
            return pct;
        }
    }

    return 0;
}

// Read the ADC, average over ADC_SAMPLES readings, apply calibration if
// available, then correct for the 2:1 voltage divider.
// Returns battery voltage in millivolts, or 0 on error.
static uint16_t read_battery_mv(void)
{
    if (!s_pwr.initialized) {
        return 0;
    }

    int32_t sum = 0;
    for (int i = 0; i < ADC_SAMPLES; i++) {
        int raw = 0;
        esp_err_t err = adc_oneshot_read(s_pwr.adc_handle, s_pwr.cfg.adc_channel, &raw);
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "adc_oneshot_read failed: %s", esp_err_to_name(err));
            return 0;
        }
        sum += raw;
    }
    int avg_raw = (int)(sum / ADC_SAMPLES);

    int adc_mv = 0;
    if (s_pwr.has_calibration) {
        esp_err_t err = adc_cali_raw_to_voltage(s_pwr.cali_handle, avg_raw, &adc_mv);
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "adc_cali_raw_to_voltage failed, falling back to raw");
            s_pwr.has_calibration = false;
        }
    }

    if (!s_pwr.has_calibration) {
        // Approximate: 12-bit ADC, Vref ~3300 mV
        adc_mv = (int)((int64_t)avg_raw * 3300 / 4095);
    }

    // Correct for the 2:1 voltage divider
    uint32_t bat_mv = (uint32_t)adc_mv * VDIV_RATIO;

    // Clamp to a sensible range to guard against noise / floating pin
    if (bat_mv > 4300) {
        bat_mv = 4300;
    }

    return (uint16_t)bat_mv;
}

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t tp4065b_init(const void *config)
{
    if (!config) {
        return ESP_ERR_INVALID_ARG;
    }

    memcpy(&s_pwr.cfg, config, sizeof(s_pwr.cfg));

    // --- ADC oneshot unit for ADC1 ---
    adc_oneshot_unit_init_cfg_t unit_cfg = {
        .unit_id = ADC_UNIT_1,
    };
    esp_err_t err = adc_oneshot_new_unit(&unit_cfg, &s_pwr.adc_handle);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "adc_oneshot_new_unit failed: %s", esp_err_to_name(err));
        return err;
    }

    // --- Configure the battery voltage channel ---
    adc_oneshot_chan_cfg_t chan_cfg = {
        .atten    = ADC_ATTEN_DB_12,   // full-scale ~3.3 V (covers ~2.1 V max divider output)
        .bitwidth = ADC_BITWIDTH_12,
    };
    err = adc_oneshot_config_channel(s_pwr.adc_handle, s_pwr.cfg.adc_channel, &chan_cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "adc_oneshot_config_channel failed: %s", esp_err_to_name(err));
        adc_oneshot_del_unit(s_pwr.adc_handle);
        s_pwr.adc_handle = NULL;
        return err;
    }

    // --- Calibration (best effort) ---
    s_pwr.has_calibration = tp4065b_cali_init(ADC_UNIT_1,
                                               s_pwr.cfg.adc_channel,
                                               ADC_ATTEN_DB_12,
                                               &s_pwr.cali_handle);

    // --- Charge status GPIO: input with internal pull-up ---
    gpio_config_t io_cfg = {
        .pin_bit_mask = (1ULL << s_pwr.cfg.pin_charge_status),
        .mode         = GPIO_MODE_INPUT,
        .pull_up_en   = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
    };
    err = gpio_config(&io_cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "gpio_config (charge status) failed: %s", esp_err_to_name(err));
        if (s_pwr.has_calibration) {
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
            adc_cali_delete_scheme_curve_fitting(s_pwr.cali_handle);
#elif ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
            adc_cali_delete_scheme_line_fitting(s_pwr.cali_handle);
#endif
        }
        adc_oneshot_del_unit(s_pwr.adc_handle);
        s_pwr.adc_handle = NULL;
        return err;
    }

    s_pwr.initialized = true;
    ESP_LOGI(TAG, "initialized (adc_ch=%d, chrg_gpio=%d, cali=%s)",
             s_pwr.cfg.adc_channel,
             s_pwr.cfg.pin_charge_status,
             s_pwr.has_calibration ? "yes" : "no");
    return ESP_OK;
}

static void tp4065b_deinit(void)
{
    if (!s_pwr.initialized) {
        return;
    }

    s_pwr.initialized = false;

    if (s_pwr.has_calibration) {
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
        adc_cali_delete_scheme_curve_fitting(s_pwr.cali_handle);
#elif ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
        adc_cali_delete_scheme_line_fitting(s_pwr.cali_handle);
#endif
        s_pwr.cali_handle     = NULL;
        s_pwr.has_calibration = false;
    }

    if (s_pwr.adc_handle) {
        adc_oneshot_del_unit(s_pwr.adc_handle);
        s_pwr.adc_handle = NULL;
    }

    ESP_LOGI(TAG, "deinitialized");
}

static uint16_t tp4065b_get_battery_mv(void)
{
    return read_battery_mv();
}

static uint8_t tp4065b_get_battery_percent(void)
{
    uint16_t mv = read_battery_mv();
    return voltage_to_percent(mv);
}

static bool tp4065b_is_charging(void)
{
    if (!s_pwr.initialized) {
        return false;
    }
    // TP4065B CHRG pin is open-drain, pulled low while charging
    return gpio_get_level(s_pwr.cfg.pin_charge_status) == 0;
}

static esp_err_t tp4065b_get_info(hal_power_info_t *info)
{
    if (!info) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_pwr.initialized) {
        return ESP_ERR_INVALID_STATE;
    }

    info->voltage_mv = read_battery_mv();
    info->percent    = voltage_to_percent(info->voltage_mv);

    bool charging = tp4065b_is_charging();
    if (charging) {
        info->state = HAL_POWER_STATE_CHARGING;
    } else if (info->percent >= 99) {
        info->state = HAL_POWER_STATE_CHARGED;
    } else {
        info->state = HAL_POWER_STATE_DISCHARGING;
    }

    ESP_LOGD(TAG, "voltage=%u mV  percent=%u%%  state=%d",
             info->voltage_mv, info->percent, info->state);
    return ESP_OK;
}

static esp_err_t tp4065b_sleep(bool enter)
{
    // TP4065B has no software sleep mode. ADC oneshot consumes no power when
    // idle, and GPIO inputs are always low power. Nothing to do here.
    ESP_LOGD(TAG, "sleep(%s) — no-op for TP4065B", enter ? "enter" : "exit");
    return ESP_OK;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_power_driver_t s_vtable = {
    .init                = tp4065b_init,
    .deinit              = tp4065b_deinit,
    .get_info            = tp4065b_get_info,
    .get_battery_mv      = tp4065b_get_battery_mv,
    .get_battery_percent = tp4065b_get_battery_percent,
    .is_charging         = tp4065b_is_charging,
    .sleep               = tp4065b_sleep,
    .name                = "TP4065B",
};

const hal_power_driver_t *drv_power_tp4065b_get(void)
{
    return &s_vtable;
}
