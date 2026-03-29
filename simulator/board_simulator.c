#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include "hal/board.h"
#include "sim_display.h"
#include "sim_input.h"
#include "sim_storage.h"
#include "sim_power.h"
#include "sim_gps.h"
#include "sim_imu.h"
#include "sim_rtc.h"
#include "sim_audio.h"
#include "sim_radio.h"
#include "sim_scenario.h"
#include "sim_i2c_bus.h"
#include "sim_spi_bus.h"
#include "sim_devices.h"

/* Forward declaration — defined in sim_network.c */
esp_err_t sim_network_register(void);

/* Selected device — set by main() via sim_board_set_device() before board_init() */
static const char *s_device = "tdeck";

void sim_board_set_device(const char *device)
{
    s_device = device;
}

typedef struct {
    const char *name;
    const char *board_name;
    int         width;
    int         height;
    bool        has_keyboard;
    bool        has_touch;
    bool        has_radio;
    bool        has_gps;
    bool        is_epaper;
} sim_device_t;

static const sim_device_t DEVICES[] = {
    { "tdeck-pro",  "T-Deck Pro (Simulator)",     320, 240, true,  true,  true,  true,  true  },
    { "tdeck",      "T-Deck (Simulator)",          320, 240, true,  true,  true,  true,  false },
    { "tdeck-plus", "T-Deck Plus (Simulator)",     320, 240, true,  true,  true,  true,  false },
    { "tdisplay",   "T-Display-S3 (Simulator)",    320, 170, false, true,  false, false, false },
    { "heltec-v3",  "Heltec V3 (Simulator)",       128, 64,  false, false, true,  false, false },
    { "cardputer",  "Cardputer (Simulator)",        240, 135, true,  false, false, false, false },
    { "t3-s3",      "T3-S3 (Simulator)",            128, 64,  false, false, true,  false, false },
    { "rak3312",    "RAK3312 (Simulator)",           128, 64,  false, false, true,  false, false },
    { NULL, NULL, 0, 0, false, false, false, false, false },
};

static const sim_device_t *find_device(const char *name)
{
    for (int i = 0; DEVICES[i].name != NULL; i++) {
        if (strcmp(DEVICES[i].name, name) == 0) return &DEVICES[i];
    }
    /* Default: tdeck (index 1) */
    return &DEVICES[1];
}

esp_err_t board_init(void)
{
    const sim_device_t *dev = find_device(s_device);
    printf("Simulator: %s (%dx%d)\n", dev->board_name, dev->width, dev->height);

    sim_display_set_resolution(dev->width, dev->height);
    sim_display_set_title(dev->board_name);

    hal_set_board_name(dev->board_name);
    hal_display_register(sim_display_get(), NULL);
    hal_input_register(sim_input_get(), NULL);
    hal_storage_register(sim_storage_get(), NULL);

    /* Register host network transport — net_manager_init() is called
     * by kernel_init() before driver_manager_init(), so the manager is
     * already initialized when board_init() runs. */
    sim_network_register();

    /* Register fake HAL drivers so apps never see NULL vtables */
    hal_power_register(sim_power_get(), NULL);
    hal_rtc_register(sim_rtc_get());
    hal_imu_register(sim_imu_get(), NULL);
    hal_audio_register(sim_audio_get(), NULL);
    if (dev->has_radio) hal_radio_register(sim_radio_get(), NULL);
    if (dev->has_gps)   hal_gps_register(sim_gps_get(), NULL);

    /* Initialize virtual buses and register device models */
    sim_i2c_bus_init();
    sim_spi_bus_init();
    hal_bus_register_i2c(0, sim_i2c_bus_get(0));
    hal_bus_register_spi(2, sim_spi_bus_get(0));

    /* Register I2C device models based on device capabilities */
    dev_pcf8563_register(0, 0x51);     /* RTC — all devices */
    if (dev->has_keyboard) dev_tca8418_register(0, 0x34);
    if (dev->has_touch)    dev_cst328_register(0, 0x1A);
    dev_qmi8658c_register(0, 0x6A);    /* IMU — all devices with accel */
    dev_ltr553_register(0, 0x23);      /* Light sensor */

    /* Apply scenario state to fake drivers */
    {
        uint16_t mv; uint8_t pct; int pstate;
        sim_scenario_get_power(&mv, &pct, &pstate);
        sim_power_set(mv, pct, (hal_power_state_t)pstate);
    }
    if (dev->has_gps) {
        double lat, lon; float alt; uint8_t sats; bool fix;
        sim_scenario_get_gps(&lat, &lon, &alt, &sats, &fix);
        sim_gps_set_position(lat, lon, alt, sats, fix);
    }
    {
        float accel[3], gyro[3];
        sim_scenario_get_imu(accel, gyro);
        sim_imu_set_accel(accel[0], accel[1], accel[2]);
        sim_imu_set_gyro(gyro[0], gyro[1], gyro[2]);
    }

    return ESP_OK;
}

/* Query device capabilities from main.c for conditional app loading */
bool sim_board_has_radio(void) { return find_device(s_device)->has_radio; }
bool sim_board_has_gps(void) { return find_device(s_device)->has_gps; }
bool sim_board_has_keyboard(void) { return find_device(s_device)->has_keyboard; }
bool sim_board_has_touch(void) { return find_device(s_device)->has_touch; }
bool sim_board_is_epaper(void) { return find_device(s_device)->is_epaper; }
