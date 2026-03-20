#include "hal/board.h"
#include "sim_display.h"
#include "sim_input.h"
#include "sim_storage.h"
#include <stdio.h>

esp_err_t board_init(void)
{
    printf("Simulator board init\n");

    hal_set_board_name("ThistleOS Simulator");
    hal_display_register(sim_display_get(), NULL);
    hal_input_register(sim_input_get(), NULL);
    hal_storage_register(sim_storage_get(), NULL);

    return ESP_OK;
}
