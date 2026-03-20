#pragma once

#include "hal/storage.h"
#include "driver/spi_master.h"
#include "driver/gpio.h"

typedef struct {
    spi_host_device_t spi_host;
    gpio_num_t pin_cs;
    const char *mount_point;    /* e.g., "/sdcard" */
    int max_files;              /* max open files, default 5 */
} sdcard_config_t;

const hal_storage_driver_t *drv_sdcard_get(void);
