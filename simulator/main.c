/*
 * ThistleOS Simulator — SDL2 host application
 *
 * Runs the real ThistleOS UI in an SDL2 window for development/testing.
 * Display: 320x240 scaled 2x to 640x480 window.
 */
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <sys/time.h>

#include "lvgl.h"
#include "hal/board.h"
#include "thistle/kernel.h"
#include "ui/manager.h"

#define SIM_WIDTH  320
#define SIM_HEIGHT 240
#define SIM_ZOOM   2

/* Forward declarations */
extern esp_err_t board_init(void);  /* from board_simulator.c */

int main(int argc, char **argv)
{
    (void)argc;
    (void)argv;

    printf("ThistleOS Simulator starting...\n");

    /* Initialize kernel subsystems */
    kernel_init();

    /* Initialize UI (creates LVGL display, status bar, etc.) */
    ui_manager_init();

    printf("ThistleOS Simulator ready. Close window to exit.\n");

    /* Main loop — run LVGL tick handler */
    while (1) {
        lv_timer_handler();
        usleep(5000);  /* 5ms tick */
    }

    return 0;
}
