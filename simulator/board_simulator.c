#include "hal/board.h"
#include "sim_display.h"
#include "sim_input.h"
#include "sim_storage.h"
#include <stdio.h>

/* Forward declaration — defined in sim_network.c */
esp_err_t sim_network_register(void);

esp_err_t board_init(void)
{
    printf("Simulator board init\n");

    hal_set_board_name("ThistleOS Simulator");
    hal_display_register(sim_display_get(), NULL);
    hal_input_register(sim_input_get(), NULL);
    hal_storage_register(sim_storage_get(), NULL);

    /* Register host network transport — net_manager_init() is called
     * by kernel_init() before driver_manager_init(), so the manager is
     * already initialized when board_init() runs. */
    sim_network_register();

    return ESP_OK;
}
