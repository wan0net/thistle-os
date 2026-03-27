#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include "hal/board.h"
#include "sim_display.h"
#include "sim_input.h"
#include "sim_storage.h"

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
    { "cyd-s022",   "CYD S022 (Simulator)",         240, 320, false, true,  false, false, false },
    { "cyd-s028",   "CYD S028 (Simulator)",         320, 240, false, true,  false, false, false },
    { "t3-s3",      "T3-S3 (Simulator)",            128, 64,  false, false, true,  false, false },
    { "c3-mini",    "C3-Mini (Simulator)",           128, 64,  false, false, false, false, false },
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

    return ESP_OK;
}
