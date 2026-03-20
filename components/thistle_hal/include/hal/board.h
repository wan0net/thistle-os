#pragma once

#include "esp_err.h"
#include "hal/display.h"
#include "hal/input.h"
#include "hal/radio.h"
#include "hal/gps.h"
#include "hal/audio.h"
#include "hal/power.h"
#include "hal/imu.h"
#include "hal/storage.h"

#define HAL_MAX_INPUT_DRIVERS  4
#define HAL_MAX_STORAGE_DRIVERS 2

/* HAL registry — holds pointers to all registered drivers and their configs */
typedef struct {
    const hal_display_driver_t *display;
    const void                 *display_config;
    const hal_input_driver_t   *inputs[HAL_MAX_INPUT_DRIVERS];
    const void                 *input_configs[HAL_MAX_INPUT_DRIVERS];
    uint8_t                     input_count;
    const hal_radio_driver_t   *radio;
    const void                 *radio_config;
    const hal_gps_driver_t     *gps;
    const void                 *gps_config;
    const hal_audio_driver_t   *audio;
    const void                 *audio_config;
    const hal_power_driver_t   *power;
    const void                 *power_config;
    const hal_imu_driver_t     *imu;
    const void                 *imu_config;
    const hal_storage_driver_t *storage[HAL_MAX_STORAGE_DRIVERS];
    const void                 *storage_configs[HAL_MAX_STORAGE_DRIVERS];
    uint8_t                     storage_count;
    const char                 *board_name;
} hal_registry_t;

/* Global HAL registry access */
const hal_registry_t *hal_get_registry(void);

/* Registration functions — called by board init */
esp_err_t hal_display_register(const hal_display_driver_t *driver, const void *config);
esp_err_t hal_input_register(const hal_input_driver_t *driver, const void *config);
esp_err_t hal_radio_register(const hal_radio_driver_t *driver, const void *config);
esp_err_t hal_gps_register(const hal_gps_driver_t *driver, const void *config);
esp_err_t hal_audio_register(const hal_audio_driver_t *driver, const void *config);
esp_err_t hal_power_register(const hal_power_driver_t *driver, const void *config);
esp_err_t hal_imu_register(const hal_imu_driver_t *driver, const void *config);
esp_err_t hal_storage_register(const hal_storage_driver_t *driver, const void *config);
esp_err_t hal_set_board_name(const char *name);

/* Board init — implemented by board_* component */
esp_err_t board_init(void);
